use crate::models::LyricLine;

pub fn fetch_lyrics(track_title: &str, artist_name: &str) -> Vec<LyricLine> {
    if track_title.is_empty() || track_title == "No media active" || track_title == "No Media Playing" {
        return Vec::new();
    }

    let url = format!(
        "https://lrclib.net/api/search?q={}+{}",
        urlencoding(track_title),
        urlencoding(artist_name)
    );

    let resp = match ureq::get(&url)
        .timeout(std::time::Duration::from_secs(4))
        .call()
    {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let body: serde_json::Value = match resp.into_json() {
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
                && !text.is_empty() {
                    lines.push(LyricLine {
                        seconds: secs,
                        text: text.to_string(),
                    });
                }
        }
    }

    lines.sort_by(|a, b| a.seconds.partial_cmp(&b.seconds).unwrap());
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
