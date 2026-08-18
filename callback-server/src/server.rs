//! HTTP server for receiving UPnP event notifications.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequest, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use if_addrs::IfAddr;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use super::router::{EventRouter, NotificationPayload};

/// The only HTTP method this server serves.
///
/// UPnP's NOTIFY is not a standard HTTP verb, so it is not in `http::Method`'s set
/// of constants and axum's `MethodFilter` cannot express it. Comparing
/// `Method::as_str()` against this constant is the whole gate: no `Method` value is
/// constructed per request, which is what the old
/// `Method::from_bytes(b"NOTIFY").unwrap()` did on every single call.
const NOTIFY_METHOD: &str = "NOTIFY";

/// Maximum accepted size of a UPnP NOTIFY body, in bytes (64 KiB).
///
/// The endpoint is unauthenticated by design (UPnP eventing has no auth), so any
/// host on the LAN can POST to it. Without a limit the whole body is buffered and
/// then a second full copy is made when it is decoded to a `String` — two
/// concurrent 1 GB posts would be ~4 GB resident.
///
/// Sizing: real Sonos propertysets are ~1-5 KB. The largest realistic case is a
/// `ZoneGroupTopology` event, whose double-escaped `ZoneGroupState` payload runs
/// a few hundred bytes per player; at the 32-player household maximum that is
/// roughly 20-30 KB. 64 KiB leaves ~2x headroom over that worst case and ~10x
/// over typical events, while capping the cost of a hostile request at 128 KiB
/// (bytes + string copy) instead of unbounded.
///
/// Enforced in two independent places, deliberately. [`declared_content_length`]
/// refuses on the declared size before a single body byte is read, which is also
/// what turns a body with no declared size into 411 Length Required rather than a
/// read of unknown length (chunked bodies; Sonos never sends them).
/// [`DefaultBodyLimit`] then caps the bytes actually read, so the ceiling holds even
/// if that header check is later changed or bypassed.
const MAX_NOTIFY_BODY_BYTES: usize = 64 * 1024;

/// How many bytes of event XML to include in trace-level logs.
const TRACE_PREVIEW_BYTES: usize = 200;

