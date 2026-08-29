//! HTTP Range segment worker: downloads one byte range to a writer.

use crate::engine::probe::RequestOptions;
use crate::engine::SharedClient;
use std::io::{Seek, SeekFrom, Write};

/// Download the byte range `start..=end` (inclusive) into `file`,
/// which is seeked to `start` first. Returns bytes written.
///
/// `live_end` supports dynamic segment re-allocation (3.9): the caller can
/// shrink the effective end at any time (another worker stole our tail);
/// the download then stops at the new end instead of the original one.
pub async fn download_segment<W>(
    client: &SharedClient,
    url: &str,
    start: u64,
    end: u64,
    file: &mut W,
    on_chunk: &mut (dyn FnMut(u64) + Send),
    opts: &RequestOptions,
    live_end: Option<&(dyn Fn() -> u64 + Send + Sync)>,
) -> Result<u64, String>
where
    W: Write + Seek + Send,
{
    let range = format!("bytes={start}-{end}");
    let mut resp = opts
        .apply(client.get(url))
        .header(reqwest::header::RANGE, range)
        .send()
        .await
        .map_err(|e| format!("segment request failed: {e}"))?;

    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!(
            "expected 206 Partial Content, got {}",
            resp.status()
        ));
    }

    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;

    let mut written: u64 = 0;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("stream error: {e}"))?
    {
        // Stop as soon as our effective range has been shrunk away.
        let eff_end = live_end.map(|f| f()).unwrap_or(end);
        let pos = start + written;
        if pos > eff_end {
            break;
        }
        let keep = (((eff_end - pos + 1) as usize).min(chunk.len())).max(0);
        file.write_all(&chunk[..keep]).map_err(|e| e.to_string())?;
        written += keep as u64;
        on_chunk(keep as u64);
        if keep < chunk.len() {
            break; // wrote up to the (new) end — done early
        }
    }
    Ok(written)
}

/// Token-bucket throttled variant: enforces `bytes_per_sec` per segment.
/// Each chunk is paced so the average throughput stays at or below the limit.
pub async fn download_segment_throttled<W>(
    client: &SharedClient,
    url: &str,
    start: u64,
    end: u64,
    file: &mut W,
    on_chunk: &mut (dyn FnMut(u64) + Send),
    bytes_per_sec: u64,
    opts: &RequestOptions,
    live_end: Option<&(dyn Fn() -> u64 + Send + Sync)>,
) -> Result<u64, String>
where
    W: Write + Seek + Send,
{
    let range = format!("bytes={start}-{end}");
    let mut resp = opts
        .apply(client.get(url))
        .header(reqwest::header::RANGE, range)
        .send()
        .await
        .map_err(|e| format!("segment request failed: {e}"))?;

    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!(
            "expected 206 Partial Content, got {}",
            resp.status()
        ));
    }

    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;

    let mut written: u64 = 0;
    let mut bucket: i64 = bytes_per_sec as i64;
    let bucket_cap = bytes_per_sec as i64;
    let tick = std::time::Duration::from_millis(100); // refill every 100ms
    let mut last_refill = tokio::time::Instant::now();

    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("stream error: {e}"))?
    {
        // Refill the token bucket.
        let now = tokio::time::Instant::now();
        let elapsed = now.duration_since(last_refill);
        if elapsed >= tick {
            let refill = (elapsed.as_millis() as i64 / 100) * (bucket_cap / 10);
            bucket = (bucket + refill).min(bucket_cap);
            last_refill = now;
        }

        // If bucket is empty, wait until tokens are available.
        if bucket <= 0 {
            tokio::time::sleep(tick).await;
            let now2 = tokio::time::Instant::now();
            let refill =
                (now2.duration_since(last_refill).as_millis() as i64 / 100) * (bucket_cap / 10);
            bucket = (bucket + refill).min(bucket_cap);
            last_refill = now2;
        }

        let chunk_len = chunk.len() as i64;
        bucket -= chunk_len;

        // Dynamic re-allocation: stop (or trim) at the shrunken end.
        let eff_end = live_end.map(|f| f()).unwrap_or(end);
        let pos = start + written;
        if pos > eff_end {
            break;
        }
        let keep = (((eff_end - pos + 1) as usize).min(chunk.len())).max(0);
        file.write_all(&chunk[..keep]).map_err(|e| e.to_string())?;
        written += keep as u64;
        on_chunk(keep as u64);
        if keep < chunk.len() {
            break;
        }
    }
    Ok(written)
}

