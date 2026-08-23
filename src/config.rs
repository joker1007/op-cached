use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};

pub const DEFAULT_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const ENV_TTL: &str = "OP_CACHED_TTL";
pub const ENV_SOCKET: &str = "OP_CACHED_SOCKET";

/// Resolve socket path: CLI arg > env var > XDG_RUNTIME_DIR > /tmp.
pub fn resolve_socket(arg: Option<PathBuf>) -> PathBuf {
    if let Some(p) = arg {
        return p;
    }
    if let Some(p) = std::env::var_os(ENV_SOCKET).filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir).join("op-cached.sock");
    }
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/op-cached-{uid}.sock"))
}

/// Resolve TTL: CLI arg > env var > default (7d).
pub fn resolve_ttl(arg: Option<Duration>) -> Result<Duration> {
    if let Some(d) = arg {
        return Ok(d);
    }
    match std::env::var(ENV_TTL) {
        Ok(s) if !s.trim().is_empty() => {
            humantime::parse_duration(s.trim()).with_context(|| format!("invalid {ENV_TTL}: {s:?}"))
        }
        _ => Ok(DEFAULT_TTL),
    }
}

pub fn parse_duration(s: &str) -> std::result::Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}