/// Truncate `s` to at most `max` bytes, snapping back to a UTF-8 char boundary.
///
/// Slicing a `&str` at a byte index that falls inside a multi-byte codepoint
/// panics. Event XML routinely carries non-ASCII track metadata, so a naive
/// `&s[..max]` in the trace-logging path is a reachable panic.
fn preview(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// An IPv4 interface as (address, netmask).
///
/// A pair rather than `if_addrs::Interface` so the selection logic below is a pure
/// function over synthetic data and can be tested without real NICs.
type Interface = (Ipv4Addr, Ipv4Addr);

/// Enumerate IPv4 interface addresses that could plausibly reach a speaker.
///
/// Loopback is excluded because a speaker cannot reach it, link-local
/// (169.254.0.0/16) because it indicates a failed DHCP lease, and unspecified
/// because it is not an address.
///
/// This mirrors `usable_interfaces()` / `is_usable_ipv4()` in
/// `sonos-discovery/src/ssdp.rs`. The ~15 lines are duplicated deliberately:
/// `callback-server` sits *below* `sonos-discovery` in the dependency graph, so
/// importing the helper from there would invert the layering for the sake of a
/// filter predicate.
fn usable_interfaces() -> Vec<Interface> {
    let addrs = match if_addrs::get_if_addrs() {
        Ok(addrs) => addrs,
        Err(e) => {
            error!(error = %e, "Failed to enumerate network interfaces");
            return Vec::new();
        }
    };

    let mut interfaces: Vec<Interface> = addrs
        .into_iter()
        .filter_map(|iface| match iface.addr {
            IfAddr::V4(v4) if is_usable_ipv4(&v4.ip) => Some((v4.ip, v4.netmask)),
            _ => None,
        })
        .collect();

    interfaces.sort();
    interfaces.dedup();
    interfaces
}

/// Whether an IPv4 address belongs to an interface that could reach a speaker.
fn is_usable_ipv4(addr: &Ipv4Addr) -> bool {
    !addr.is_loopback() && !addr.is_link_local() && !addr.is_unspecified()
}

/// Prefix length of a netmask, used only to rank specificity.
fn prefix_len(netmask: Ipv4Addr) -> u32 {
    u32::from_be_bytes(netmask.octets()).count_ones()
}

/// Whether `target` falls inside the subnet described by `(addr, netmask)`.
///
/// The interface's *actual* netmask is used. Assuming /24 would be wrong on this
/// project's own test network, which is a single /22 (`192.168.4.0/22`): a speaker
/// at `192.168.5.19` is directly reachable from an interface at `192.168.4.32`,
/// but a /24 comparison would call it off-net.
fn contains(addr: Ipv4Addr, netmask: Ipv4Addr, target: Ipv4Addr) -> bool {
    let mask = u32::from_be_bytes(netmask.octets());
    (u32::from_be_bytes(addr.octets()) & mask) == (u32::from_be_bytes(target.octets()) & mask)
}

/// Pick the local address a speaker at `target` would see us on.
///
/// Selects the interface whose subnet actually contains `target`, most specific
/// prefix first. Returns `None` when no interface is on the target's subnet, which
/// is the honest answer: we have no address that speaker can reach directly.
///
/// A pure function over `(addr, netmask)` pairs so the /22 and VPN cases are
/// provable offline.
fn select_interface(interfaces: &[Interface], target: Ipv4Addr) -> Option<Ipv4Addr> {
    interfaces
        .iter()
        .filter(|(addr, netmask)| contains(*addr, *netmask, target))
        .max_by_key(|(_, netmask)| prefix_len(*netmask))
        .map(|(addr, _)| *addr)
}

/// Pick a local address when no target speaker is known yet.
///
/// The callback server is constructed before any speaker is registered, so there
/// is no target to match against; this is the target-less fallback that
/// [`CallbackServer::base_url`] is built from.
///
/// Preference order — RFC 1918 private, then anything else, then CGNAT
/// (100.64.0.0/10) last. CGNAT is deprioritised because that is where VPN tunnel
/// interfaces (e.g. Tailscale) live, and a tunnel address is exactly what the old
/// route-to-8.8.8.8 probe used to return: a plausible-looking address that no
/// speaker on the LAN can reach. Ties break on address value so the choice is
/// deterministic across runs.
fn preferred_local_ip(interfaces: &[Interface]) -> Option<Ipv4Addr> {
    interfaces
        .iter()
        .min_by_key(|(addr, _)| {
            let class = if is_cgnat(addr) {
                2
            } else if addr.is_private() {
                0
            } else {
                1
            };
            (class, u32::from_be_bytes(addr.octets()))
        })
        .map(|(addr, _)| *addr)
}

/// Whether an address is in the carrier-grade NAT range (100.64.0.0/10).
fn is_cgnat(addr: &Ipv4Addr) -> bool {
    let [a, b, ..] = addr.octets();
    a == 100 && (64..128).contains(&b)
}

/// HTTP callback server for receiving UPnP event notifications.
///
/// The `CallbackServer` binds to a local port and provides an HTTP endpoint
/// for receiving UPnP NOTIFY requests. It validates UPnP headers and routes
/// events through an `EventRouter` to a channel.
///
/// # Example
///
/// ```no_run
/// use tokio::sync::mpsc;
/// use callback_server::{CallbackServer, NotificationPayload};
///
/// #[tokio::main]
/// async fn main() {
///     let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();
///     
///     let server = CallbackServer::new((3400, 3500), tx)
///         .await
///         .expect("Failed to create callback server");
///     
///     println!("Server listening at: {}", server.base_url());
///     
///     // Process notifications
///     while let Some(notification) = rx.recv().await {
///         println!("Received event for subscription: {}", notification.subscription_id);
///     }
/// }
/// ```
pub struct CallbackServer {
    /// The port the server is bound to
    port: u16,
    /// The base URL for callback registration
    base_url: String,
    /// Event router for handling incoming events
    event_router: Arc<EventRouter>,
    /// Shutdown signal sender
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// Server task handle
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl CallbackServer {
    /// Create and start a new unified callback server.
    ///
    /// This method creates a single HTTP server that efficiently handles all UPnP
    /// event notifications from multiple speakers and services. The server:
    /// - Finds an available port in the specified range
    /// - Detects the local IP address for callback URLs
    /// - Starts an HTTP server to receive all UPnP NOTIFY requests
    /// - Routes events through a unified event router to registered handlers
    ///
    /// # Unified Event Stream Processing
    ///
    /// The callback server is designed to support the unified event stream processor
    /// pattern where a single HTTP endpoint receives events from multiple UPnP
    /// services and speakers, then routes them to appropriate handlers based on
    /// subscription IDs.
    ///
    /// # Arguments
    ///
    /// * `port_range` - Range of ports to try binding to (start, end)
    /// * `event_sender` - Channel for sending notification payloads to the unified processor
    ///
    /// # Returns
    ///
    /// Returns the callback server instance or an error if no port could be bound
    /// or the local IP address could not be detected.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tokio::sync::mpsc;
    /// # use callback_server::{CallbackServer, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// let server = CallbackServer::new((3400, 3500), tx).await.unwrap();
    /// println!("Unified callback server listening at: {}", server.base_url());
    /// # }
    /// ```
    pub async fn new(
        port_range: (u16, u16),
        event_sender: mpsc::UnboundedSender<NotificationPayload>,
    ) -> Result<Self, String> {
        // Find an available port in the range
        let port = Self::find_available_port(port_range.0, port_range.1).ok_or_else(|| {
            format!(
                "No available port found in range {}-{}",
                port_range.0, port_range.1
            )
        })?;

        // Detect the local IP address speakers should call back to.
        let local_ip = Self::detect_local_ip()
            .ok_or_else(|| "Failed to detect local IP address".to_string())?;

        let base_url = format!("http://{local_ip}:{port}");

        // Create event router
        let event_router = Arc::new(EventRouter::new(event_sender));

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

        // Create ready signal channel
        let (ready_tx, mut ready_rx) = mpsc::channel::<()>(1);

        // Start the HTTP server
        let server_handle = Self::start_server(port, event_router.clone(), shutdown_rx, ready_tx);

        // Wait for server to be ready
        ready_rx
            .recv()
            .await
            .ok_or_else(|| "Server failed to start".to_string())?;

        Ok(Self {
            port,
            base_url,
            event_router,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
        })
    }

    /// Get the unified callback URL for subscription registration.
    ///
    /// This URL should be used when subscribing to UPnP events from any speaker
    /// or service. The unified callback server will route all incoming events
    /// based on their subscription IDs to the appropriate handlers.
    ///
    /// The format is `http://<local_ip>:<port>` and this same URL is used for
    /// all subscriptions, enabling the unified event stream processing pattern.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tokio::sync::mpsc;
    /// # use callback_server::{CallbackServer, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let server = CallbackServer::new((3400, 3500), tx).await.unwrap();
    /// let callback_url = server.base_url();
    /// println!("Use this URL for all subscriptions: {}", callback_url);
    /// # }
    /// ```
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the port the server is bound to.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get a reference to the event router.
    ///
    /// The router can be used to register and unregister subscription IDs
    /// for event routing.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tokio::sync::mpsc;
    /// # use callback_server::{CallbackServer, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let server = CallbackServer::new((3400, 3500), tx).await.unwrap();
    /// server.router().register("uuid:subscription-123".to_string()).await;
    /// # }
    /// ```
    pub fn router(&self) -> &Arc<EventRouter> {
        &self.event_router
    }

    /// Shutdown the callback server gracefully.
    ///
    /// Sends a shutdown signal to the HTTP server and waits for it to complete
    /// any in-flight requests.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use tokio::sync::mpsc;
    /// # use callback_server::{CallbackServer, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let server = CallbackServer::new((3400, 3500), tx).await.unwrap();
    /// server.shutdown().await.unwrap();
    /// # }
    /// ```
    pub async fn shutdown(mut self) -> Result<(), String> {
        // Send shutdown signal to HTTP server
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Wait for server task to complete
        if let Some(handle) = self.server_handle.take() {
            let _ = handle.await;
        }

        Ok(())
    }

    /// Find an available port in the given range.
    fn find_available_port(start: u16, end: u16) -> Option<u16> {
        (start..=end).find(|&port| Self::is_port_available(port))
    }

    /// Check if a port is available for binding.
    fn is_port_available(port: u16) -> bool {
        TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port)).is_ok()
    }

    /// Detect the local IP address to advertise in callback URLs.
    ///
    /// Enumerates usable IPv4 interfaces and picks one via [`preferred_local_ip`].
    ///
    /// This replaces a "connect a UDP socket to 8.8.8.8 and read back the local
    /// address" probe, which reported whichever interface won the *default route*.
    /// With a VPN up that is the tunnel, so the callback URL advertised a tunnel
    /// address no speaker could reach — every event was then silently lost and
    /// misreported as a firewall block.
    fn detect_local_ip() -> Option<Ipv4Addr> {
        preferred_local_ip(&usable_interfaces())
    }

    /// The local address a speaker at `speaker_ip` would see us on, if any.
    ///
    /// Selects the interface whose subnet actually contains `speaker_ip`, using
    /// that interface's real netmask. Exposed so callers can tell whether
    /// [`Self::base_url`] is genuinely reachable from a given speaker; see the
    /// single-`base_url` limitation in `docs/specs/callback-server.md` §14.1.
    pub fn local_ip_for_speaker(speaker_ip: Ipv4Addr) -> Option<Ipv4Addr> {
        select_interface(&usable_interfaces(), speaker_ip)
    }

    /// Start the HTTP server on the given port.
    fn start_server(
        port: u16,
        event_router: Arc<EventRouter>,
        mut shutdown_rx: mpsc::Receiver<()>,
        ready_tx: mpsc::Sender<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // One catch-all endpoint: any path, method gated to NOTIFY inside the
            // `NotifyRequest` extractor. `fallback` is what makes the path
            // arbitrary — the SID header, not the URL, is the routing key.
            let app = Router::new()
                .fallback(handle_notify)
                // Caps the bytes actually read, independently of the declared
                // Content-Length check. See MAX_NOTIFY_BODY_BYTES.
                .layer(DefaultBodyLimit::max(MAX_NOTIFY_BODY_BYTES))
                .with_state(event_router);

            let listener = match tokio::net::TcpListener::bind(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                port,
            ))
            .await
            {
                Ok(listener) => listener,
                Err(e) => {
                    // `ready_tx` drops here, so `new()` reports "Server failed to
                    // start" rather than handing back a server nothing can reach.
                    error!(port, error = %e, "CallbackServer failed to bind");
                    return;
                }
            };

            info!(
                address = ?listener.local_addr(),
                "CallbackServer listening - ready to process UPnP events"
            );
            // The socket is accepting as soon as `bind` returns, so signalling here
            // means callers can subscribe knowing events have somewhere to land.
            let _ = ready_tx.send(()).await;

            if let Err(e) = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_rx.recv().await;
                })
                .await
            {
                error!(error = %e, "CallbackServer stopped with an error");
            }
        })
    }

    /// Validate UPnP event notification headers.
    ///
    /// SID is required — without it an event cannot be routed. NT and NTS are
    /// optional (some devices omit them) but each is checked **independently** when
    /// present: the previous `if let (Some(nt), Some(nts))` form meant a request
    /// carrying only one of the two skipped validation entirely.
    fn validate_upnp_headers(sid: Option<&str>, nt: Option<&str>, nts: Option<&str>) -> bool {
        // SID header is required for event notifications
        if sid.is_none() {
            return false;
        }

        if nt.is_some_and(|nt| nt != "upnp:event") {
            return false;
        }

        if nts.is_some_and(|nts| nts != "upnp:propchange") {
            return false;
        }

        true
    }
}

