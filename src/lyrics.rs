use crate::models::LyricLine;
use std::time::{Duration, Instant};
use windows::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::core::{PCWSTR, w};

const WINHTTP_USER_AGENT: PCWSTR = w!("CyNotch/0.3.0");
const WINHTTP_METHOD_GET: PCWSTR = w!("GET");
/// Strict upper bound (512 KiB) on HTTP lyrics response body to prevent OOM DoS.
const MAX_LYRICS_RESPONSE_BYTES: usize = 512 * 1024;

fn encode_utf16_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Accumulates raw HTTP response chunks up to `max_bytes` and decodes UTF-8 once
/// at the end so multi-byte UTF-8 characters split across read boundaries are never corrupted.
fn append_bounded_chunk(raw: &mut Vec<u8>, chunk: &[u8], max_bytes: usize) -> bool {
    if raw.len().saturating_add(chunk.len()) > max_bytes {
        return false;
    }
    raw.extend_from_slice(chunk);
    true
}

pub fn fetch_lyrics(track_title: &str, artist_name: &str) -> Vec<LyricLine> {
    if track_title.is_empty()
        || track_title == "No media active"
        || track_title == "No Media Playing"
    {
        return Vec::new();
    }

    let host = "lrclib.net";
    let path_and_query = format!(
        "/api/search?q={}+{}",
        urlencoding(track_title),
        urlencoding(artist_name)
    );

    let resp_body = match http_get_string(host, &path_and_query, 4000) {
        Some(body) => body,
        None => return Vec::new(),
    };

    parse_lrclib_response(&resp_body)
}

pub fn parse_lrclib_response(json_str: &str) -> Vec<LyricLine> {
    let body: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    if let Some(arr) = body.as_array() {
        for item in arr {
            if let Some(synced) = item.get("syncedLyrics").and_then(|s| s.as_str()) {
                let parsed = parse_lrc(synced);
                if !parsed.is_empty() {
                    return parsed;
                }
            }
        }
    }

    Vec::new()
}

fn http_get_string(host: &str, path_and_query: &str, timeout_ms: u32) -> Option<String> {
    unsafe {
        let host_wide = encode_utf16_z(host);
        let path_wide = encode_utf16_z(path_and_query);

        let session = WinHttpOpen(
            WINHTTP_USER_AGENT,
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            None,
            None,
            0,
        );
        if session.is_null() {
            return None;
        }

        let connect = WinHttpConnect(session, PCWSTR(host_wide.as_ptr()), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let request = WinHttpOpenRequest(
            connect,
            WINHTTP_METHOD_GET,
            PCWSTR(path_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_OPEN_REQUEST_FLAGS(WINHTTP_FLAG_SECURE.0),
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        // Set timeouts: resolve, connect, send, receive (all in ms) - expects i32
        let timeout_i32 = timeout_ms as i32;
        let _ = WinHttpSetTimeouts(request, timeout_i32, timeout_i32, timeout_i32, timeout_i32);

        let sent = WinHttpSendRequest(request, None, None, 0, 0, 0);
        if sent.is_err() {
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let received = WinHttpReceiveResponse(request, std::ptr::null_mut());
        if received.is_err() {
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        // BH-38: Require 2xx HTTP status before downloading response body.
        let mut status_code: u32 = 0;
        let mut status_len: u32 = std::mem::size_of::<u32>() as u32;
        let status_ok = WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status_code as *mut u32 as *mut core::ffi::c_void),
            &mut status_len,
            std::ptr::null_mut(),
        )
        .is_ok();
        if !status_ok || !(200..=299).contains(&status_code) {
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let total_deadline = Instant::now()
            + Duration::from_millis((timeout_ms as u64).saturating_mul(2).max(2000));
        let mut raw_body: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 8192];
        let mut aborted = false;
        loop {
            if Instant::now() >= total_deadline {
                aborted = true;
                break;
            }
            let mut bytes_read: u32 = 0;
            let result = WinHttpReadData(
                request,
                buffer.as_mut_ptr() as *mut _,
                buffer.len() as u32,
                &mut bytes_read,
            );
            if result.is_err() {
                // BH-38: Treat mid-stream I/O error as failure rather than returning truncated body.
                aborted = true;
                break;
            }
            if bytes_read == 0 {
                break;
            }
            let n = (bytes_read as usize).min(buffer.len());
            if !append_bounded_chunk(&mut raw_body, &buffer[..n], MAX_LYRICS_RESPONSE_BYTES) {
                aborted = true;
                break;
            }
        }

        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);

        if aborted || raw_body.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&raw_body).into_owned())
        }
    }
}

