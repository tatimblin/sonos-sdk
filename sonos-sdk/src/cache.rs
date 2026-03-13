//! Disk cache for discovered devices.
//!
//! Stores discovered devices in `~/.cache/sonos/cache.json` (or platform equivalent)
//! with a 24-hour TTL. Cache is disposable — the system recovers via SSDP on miss.

use serde::{Deserialize, Serialize};
use sonos_discovery::Device;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

const CACHE_TTL_SECS: u64 = 24 * 3600;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CachedDevices {
    pub devices: Vec<Device>,
    pub cached_at: u64, // seconds since UNIX_EPOCH
}

pub(crate) fn cache_dir() -> Option<PathBuf> {
    std::env::var("SONOS_CACHE_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::cache_dir().map(|p| p.join("sonos")))
}

pub(crate) fn load() -> Option<CachedDevices> {
    let path = cache_dir()?.join("cache.json");
    let contents = fs::read_to_string(&path).ok()?;
    let cached: CachedDevices = serde_json::from_str(&contents).ok()?;
    if cached.devices.len() > 256 {
        return None;
    }
    Some(cached)
}

pub(crate) fn save(devices: &[Device]) -> Result<(), io::Error> {
    let dir = cache_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cache dir not found"))?;
    fs::create_dir_all(&dir)?;

    let cached = CachedDevices {
        devices: devices.to_vec(),
        cached_at: now_secs(),
    };
    let json = serde_json::to_string(&cached)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let temp_path = dir.join("cache.json.tmp");
    fs::write(&temp_path, &json)?;
    fs::rename(&temp_path, dir.join("cache.json")).inspect_err(|_| {
        let _ = fs::remove_file(&temp_path);
    })?;
    Ok(())
}

pub(crate) fn is_stale(cached: &CachedDevices) -> bool {
    let now = now_secs();
    // Reject future timestamps — treat as stale (forces rediscovery)
    if cached.cached_at > now {
        return true;
    }
    now - cached.cached_at >= CACHE_TTL_SECS
}