/// A NOTIFY request that has passed every gate: NOTIFY-only method, declared and
/// actual body size within [`MAX_NOTIFY_BODY_BYTES`], and valid SID/NT/NTS.
///
/// Expressed as an extractor so the checks run before the handler body, and so each
/// failure carries its own status via [`IntoResponse`] rather than needing a
/// separate error-to-status mapping table.
struct NotifyRequest {
    /// Value of the SID header; the routing key.
    subscription_id: String,
    /// Raw request body, at most [`MAX_NOTIFY_BODY_BYTES`].
    body: Bytes,
}

/// Why a request was not a servable NOTIFY. Each variant carries the status a
/// malformed request has always received (404/411/413/400).
enum NotifyRejection {
    /// Not a NOTIFY request at all.
    NotNotify,
    /// No usable `Content-Length`, so the body cannot be bounded before reading it.
    LengthRequired,
    /// `Content-Length` declares more than [`MAX_NOTIFY_BODY_BYTES`].
    BodyTooLarge,
    /// The body could not be buffered — over the limit while reading, or the
    /// connection failed part-way through.
    UnreadableBody(axum::extract::rejection::BytesRejection),
    /// Missing SID, or an NT/NTS that is present with the wrong value.
    InvalidUpnpHeaders,
}

impl IntoResponse for NotifyRejection {
    fn into_response(self) -> Response {
        match self {
            // 404, as before: the speaker asked for something this server does not
            // serve. Deliberately not 405, which would advertise the NOTIFY verb.
            Self::NotNotify => (StatusCode::NOT_FOUND, "Subscription not found").into_response(),
            // Sonos always sends Content-Length; a missing one means we cannot
            // bound the body before reading it, so refuse.
            Self::LengthRequired => (
                StatusCode::LENGTH_REQUIRED,
                "Content-Length header is required",
            )
                .into_response(),
            Self::BodyTooLarge => {
                error!(
                    limit_bytes = MAX_NOTIFY_BODY_BYTES,
                    "Rejected NOTIFY body exceeding size limit"
                );
                (StatusCode::PAYLOAD_TOO_LARGE, "NOTIFY body too large").into_response()
            }
            // 413 when the read hit the limit, 400 otherwise — axum's own mapping.
            Self::UnreadableBody(rejection) => {
                error!(
                    limit_bytes = MAX_NOTIFY_BODY_BYTES,
                    status = %rejection.status(),
                    "Could not buffer NOTIFY body"
                );
                rejection.into_response()
            }
            Self::InvalidUpnpHeaders => {
                (StatusCode::BAD_REQUEST, "Invalid UPnP headers").into_response()
            }
        }
    }
}