/// Plain (non-Range) download for servers that don't report a size:
/// streams the entire response body into `file` from the start.
/// Returns bytes written.
pub async fn download_plain<W>(
    client: &SharedClient,
    url: &str,
    file: &mut W,
    on_chunk: &mut (dyn FnMut(u64) + Send),
    opts: &RequestOptions,
) -> Result<u64, String>
where
    W: Write + Send,
{
    let mut resp = opts
        .apply(client.get(url))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("expected 200 OK, got {}", resp.status()));
    }

    let mut written: u64 = 0;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("stream error: {e}"))?
    {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        written += chunk.len() as u64;
        on_chunk(chunk.len() as u64);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io as std_io;

    #[tokio::test]
    async fn segment_hits_local_test_server() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let payload: Vec<u8> = (0..=255u8).cycle().take(64 * 1024).collect();

        let server_payload = payload.clone();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                let p = server_payload.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut sock = sock;
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let (s, e) = if let Some(r) = extract_range(&req) {
                        (r.0, r.1)
                    } else {
                        (0u64, p.len() as u64 - 1)
                    };
                    let s = s.min(p.len() as u64 - 1);
                    let e = e.min(p.len() as u64 - 1);
                    let body = &p[s as usize..=e as usize];
                    let resp = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        body.len(), s, e, p.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.write_all(body).await;
                });
            }
        });

        let client = crate::engine::build_client();
        let url = format!("http://{addr}/file.bin");

        let mut out: Vec<u8> = vec![0; payload.len()];
        let mut cursor = std_io::Cursor::new(&mut out);
        let total = download_segment(
            &client,
            &url,
            100,
            1123,
            &mut cursor,
            &mut |_| {},
            &RequestOptions::default(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(total, 1024);
        assert_eq!(&out[100..=1123], &payload[100..=1123]);
    }

    /// The download must stop at the *live* end even when the server keeps
    /// streaming the original (longer) range — the basis of work stealing.
    #[tokio::test]
    async fn segment_stops_at_shrunk_live_end() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let payload: Vec<u8> = (0..=255u8).cycle().take(64 * 1024).collect();

        let server_payload = payload.clone();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                let p = server_payload.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut sock = sock;
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let _ = String::from_utf8_lossy(&buf[..n]);
                    // Ignore the requested range — send the WHOLE payload as
                    // 206 so the client has far more bytes than it needs.
                    let resp = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes 0-{}/{}\r\nConnection: close\r\n\r\n",
                        p.len(), p.len() - 1, p.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.write_all(&p).await;
                });
            }
        });

        let client = crate::engine::build_client();
        let url = format!("http://{addr}/file.bin");

        let mut out: Vec<u8> = vec![0; 64 * 1024];
        let mut cursor = std_io::Cursor::new(&mut out);
        let stolen_at: u64 = 511; // live end shrunk from 1023 down to 511
        let total = download_segment(
            &client,
            &url,
            0,
            1023,
            &mut cursor,
            &mut |_| {},
            &RequestOptions::default(),
            Some(&|| stolen_at),
        )
        .await
        .unwrap();
        assert_eq!(total, stolen_at + 1); // wrote exactly 0..=511
    }

    #[tokio::test]
    async fn plain_download_hits_local_test_server() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let payload: Vec<u8> = (0..=255u8).cycle().take(64 * 1024).collect();

        let server_payload = payload.clone();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                let p = server_payload.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut sock = sock;
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let _ = String::from_utf8_lossy(&buf[..n]); // request (unused)
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        p.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.write_all(&p).await;
                });
            }
        });

        let client = crate::engine::build_client();
        let url = format!("http://{addr}/file.bin");

        let mut out: Vec<u8> = Vec::new();
        let total = download_plain(
            &client,
            &url,
            &mut out,
            &mut |_| {},
            &RequestOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(total as usize, payload.len());
        assert_eq!(out, payload);
    }

    fn extract_range(req: &str) -> Option<(u64, u64)> {
        let line = req
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("range:"))?;
        let spec = line.split_once(':')?.1.trim().strip_prefix("bytes=")?;
        let (a, b) = spec.split_once('-')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    }
}
