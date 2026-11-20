use crate::models::LyricLine;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
};
use windows_core::PCWSTR;

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
        let host_wide: Vec<u16> = host.encode_utf16().chain(std::iter::once(0)).collect();
        let path_wide: Vec<u16> = path_and_query.encode_utf16().chain(std::iter::once(0)).collect();

        let session = WinHttpOpen(
            PCWSTR::from_raw("CyNotch/0.3.0\0".as_ptr() as *const u16),
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
            PCWSTR::from_raw("GET\0".as_ptr() as *const u16),
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

        let mut body = String::new();
        let mut buffer = [0u8; 8192];
        loop {
            let mut bytes_read: u32 = 0;
            let result = WinHttpReadData(
                request,
                buffer.as_mut_ptr() as *mut _,
                buffer.len() as u32,
                &mut bytes_read,
            );
            if result.is_err() || bytes_read == 0 {
                break;
            }
            body.push_str(&String::from_utf8_lossy(&buffer[..bytes_read as usize]));
        }

        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);

        if body.is_empty() {
            None
        } else {
            Some(body)
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

    Some(minutes * 60.0 + seconds)
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
