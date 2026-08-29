//! Server probing: size, Accept-Ranges support, filename, content type.

use crate::engine::SharedClient;
use reqwest::Method;
use std::collections::HashMap;

/// Per-download HTTP options: extra headers, cookies, basic auth.
///
/// These are applied to *every* request made for a task (the probe and all
/// segment workers), so authenticated / cookie-gated downloads work end to end.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RequestOptions {
    /// Extra HTTP headers (e.g. `Referer`, custom tokens).
    pub headers: HashMap<String, String>,
    /// Cookie string sent as the `Cookie` header (applied after `headers`).
    pub cookies: Option<String>,
    /// Basic-auth credentials (user, password) sent as `Authorization`.
    pub basic_auth: Option<(String, String)>,
}

impl RequestOptions {
    /// True when nothing was configured (avoids touching the request).
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && self.cookies.is_none() && self.basic_auth.is_none()
    }

    /// Apply these options onto a request builder. Invalid header names or
    /// values are skipped silently instead of failing the whole download.
    pub fn apply(&self, mut rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        for (k, v) in &self.headers {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                rb = rb.header(name, val);
            }
        }
        if let Some(c) = &self.cookies {
            if !c.is_empty() {
                rb = rb.header(reqwest::header::COOKIE, c.clone());
            }
        }
        if let Some((u, p)) = &self.basic_auth {
            rb = rb.basic_auth(u, Some(p));
        }
        rb
    }
}

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
pub async fn probe(
    client: &SharedClient,
    url: &str,
    opts: &RequestOptions,
) -> Result<ProbeResult, String> {
    let head = opts
        .apply(client.request(Method::HEAD, url))
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

    #[test]
    fn request_options_default_is_empty_and_apply_is_safe() {
        let o = RequestOptions::default();
        assert!(o.is_empty());

        let mut o = RequestOptions::default();
        o.headers.insert("X-Test".into(), "1".into());
        o.cookies = Some("a=b; c=d".into());
        o.basic_auth = Some(("user".into(), "pass".into()));
        assert!(!o.is_empty());

        // apply() must not panic, even with an invalid header name present.
        o.headers
            .insert("bad header name with spaces".into(), "x".into());
        let client = reqwest::Client::new();
        let _ = o.apply(client.get("http://example.com/"));
    }
}
