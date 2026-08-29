//! Clipboard monitoring: watches the system clipboard for URLs and surfaces
//! them to the UI so users can send a copied link to IDIN with one click.
//!
//! This is a lightweight poller (not a global hotkey) so it runs only while
//! the app is alive — no background code persists after exit.

use tauri::{AppHandle, Emitter};

/// Poll the clipboard every N ms for a new http(s) URL.
/// When one is found, emit a `clipboard-url` event whose payload is the URL.
pub fn start_clipboard_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: Option<String> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let current = current_clipboard_text();
            let Some(text) = current else { continue };
            // Don't re-fire the same URL over and over.
            if last.as_deref() == Some(&text) {
                continue;
            }
            last = Some(text.clone());

            if let Some(url) = extract_first_url(&text) {
                let _ = app.emit("clipboard-url", url);
            }
        }
    });
}

/// Read the current clipboard text, robustly (may be empty or locked).
fn current_clipboard_text() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    cb.get_text().ok()
}

/// Find the first http(s):// URL in a piece of text.
fn extract_first_url(text: &str) -> Option<String> {
    let mut idx = 0;
    while idx < text.len() {
        let Some(rel) = text[idx..].find("http") else {
            break;
        };
        idx += rel;
        if !text[idx..].starts_with("http://") && !text[idx..].starts_with("https://") {
            idx += 4; // skip past this "http" match
            continue;
        }
        let rest = &text[idx..];
        let url = rest
            .split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .next()
            .unwrap_or(rest);
        // Trim common trailing punctuation.
        let url = url.trim_end_matches(|c: char| "),.;!?".contains(c));
        if !url.is_empty() {
            return Some(url.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_url() {
        assert_eq!(
            extract_first_url("hello https://example.com/a.zip"),
            Some("https://example.com/a.zip".into())
        );
    }

    #[test]
    fn extracts_url_with_punct() {
        assert_eq!(
            extract_first_url("see https://x.io/file.mp4, now"),
            Some("https://x.io/file.mp4".into())
        );
    }

    #[test]
    fn no_url_returns_none() {
        assert_eq!(extract_first_url("just text"), None);
    }

    #[test]
    fn ignores_plain_http_word() {
        assert_eq!(extract_first_url("http is a protocol"), None);
    }

    #[test]
    fn takes_first_of_many() {
        assert_eq!(
            extract_first_url("https://a.com/1 https://b.com/2"),
            Some("https://a.com/1".into())
        );
    }
}
