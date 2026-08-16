//! Declarative macros for UPnP operation and service definitions
//!
//! This module provides macros that dramatically reduce boilerplate when defining
//! UPnP operations. Instead of manually implementing traits and structs, developers
//! can use simple declarative syntax to generate all necessary code.

/// Simplified macro for defining UPnP operations with minimal boilerplate
///
/// This macro generates all the necessary structs and trait implementations
/// for a UPnP operation.
///
/// # Example
/// ```rust,ignore
/// define_upnp_operation! {
///     operation: PlayOperation,
///     action: "Play",
///     service: AVTransport,
///     request: {
///         speed: String,
///     },
///     response: (),
///     payload: |req| format!("<InstanceID>{}</InstanceID><Speed>{}</Speed>", req.instance_id, req.speed),
///     parse: |_xml| Ok(()),
/// }
/// ```
#[macro_export]
macro_rules! define_upnp_operation {
    (
        operation: $op_struct:ident,
        action: $action:literal,
        service: $service:ident,
        request: {
            $($field:ident: $field_type:ty),* $(,)?
        },
        response: $response_type:ty,
        payload: |$req_param:ident| $payload_expr:expr,
        parse: |$xml_param:ident| $parse_expr:expr $(,)?
    ) => {
        paste! {
            #[derive(serde::Serialize, Clone, Debug, PartialEq)]
            pub struct [<$op_struct Request>] {
                $(pub $field: $field_type,)*
                pub instance_id: u32,
            }

            // Note: Validate implementation can be provided manually if needed
            // Default empty implementation is not generated to avoid conflicts

            #[derive(serde::Deserialize, Debug, Clone, PartialEq)]
            pub struct [<$op_struct Response>];

            pub struct $op_struct;

            impl $crate::operation::UPnPOperation for $op_struct {
                type Request = [<$op_struct Request>];
                type Response = $response_type;

                const SERVICE: $crate::service::Service = $crate::service::Service::$service;
                const ACTION: &'static str = $action;

                fn build_payload(request: &Self::Request) -> Result<String, $crate::operation::ValidationError> {
                    request.validate($crate::operation::ValidationLevel::Basic)?;
                    let $req_param = request;
                    Ok($payload_expr)
                }

                fn parse_response(xml: &xmltree::Element) -> Result<Self::Response, $crate::error::ApiError> {
                    let $xml_param = xml;
                    $parse_expr
                }
            }

            // Generate convenience function
            pub fn [<$op_struct:snake>]($($field: $field_type),*) -> $crate::operation::OperationBuilder<$op_struct> {
                let request = [<$op_struct Request>] {
                    $($field,)*
                    instance_id: 0,
                };
                $crate::operation::OperationBuilder::new(request)
            }
        }
    };
}