impl<S> FromRequest<S> for NotifyRequest
where
    S: Send + Sync,
{
    type Rejection = NotifyRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Only NOTIFY is served. This gate runs *before* anything looks at the
        // body so that ordinary bodiless requests (e.g. a browser GET) still get
        // 404 rather than 411 Length Required.
        if req.method().as_str() != NOTIFY_METHOD {
            return Err(NotifyRejection::NotNotify);
        }

        // Reject on the declared length before reading a single byte.
        let content_length = declared_content_length(req.headers())?;

        // Owned copies because reading the body below consumes the request. Three
        // short strings, and only for requests that already passed the method gate.
        let path = req.uri().path().to_owned();
        let sid = upnp_header(req.headers(), "sid")?;
        let nt = upnp_header(req.headers(), "nt")?;
        let nts = upnp_header(req.headers(), "nts")?;

        let body = Bytes::from_request(req, state)
            .await
            .map_err(NotifyRejection::UnreadableBody)?;

        debug!(
            method = NOTIFY_METHOD,
            path = %path,
            content_length,
            body_size = body.len(),
            sid = ?sid,
            nt = ?nt,
            nts = ?nts,
            "Received UPnP NOTIFY event"
        );

        // Validate UPnP headers *before* decoding the body to a `String`, so junk
        // requests never pay for that second full allocation.
        if !CallbackServer::validate_upnp_headers(sid.as_deref(), nt.as_deref(), nts.as_deref()) {
            error!(
                sid = ?sid,
                nt = ?nt,
                nts = ?nts,
                "Invalid UPnP headers in NOTIFY request"
            );
            return Err(NotifyRejection::InvalidUpnpHeaders);
        }

        // Guaranteed present by the validation above.
        let subscription_id = sid.ok_or_else(|| {
            error!("Missing required SID header in UPnP NOTIFY request");
            NotifyRejection::InvalidUpnpHeaders
        })?;

        Ok(Self {
            subscription_id,
            body,
        })
    }
}