pub fn parse_lrc(lrc_text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();

    for raw_line in lrc_text.lines() {
        let trimmed = raw_line.trim();
        if !trimmed.starts_with('[') {
            continue;
        }

        if let Some(close_bracket) = trimmed.find(']') {
            let timestamp_str = &trimmed[1..close_bracket];
            let text = trimmed[close_bracket + 1..].trim();

            if let Some(secs) = parse_timestamp(timestamp_str)
                && !text.is_empty()
            {
                lines.push(LyricLine {
                    seconds: secs,
                    text: text.to_string(),
                });
            }
        }
    }

    lines.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
    lines
}

fn parse_timestamp(ts: &str) -> Option<f64> {
    // Expected format: mm:ss.xx or mm:ss
    let parts: Vec<&str> = ts.split(':').collect();
    if parts.len() != 2 {
        return None;
    }

    let minutes: f64 = parts[0].parse().ok()?;
    let seconds: f64 = parts[1].parse().ok()?;
    if !minutes.is_finite() || !seconds.is_finite() || minutes < 0.0 || seconds < 0.0 {
        return None;
    }

    let total = minutes * 60.0 + seconds;
    if total.is_finite() { Some(total) } else { None }
}

fn urlencoding(s: &str) -> String {
    let mut encoded = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{:02X}", b));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winhttp_constants_are_valid_utf16_null_terminated() {
        let ua = unsafe { WINHTTP_USER_AGENT.as_wide() };
        assert_eq!(String::from_utf16(ua).unwrap(), "CyNotch/0.3.0");
        assert_eq!(unsafe { *WINHTTP_USER_AGENT.0.add(ua.len()) }, 0);

        let method = unsafe { WINHTTP_METHOD_GET.as_wide() };
        assert_eq!(String::from_utf16(method).unwrap(), "GET");
        assert_eq!(unsafe { *WINHTTP_METHOD_GET.0.add(method.len()) }, 0);
    }

    #[test]
    fn encode_utf16_z_produces_null_terminated_utf16() {
        let wide = encode_utf16_z("lrclib.net");
        assert_eq!(*wide.last().unwrap(), 0u16);
        assert_eq!(String::from_utf16(&wide[..wide.len() - 1]).unwrap(), "lrclib.net");
    }

    #[test]
    fn bh12_bounded_chunks_preserve_split_utf8_and_enforce_size_limit() {
        // Multi-byte Japanese + emoji split across arbitrary chunk boundaries
        let text = "夢の続き 🎵 ありがとう";
        let bytes = text.as_bytes();
        let mut acc = Vec::new();
        for ch in bytes.chunks(2) {
            assert!(append_bounded_chunk(&mut acc, ch, 1024));
        }
        assert_eq!(String::from_utf8(acc).unwrap(), text);

        // Enforce strict upper bound (prevent OOM DoS)
        let mut capped = Vec::new();
        assert!(append_bounded_chunk(&mut capped, &[b'a'; 100], 128));
        assert!(!append_bounded_chunk(&mut capped, &[b'b'; 50], 128));
        assert_eq!(capped.len(), 100);
    }

    #[test]
    fn bh38_parse_lrc_rejects_nan_inf_and_negative_timestamps() {
        let lrc = "[NaN:00] bad1\n[inf:00] bad2\n[-1:30] bad3\n[01:-5] bad4\n[01:23.5] valid line";
        let lines = parse_lrc(lrc);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "valid line");
        assert!((lines[0].seconds - 83.5).abs() < 1e-6);
    }
}
