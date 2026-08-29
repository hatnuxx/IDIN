//! Local TCP API (port 45187) — receives URLs from the browser extension
//! native host and hands them to the engine.
//!
//! Security model:
//! - Binds **127.0.0.1 only**.
//! - Requires a **shared-secret token**: the app generates one at startup and
//!   stores it in `%LOCALAPPDATA%\IDIN\api_token` (per-user, next to the
//!   native host). The host attaches it to every request; any *other* local
//!   process that cannot read the user's file is rejected.
//! - Only `http://` / `https://` URLs are accepted.

use crate::engine::Engine;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

pub const LOCAL_API_PORT: u16 = 45187;
/// Matches the native host's own message-size limit.
const MAX_MSG_LEN: usize = 1024 * 1024;

/// Where the shared API token lives (next to the native-messaging host).
fn api_token_path() -> Option<PathBuf> {
    if let Some(base) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        return Some(base.join("IDIN").join("api_token"));
    }
    // Non-Windows fallback.
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".idin").join("api_token"))
}

/// Load or create the shared API token.
/// Returns `None` when it cannot be persisted — the local API then refuses
/// every request rather than run unauthenticated.
pub fn ensure_api_token() -> Option<String> {
    let path = api_token_path()?;
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let t = existing.trim().to_string();
        if t.len() >= 32 {
            return Some(t);
        }
    }
    // 32 random bytes → 64 hex chars.
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).ok()?;
    let token: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    std::fs::write(&path, &token).ok()?;
    Some(token)
}

/// Accept only well-formed http(s) URLs: no whitespace/control characters,
/// sane length. Anything else is rejected before it can reach the engine.
fn is_valid_url(url: &str) -> bool {
    (url.starts_with("http://") || url.starts_with("https://"))
        && url.len() >= 11
        && url.len() <= 2048
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub async fn serve(engine: Arc<Engine>) {
    let Some(expected_token) = ensure_api_token() else {
        log::error!("local API disabled: cannot create API token");
        return;
    };
    let listener = match TcpListener::bind(("127.0.0.1", LOCAL_API_PORT)).await {
        Ok(l) => l,
        Err(e) => {
            log::warn!("local API unavailable on {LOCAL_API_PORT}: {e}");
            return;
        }
    };
    log::info!("local API listening on 127.0.0.1:{LOCAL_API_PORT}");

    loop {
        let Ok((mut sock, _)) = listener.accept().await else {
            break;
        };
        let engine = engine.clone();
        let expected_token = expected_token.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_MSG_LEN + 1];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            if n > MAX_MSG_LEN {
                log::warn!("local API: oversized message rejected");
                return;
            }
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf[..n]) else {
                log::warn!("local API: malformed JSON rejected");
                return;
            };
            if v.get("type").and_then(|t| t.as_str()) != Some("add") {
                return;
            }
            // Shared-secret check: only our native host knows the token.
            if v.get("token").and_then(|t| t.as_str()) != Some(expected_token.as_str()) {
                log::warn!("local API: request with invalid token rejected");
                return;
            }
            let Some(url) = v.get("url").and_then(|u| u.as_str()) else {
                return;
            };
            if !is_valid_url(url) {
                log::warn!("local API: invalid URL rejected");
                return;
            }
            let url = url.to_string();
            let eng = engine.clone();
            tokio::spawn(async move {
                // Extension doesn't have access to Tauri config state,
                // so pass None — downloads go to the default directory.
                if let Err(e) = eng.add(url, default_dir(), 8, None).await {
                    log::warn!("extension download failed: {e}");
                }
            });
        });
    }
}

fn default_dir() -> std::path::PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|h| std::path::PathBuf::from(h).join("Downloads"))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_validation_accepts_http_s() {
        assert!(is_valid_url("http://example.com/file.zip"));
        assert!(is_valid_url("https://example.com/a/b?q=1"));
    }

    #[test]
    fn url_validation_rejects_other_schemes_and_junk() {
        assert!(!is_valid_url("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_valid_url("ftp://example.com/f"));
        assert!(!is_valid_url("https://"));
        assert!(!is_valid_url(""));
        assert!(!is_valid_url("https://example.com/a b"));
        assert!(!is_valid_url("https://example.com/\n{bad}"));
    }
}

