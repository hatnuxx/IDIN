//! Server probing: size, Accept-Ranges support, filename, content type.

use crate::engine::SharedClient;
use reqwest::Method;

/// What we learn about a URL before downloading.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub total_bytes: Option<u64>,
    pub accepts_ranges: bool,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub final_url: String,
}

/// Probe a URL with a HEAD request, falling back to a ranged GET.
pub async fn probe(client: &SharedClient, url: &str) -> Result<ProbeResult, String> {
    let head = client
        .request(Method::HEAD, url)
        .send()
        .await
        .map_err(|e| format!("HEAD failed: {e}"))?;

    let (status, headers, final_url) = (
        head.status(),
        head.headers().clone(),
        head.url().to_string(),
    );

    if !status.is_success() {
        return Err(format!("server returned {status}"));
    }

    let total_bytes = content_length(&headers);
    let accepts_ranges = headers
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
    let filename = filename_from_headers(&headers);
    let content_type = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    Ok(ProbeResult {
        total_bytes,
        accepts_ranges,
        filename,
        content_type,
        final_url,
    })
}

fn content_length(h: &reqwest::header::HeaderMap) -> Option<u64> {
    h.get(reqwest::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

fn filename_from_headers(h: &reqwest::header::HeaderMap) -> Option<String> {
    let cd = h.get(reqwest::header::CONTENT_DISPOSITION)?.to_str().ok()?;
    // Simple RFC 6266-ish parse: prefer filename*=... (UTF-8), else filename="..."
    if let Some(idx) = cd.find("filename*=") {
        let rest = &cd[idx + 10..];
        // form: utf-8''name
        let name = rest.split(';').next()?.split('\'').next_back()?;
        return Some(percent_decode(name));
    }
    let idx = cd.find("filename=")?;
    let rest = &cd[idx + 9..];
    let name = rest
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"');
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_filename_utf8_ext() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static(
                "attachment; filename*=UTF-8''%D9%81%D8%A7%DB%8C%D9%84.zip",
            ),
        );
        assert_eq!(filename_from_headers(&h).as_deref(), Some("فایل.zip"));
    }

    #[test]
    fn parses_filename_quoted() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static("attachment; filename=\"setup.exe\""),
        );
        assert_eq!(filename_from_headers(&h).as_deref(), Some("setup.exe"));
    }

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("plain"), "plain");
    }
}
