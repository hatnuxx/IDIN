// IDIN native-messaging host — reads Chrome/Edge/Firefox messages on stdin
// and forwards the URL to the running IDIN instance over a local TCP port.
// Standalone binary (no Tauri) so browsers can spawn it cheaply.

use std::io::{Read, Write};
use std::process::exit;

const FORWARD_PORT: u16 = 45187;

fn main() {
    // Native messaging: 4-byte little-endian length prefix + JSON.
    let mut len_buf = [0u8; 4];
    if std::io::stdin().read_exact(&mut len_buf).is_err() {
        exit(0);
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 1024 * 1024 {
        exit(1);
    }
    let mut buf = vec![0u8; len];
    if std::io::stdin().read_exact(&mut buf).is_err() {
        exit(1);
    }

    let msg: serde_json::Value = match serde_json::from_slice(&buf) {
        Ok(v) => v,
        Err(_) => exit(1),
    };

    let url = msg.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        exit(1);
    }

    // Attach the shared API token so the app can tell our requests apart
    // from any other local process poking at the port.
    let token = load_api_token().unwrap_or_default();
    if token.is_empty() {
        eprintln!("idin-host: api_token not found — is IDIN installed and has it run once?");
    }

    // Forward to IDIN's local API.
    let response = send_to_idin(url, &token);
    // Reply to the browser so it knows the outcome.
    let reply = serde_json::json!({ "ok": response.is_ok(), "url": url });
    let bytes = serde_json::to_vec(&reply).unwrap_or_default();
    let mut out = std::io::stdout();
    let _ = out.write_all(&(bytes.len() as u32).to_le_bytes());
    let _ = out.write_all(&bytes);
    let _ = out.flush();
}

fn send_to_idin(url: &str, token: &str) -> std::io::Result<()> {
    use std::net::TcpStream;
    let mut stream = TcpStream::connect(("127.0.0.1", FORWARD_PORT))?;
    let payload = serde_json::json!({ "type": "add", "url": url, "token": token });
    stream.write_all(payload.to_string().as_bytes())
}

/// Read the shared secret written by the app (%LOCALAPPDATA%\IDIN\api_token).
fn load_api_token() -> Option<String> {
    let path = if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        std::path::PathBuf::from(base).join("IDIN").join("api_token")
    } else {
        let home = std::env::var_os("HOME")?;
        std::path::PathBuf::from(home).join(".idin").join("api_token")
    };
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() >= 32)
}