/// The declared body size, refusing requests we cannot bound up front.
///
/// A missing or unparseable `Content-Length` yields 411 rather than being treated
/// as zero: a chunked body declares no size at all, and real Sonos devices always
/// send the header (see `docs/specs/callback-server.md` §14.1).
fn declared_content_length(headers: &HeaderMap) -> Result<u64, NotifyRejection> {
    let length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(NotifyRejection::LengthRequired)?;

    if length > MAX_NOTIFY_BODY_BYTES as u64 {
        return Err(NotifyRejection::BodyTooLarge);
    }

    Ok(length)
}

/// Read a UPnP header as text. Absent is `None`; present-but-not-UTF-8 is invalid.
///
/// A non-UTF-8 value previously fell through to an unhandled rejection and became a
/// 500. None of these three headers can be valid without being text, so 400 is the
/// honest answer.
fn upnp_header(headers: &HeaderMap, name: &str) -> Result<Option<String>, NotifyRejection> {
    match headers.get(name) {
        None => Ok(None),
        Some(value) => match value.to_str() {
            Ok(value) => Ok(Some(value.to_owned())),
            Err(_) => Err(NotifyRejection::InvalidUpnpHeaders),
        },
    }
}

/// Handle a validated UPnP NOTIFY: decode, log, route.
///
/// Always answers 200 OK. The event is either delivered immediately (registered
/// SID) or buffered for replay; returning 404 could cause the speaker to cancel the
/// subscription.
async fn handle_notify(
    State(router): State<Arc<EventRouter>>,
    notify: NotifyRequest,
) -> StatusCode {
    let NotifyRequest {
        subscription_id,
        body,
    } = notify;

    // Second full copy of the body; bounded by MAX_NOTIFY_BODY_BYTES.
    let event_xml = String::from_utf8_lossy(&body).to_string();
    if event_xml.len() > TRACE_PREVIEW_BYTES {
        trace!(
            event_xml_preview = %preview(&event_xml, TRACE_PREVIEW_BYTES),
            total_length = event_xml.len(),
            "UPnP event XML content (truncated)"
        );
    } else {
        trace!(event_xml = %event_xml, "UPnP event XML content (full)");
    }

    router.route_event(subscription_id.clone(), event_xml).await;

    debug!(subscription_id = %subscription_id, "UPnP event accepted");
    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_port_available() {
        // Port 0 should always be available (OS assigns a free port)
        assert!(CallbackServer::is_port_available(0));

        // Bind to a port and verify it's no longer available
        let _listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = _listener.local_addr().unwrap().port();
        // While the listener is held, the port should not be available
        assert!(!CallbackServer::is_port_available(port));
        // Keep listener alive for the assertion
        drop(_listener);
    }

    /// A port range the OS just told us is free.
    ///
    /// Tests used to hardcode 50000-50100. Two concurrent `cargo test --workspace`
    /// runs then raced for the same ports and one failed with "Address already in
    /// use" — separate `CARGO_TARGET_DIR`s do not help, because the contended
    /// resource is the host's port space. Asking the OS for an ephemeral port and
    /// releasing it narrows the race to the microseconds between drop and rebind.
    fn free_port_range() -> (u16, u16) {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        (port, port)
    }

    #[test]
    fn test_find_available_port() {
        // A range whose only port is held is exhausted; the same range is usable
        // once the holder releases it.
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(CallbackServer::find_available_port(port, port), None);
        drop(listener);
        assert_eq!(CallbackServer::find_available_port(port, port), Some(port));
    }

    #[test]
    fn test_detect_local_ip() {
        let ip = CallbackServer::detect_local_ip();
        assert!(ip.is_some());

        // Should not be localhost
        if let Some(addr) = ip {
            assert_ne!(addr, Ipv4Addr::new(127, 0, 0, 1));
        }
    }

    /// The real network this SDK is developed against is a single **/22**
    /// (`192.168.4.0/22`, netmask `255.255.252.0`), not two /24s: speakers sit on
    /// both `192.168.4.x` and `192.168.5.x` and are all directly reachable from
    /// the one interface at `192.168.4.32`.
    ///
    /// An implementation that assumed /24 would decide `192.168.5.19` is off-net
    /// and either refuse to subscribe or advertise a wrong callback address. This
    /// test fails under such an implementation, which is the point of it.
    #[test]
    fn test_selects_interface_across_22_subnet() {
        let lan = (
            Ipv4Addr::new(192, 168, 4, 32),
            Ipv4Addr::new(255, 255, 252, 0),
        );

        // A speaker in the *upper* half of the /22, outside a naive /24 read.
        assert_eq!(
            select_interface(&[lan], Ipv4Addr::new(192, 168, 5, 19)),
            Some(Ipv4Addr::new(192, 168, 4, 32))
        );
        // And one in the same /24, which both readings get right.
        assert_eq!(
            select_interface(&[lan], Ipv4Addr::new(192, 168, 4, 20)),
            Some(Ipv4Addr::new(192, 168, 4, 32))
        );
        // Just past the /22 boundary is genuinely off-net.
        assert_eq!(
            select_interface(&[lan], Ipv4Addr::new(192, 168, 8, 1)),
            None
        );
    }

    #[test]
    fn test_selects_interface_on_target_subnet() {
        let interfaces = [
            (Ipv4Addr::new(10, 0, 0, 5), Ipv4Addr::new(255, 255, 255, 0)),
            (
                Ipv4Addr::new(192, 168, 4, 32),
                Ipv4Addr::new(255, 255, 252, 0),
            ),
            (
                Ipv4Addr::new(172, 16, 3, 9),
                Ipv4Addr::new(255, 255, 255, 0),
            ),
        ];

        // Each target picks the interface sharing its subnet, not merely the first.
        assert_eq!(
            select_interface(&interfaces, Ipv4Addr::new(192, 168, 5, 19)),
            Some(Ipv4Addr::new(192, 168, 4, 32))
        );
        assert_eq!(
            select_interface(&interfaces, Ipv4Addr::new(10, 0, 0, 200)),
            Some(Ipv4Addr::new(10, 0, 0, 5))
        );
        // Overlapping subnets resolve to the more specific prefix.
        let overlapping = [
            (
                Ipv4Addr::new(192, 168, 4, 32),
                Ipv4Addr::new(255, 255, 0, 0),
            ),
            (
                Ipv4Addr::new(192, 168, 5, 7),
                Ipv4Addr::new(255, 255, 255, 0),
            ),
        ];
        assert_eq!(
            select_interface(&overlapping, Ipv4Addr::new(192, 168, 5, 19)),
            Some(Ipv4Addr::new(192, 168, 5, 7))
        );
    }

    /// A live Tailscale/VPN tunnel is what made the old route-to-8.8.8.8 probe
    /// dangerous: the tunnel wins the default route, so the callback URL advertised
    /// a CGNAT address unreachable from the LAN and every event vanished.
    #[test]
    fn test_tunnel_interface_not_chosen_for_lan_target() {
        let interfaces = [
            // Tailscale CGNAT /32 — sorts first numerically, so ordering alone
            // would pick it.
            (
                Ipv4Addr::new(100, 78, 222, 31),
                Ipv4Addr::new(255, 255, 255, 255),
            ),
            (
                Ipv4Addr::new(192, 168, 4, 32),
                Ipv4Addr::new(255, 255, 252, 0),
            ),
        ];

        // Subnet containment rules the tunnel out for a LAN speaker.
        assert_eq!(
            select_interface(&interfaces, Ipv4Addr::new(192, 168, 5, 19)),
            Some(Ipv4Addr::new(192, 168, 4, 32))
        );
        // And the target-less fallback deprioritises CGNAT too, since base_url is
        // built before any speaker is known.
        assert_eq!(
            preferred_local_ip(&interfaces),
            Some(Ipv4Addr::new(192, 168, 4, 32))
        );
    }

    #[test]
    fn test_unusable_addresses_are_filtered() {
        assert!(is_usable_ipv4(&Ipv4Addr::new(192, 168, 4, 32)));
        assert!(!is_usable_ipv4(&Ipv4Addr::LOCALHOST));
        assert!(!is_usable_ipv4(&Ipv4Addr::UNSPECIFIED));
        // Link-local means DHCP failed; it cannot reach a speaker.
        assert!(!is_usable_ipv4(&Ipv4Addr::new(169, 254, 1, 1)));
    }

    #[test]
    fn test_validate_upnp_headers() {
        // Valid headers with NT and NTS
        assert!(CallbackServer::validate_upnp_headers(
            Some("uuid:123"),
            Some("upnp:event"),
            Some("upnp:propchange"),
        ));

        // Valid headers without NT and NTS (event notification)
        assert!(CallbackServer::validate_upnp_headers(
            Some("uuid:123"),
            None,
            None
        ));

        // Invalid: missing SID
        assert!(!CallbackServer::validate_upnp_headers(
            None,
            Some("upnp:event"),
            Some("upnp:propchange"),
        ));

        // Invalid: wrong NT value
        assert!(!CallbackServer::validate_upnp_headers(
            Some("uuid:123"),
            Some("wrong"),
            Some("upnp:propchange"),
        ));

        // Invalid: wrong NTS value
        assert!(!CallbackServer::validate_upnp_headers(
            Some("uuid:123"),
            Some("upnp:event"),
            Some("wrong"),
        ));
    }

    /// NT and NTS used to be checked inside `if let (Some(nt), Some(nts))`, so a
    /// request carrying only one of the two bypassed validation entirely: a bogus
    /// `NT` with no `NTS` was accepted. Each header is now checked independently.
    #[test]
    fn test_validate_upnp_headers_checks_nt_and_nts_independently() {
        // A single valid header on its own is still fine.
        assert!(CallbackServer::validate_upnp_headers(
            Some("uuid:123"),
            Some("upnp:event"),
            None
        ));
        assert!(CallbackServer::validate_upnp_headers(
            Some("uuid:123"),
            None,
            Some("upnp:propchange"),
        ));

        // A single *invalid* header on its own is now rejected; both of these
        // returned true before the fix.
        assert!(!CallbackServer::validate_upnp_headers(
            Some("uuid:123"),
            Some("upnp:not-an-event"),
            None,
        ));
        assert!(!CallbackServer::validate_upnp_headers(
            Some("uuid:123"),
            None,
            Some("upnp:not-a-propchange"),
        ));
    }

    /// Slicing event XML at a fixed byte offset panics when that offset lands
    /// inside a multi-byte codepoint — very reachable for non-Latin track titles.
    #[test]
    fn test_event_xml_preview_handles_multibyte_boundary() {
        // 198 ASCII bytes + a 3-byte codepoint occupying bytes 198..201, so byte
        // index 200 is *not* a char boundary.
        let xml = format!("{}日本語", "a".repeat(198));
        assert!(!xml.is_char_boundary(TRACE_PREVIEW_BYTES));

        let out = preview(&xml, TRACE_PREVIEW_BYTES);

        // Snapped back to the boundary at 198, still valid UTF-8, still a prefix.
        assert_eq!(out.len(), 198);
        assert!(xml.starts_with(out));
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }

    #[test]
    fn test_event_xml_preview_short_and_exact_inputs() {
        // Shorter than the limit is returned whole.
        assert_eq!(preview("<event/>", TRACE_PREVIEW_BYTES), "<event/>");
        // Exactly at the limit is returned whole.
        let exact = "a".repeat(TRACE_PREVIEW_BYTES);
        assert_eq!(
            preview(&exact, TRACE_PREVIEW_BYTES).len(),
            TRACE_PREVIEW_BYTES
        );
        // A single wide codepoint over the limit truncates to empty, not a panic.
        assert_eq!(preview("日", 1), "");
    }

    #[tokio::test]
    async fn test_callback_server_creation() {
        let (tx, _rx) = mpsc::unbounded_channel();

        let range = free_port_range();
        let server = CallbackServer::new(range, tx).await;
        assert!(server.is_ok());

        let server = server.unwrap();
        assert_eq!(server.port(), range.0);
        assert!(server.base_url().contains(&server.port().to_string()));
        // The advertised host is a real interface address, not a route probe result.
        assert!(server.base_url().starts_with("http://"));

        // Cleanup
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_callback_server_register_unregister() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let server = CallbackServer::new(free_port_range(), tx).await.unwrap();

        let sub_id = "test-sub-123".to_string();

        // Register subscription via router
        server.router().register(sub_id.clone()).await;

        // Unregister subscription via router
        server.router().unregister(&sub_id).await;

        // Plugin system has been removed

        // Cleanup
        server.shutdown().await.unwrap();
    }
}
