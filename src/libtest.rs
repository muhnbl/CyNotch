#[cfg(test)]
mod tests {
    use crate::lyrics::parse_lrclib_response;
    #[test]
    fn test_parse_lrclib_response_valid_json() {
        let json = r#"[
            {
                "id": 1,
                "trackName": "Test Song",
                "artistName": "Test Artist",
                "albumName": "Test Album",
                "duration": 240.5,
                "instrumental": false,
                "plainLyrics": "Line 1\nLine 2",
                "syncedLyrics": "[00:10.50]First line\n[00:20.75]Second line",
                "language": "en"
            }
        ]"#;
        let result = parse_lrclib_response(json);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].seconds, 10.5);
        assert_eq!(result[0].text, "First line");
        assert_eq!(result[1].seconds, 20.75);
        assert_eq!(result[1].text, "Second line");
    }

    #[test]
    fn test_parse_lrclib_response_empty_array() {
        let json = "[]";
        let result = parse_lrclib_response(json);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_lrclib_response_no_synced_lyrics() {
        let json = r#"[
            {
                "id": 1,
                "trackName": "Test Song",
                "artistName": "Test Artist",
                "albumName": "Test Album",
                "duration": 240.5,
                "instrumental": false,
                "plainLyrics": "Line 1\nLine 2",
                "syncedLyrics": null,
                "language": "en"
            }
        ]"#;
        let result = parse_lrclib_response(json);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_lrclib_response_invalid_json() {
        let json = "{ invalid json }";
        let result = parse_lrclib_response(json);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_lrclib_response_empty_string() {
        let json = "";
        let result = parse_lrclib_response(json);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_lrclib_response_malformed_synced() {
        let json = r#"[
            {
                "id": 1,
                "trackName": "Test Song",
                "artistName": "Test Artist",
                "albumName": "Test Album",
                "duration": 240.5,
                "instrumental": false,
                "plainLyrics": "Line 1\nLine 2",
                "syncedLyrics": "not a valid lrc format",
                "language": "en"
            }
        ]"#;
        let result = parse_lrclib_response(json);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_lrclib_response_multiple_tracks_picks_first_with_synced() {
        let json = r#"[
            {
                "id": 1,
                "trackName": "Test Song",
                "artistName": "Test Artist",
                "albumName": "Test Album",
                "duration": 240.5,
                "instrumental": false,
                "plainLyrics": "Line 1\nLine 2",
                "syncedLyrics": null,
                "language": "en"
            },
            {
                "id": 2,
                "trackName": "Test Song",
                "artistName": "Test Artist",
                "albumName": "Test Album",
                "duration": 240.5,
                "instrumental": false,
                "plainLyrics": "Line 1\nLine 2",
                "syncedLyrics": "[00:05.00]First synced\n[00:15.25]Second synced",
                "language": "en"
            }
        ]"#;
        let result = parse_lrclib_response(json);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].seconds, 5.0);
        assert_eq!(result[0].text, "First synced");
        assert_eq!(result[1].seconds, 15.25);
        assert_eq!(result[1].text, "Second synced");
    }
}