/// Macro for defining operations with XML response parsing
///
/// # Example
/// ```rust,ignore
/// define_operation_with_response! {
///     operation: GetVolumeOperation,
///     action: "GetVolume",
///     service: RenderingControl,
///     request: {
///         channel: String,
///     },
///     response: GetVolumeResponse {
///         current_volume: u8,
///     },
///     xml_mapping: {
///         current_volume: "CurrentVolume",
///     },
/// }
/// ```
///
/// # Request element names
///
/// UPnP argument names come from each device's SCPD and use casing that cannot be
/// derived mechanically from snake_case (`ObjectID`, `EnqueuedURI`, `NumberOfTracks`).
/// For single-word request fields the macro derives the element name by capitalizing
/// the first character (`channel` -> `Channel`). Multi-word request fields **must**
/// declare their element name explicitly via the optional `request_xml_mapping:` block,
/// which is otherwise a compile error:
///
/// ```rust,ignore
/// define_operation_with_response! {
///     operation: SaveQueueOperation,
///     action: "SaveQueue",
///     service: AVTransport,
///     request: {
///         title: String,
///         object_id: String,
///     },
///     response: SaveQueueResponse {
///         assigned_object_id: String,
///     },
///     request_xml_mapping: {
///         title: "Title",
///         object_id: "ObjectID",
///     },
///     xml_mapping: {
///         assigned_object_id: "AssignedObjectID",
///     },
/// }
/// ```
///
/// When present, `request_xml_mapping:` must list every request field (enforced by an
/// exhaustive destructuring of the generated request struct) and its order determines
/// the order arguments are written to the SOAP body.
#[macro_export]
macro_rules! define_operation_with_response {
    // Variant with explicit request element names.
    (
        operation: $op_struct:ident,
        action: $action:literal,
        service: $service:ident,
        request: {
            $($field:ident: $field_type:ty),* $(,)?
        },
        response: $response_struct:ident {
            $($resp_field:ident: $resp_type:ty),* $(,)?
        },
        request_xml_mapping: {
            $($req_field:ident: $req_xml_name:literal),* $(,)?
        },
        xml_mapping: {
            $($xml_field:ident: $xml_path:literal),* $(,)?
        } $(,)?
    ) => {
        paste! {
            #[derive(serde::Serialize, Clone, Debug, PartialEq)]
            pub struct [<$op_struct Request>] {
                $(pub $field: $field_type,)*
                pub instance_id: u32,
            }

            #[derive(serde::Deserialize, Debug, Clone, PartialEq)]
            pub struct $response_struct {
                $(pub $resp_field: $resp_type,)*
            }

            pub struct $op_struct;

            impl $crate::operation::UPnPOperation for $op_struct {
                type Request = [<$op_struct Request>];
                type Response = $response_struct;

                const SERVICE: $crate::service::Service = $crate::service::Service::$service;
                const ACTION: &'static str = $action;

                fn build_payload(request: &Self::Request) -> Result<String, $crate::operation::ValidationError> {
                    request.validate($crate::operation::ValidationLevel::Basic)?;

                    // Exhaustive destructuring: omitting a request field from
                    // `request_xml_mapping` fails to compile.
                    let [<$op_struct Request>] { $($req_field,)* instance_id } = request;

                    #[allow(unused_mut)]
                    let mut xml = format!("<InstanceID>{}</InstanceID>", instance_id);
                    $(
                        let escaped = $crate::operation::xml_escape(&format!("{}", $req_field));
                        xml.push_str(&format!("<{0}>{1}</{0}>", $req_xml_name, escaped));
                    )*
                    Ok(xml)
                }

                fn parse_response(xml: &xmltree::Element) -> Result<Self::Response, $crate::error::ApiError> {
                    $(let $xml_field = xml
                        .get_child($xml_path)
                        .and_then(|e| e.get_text())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_default();)*

                    Ok($response_struct {
                        $($resp_field: $xml_field,)*
                    })
                }
            }

            // Generate convenience function
            pub fn [<$op_struct:snake>]($($field: $field_type),*) -> $crate::operation::OperationBuilder<$op_struct> {
                let request = [<$op_struct Request>] {
                    $($field,)*
                    instance_id: 0,
                };
                $crate::operation::OperationBuilder::new(request)
            }
        }
    };

    // Variant without explicit request element names: only single-word request
    // fields are allowed, since their element name is just the capitalized field.
    (
        operation: $op_struct:ident,
        action: $action:literal,
        service: $service:ident,
        request: {
            $($field:ident: $field_type:ty),* $(,)?
        },
        response: $response_struct:ident {
            $($resp_field:ident: $resp_type:ty),* $(,)?
        },
        xml_mapping: {
            $($xml_field:ident: $xml_path:literal),* $(,)?
        } $(,)?
    ) => {
        paste! {
            #[derive(serde::Serialize, Clone, Debug, PartialEq)]
            pub struct [<$op_struct Request>] {
                $(pub $field: $field_type,)*
                pub instance_id: u32,
            }

            // Note: Validate implementation can be provided manually if needed
            // Default empty implementation is not generated to avoid conflicts

            #[derive(serde::Deserialize, Debug, Clone, PartialEq)]
            pub struct $response_struct {
                $(pub $resp_field: $resp_type,)*
            }

            pub struct $op_struct;

            impl $crate::operation::UPnPOperation for $op_struct {
                type Request = [<$op_struct Request>];
                type Response = $response_struct;

                const SERVICE: $crate::service::Service = $crate::service::Service::$service;
                const ACTION: &'static str = $action;

                fn build_payload(request: &Self::Request) -> Result<String, $crate::operation::ValidationError> {
                    request.validate($crate::operation::ValidationLevel::Basic)?;

                    #[allow(unused_mut)]
                    let mut xml = format!("<InstanceID>{}</InstanceID>", request.instance_id);
                    $(
                        // Only single-word fields can have their UPnP element name derived
                        // by capitalizing the first character. Multi-word fields need
                        // `request_xml_mapping:` because UPnP casing (ObjectID, EnqueuedURI)
                        // is not recoverable from snake_case.
                        const _: () = $crate::operation::assert_derivable_arg_name(stringify!($field));
                        let capitalized = $crate::operation::capitalize_first(stringify!($field));
                        let escaped = $crate::operation::xml_escape(&format!("{}", request.$field));
                        xml.push_str(&format!("<{0}>{1}</{0}>", capitalized, escaped));
                    )*
                    Ok(xml)
                }

                fn parse_response(xml: &xmltree::Element) -> Result<Self::Response, $crate::error::ApiError> {
                    // Create a temporary mapping from field names to XML paths
                    $(let $xml_field = xml
                        .get_child($xml_path)
                        .and_then(|e| e.get_text())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_default();)*

                    Ok($response_struct {
                        $($resp_field: $xml_field,)*
                    })
                }
            }

            // Generate convenience function
            pub fn [<$op_struct:snake>]($($field: $field_type),*) -> $crate::operation::OperationBuilder<$op_struct> {
                let request = [<$op_struct Request>] {
                    $($field,)*
                    instance_id: 0,
                };
                $crate::operation::OperationBuilder::new(request)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_macro_compilation() {
        // Test that our macros compile without errors
        // This is mainly a compilation test to ensure the macro syntax is correct

        // Note: Actual usage tests would go in the services modules where the macros are used
        // since we can't easily test macro expansion here without a more complex test setup
    }
}
