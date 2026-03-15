//! Snapshot testing for Rapina API responses.
//!
//! Provides golden-file testing: capture API response bodies as `.snap` files
//! and compare against them on subsequent runs. Dynamic values (UUIDs,
//! timestamps) are automatically redacted so snapshots stay stable.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

fn uuid_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap()
    })
}

fn timestamp_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:?\d{2})?").unwrap()
    })
}

/// Returns `true` if the `RAPINA_BLESS` environment variable is set to `"1"`.
pub fn is_bless_mode() -> bool {
    std::env::var("RAPINA_BLESS").is_ok_and(|v| v == "1")
}

/// Redact dynamic values in a JSON value, replacing them with stable placeholders.
///
/// - UUID v4 strings → `"[UUID]"`
/// - ISO 8601 timestamps → `"[TIMESTAMP]"`
/// - Any value under a `trace_id` key → `"[UUID]"`
pub fn redact(value: &mut serde_json::Value) {
    redact_inner(value, None);
}

fn redact_inner(value: &mut serde_json::Value, key: Option<&str>) {
    match value {
        serde_json::Value::String(s) => {
            if key == Some("trace_id") {
                *s = "[UUID]".to_string();
                return;
            }
            if uuid_regex().is_match(s) {
                *s = uuid_regex().replace_all(s, "[UUID]").to_string();
                return;
            }
            if timestamp_regex().is_match(s) {
                *s = timestamp_regex().replace_all(s, "[TIMESTAMP]").to_string();
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                redact_inner(v, Some(k));
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                redact_inner(v, None);
            }
        }
        _ => {}
    }
}

/// Format a snapshot with a status line, content type, and body.
pub fn format_snapshot(status: u16, content_type: &str, body: &str) -> String {
    let reason = reason_phrase(status);
    format!(
        "HTTP {} {}\nContent-Type: {}\n\n{}\n",
        status, reason, content_type, body
    )
}

/// Assert a response matches its snapshot file, using `snapshots/` as the base directory.
///
/// In bless mode (`RAPINA_BLESS=1`), writes the snapshot. Otherwise, compares
/// against the existing snapshot and panics with a diff on mismatch.
pub fn assert_snapshot(name: &str, status: u16, content_type: &str, body: &[u8]) {
    assert_snapshot_impl(
        name,
        status,
        content_type,
        body,
        Path::new("snapshots"),
        is_bless_mode(),
    );
}

/// Assert a response matches its snapshot file under a given base directory.
///
/// The `bless` parameter controls whether to write or compare.
#[cfg_attr(not(test), allow(dead_code))]
pub fn assert_snapshot_in(
    name: &str,
    status: u16,
    content_type: &str,
    body: &[u8],
    base_dir: &Path,
    bless: bool,
) {
    assert_snapshot_impl(name, status, content_type, body, base_dir, bless);
}

fn assert_snapshot_impl(
    name: &str,
    status: u16,
    content_type: &str,
    body: &[u8],
    base_dir: &Path,
    bless: bool,
) {
    let display_body = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(mut val) => {
            redact(&mut val);
            serde_json::to_string_pretty(&val).unwrap()
        }
        Err(_) => String::from_utf8_lossy(body).to_string(),
    };

    let snapshot = format_snapshot(status, content_type, &display_body);
    let snap_path = base_dir.join(format!("{}.snap", name));

    if bless {
        fs::create_dir_all(snap_path.parent().unwrap()).unwrap();
        fs::write(&snap_path, &snapshot).unwrap();
        return;
    }

    let expected = fs::read_to_string(&snap_path).unwrap_or_else(|_| {
        panic!(
            "Snapshot '{}' not found at {}. Run with --bless to create it.",
            name,
            snap_path.display()
        )
    });

    if snapshot != expected {
        panic!(
            "Snapshot '{}' mismatch!\n\n--- expected ({})\n+++ actual\n\n{}",
            name,
            snap_path.display(),
            line_diff(&expected, &snapshot)
        );
    }
}

fn reason_phrase(status: u16) -> &'static str {
    http::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("Unknown")
}

fn line_diff(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let mut output = String::new();

    let max = expected_lines.len().max(actual_lines.len());
    for i in 0..max {
        match (expected_lines.get(i), actual_lines.get(i)) {
            (Some(e), Some(a)) if e == a => {
                output.push_str(&format!("  {}\n", e));
            }
            (Some(e), Some(a)) => {
                output.push_str(&format!("- {}\n", e));
                output.push_str(&format!("+ {}\n", a));
            }
            (Some(e), None) => {
                output.push_str(&format!("- {}\n", e));
            }
            (None, Some(a)) => {
                output.push_str(&format!("+ {}\n", a));
            }
            (None, None) => {}
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- redact ---

    #[test]
    fn test_redact_uuid() {
        let mut val = json!({"id": "550e8400-e29b-41d4-a716-446655440000"});
        redact(&mut val);
        assert_eq!(val["id"], "[UUID]");
    }

    #[test]
    fn test_redact_timestamp() {
        let mut val = json!({"created_at": "2026-03-14T10:30:00Z"});
        redact(&mut val);
        assert_eq!(val["created_at"], "[TIMESTAMP]");
    }

    #[test]
    fn test_redact_timestamp_with_fractional_seconds() {
        let mut val = json!({"updated_at": "2026-03-14T10:30:00.123456+00:00"});
        redact(&mut val);
        assert_eq!(val["updated_at"], "[TIMESTAMP]");
    }

    #[test]
    fn test_redact_trace_id_key() {
        let mut val = json!({"trace_id": "not-a-uuid-but-still-redacted"});
        redact(&mut val);
        assert_eq!(val["trace_id"], "[UUID]");
    }

    #[test]
    fn test_redact_nested() {
        let mut val = json!({
            "user": {
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "name": "Alice"
            },
            "items": [
                {"created_at": "2026-01-01T00:00:00Z"}
            ]
        });
        redact(&mut val);
        assert_eq!(val["user"]["id"], "[UUID]");
        assert_eq!(val["user"]["name"], "Alice");
        assert_eq!(val["items"][0]["created_at"], "[TIMESTAMP]");
    }

    #[test]
    fn test_redact_non_string_untouched() {
        let mut val = json!({"count": 42, "active": true, "data": null});
        let original = val.clone();
        redact(&mut val);
        assert_eq!(val, original);
    }

    #[test]
    fn test_redact_no_false_positives() {
        let mut val = json!({"name": "hello", "code": "NOT_FOUND", "short": "abc-123"});
        let original = val.clone();
        redact(&mut val);
        assert_eq!(val, original);
    }

    // --- format_snapshot ---

    #[test]
    fn test_format_snapshot_json() {
        let snap = format_snapshot(200, "application/json", "{\n  \"id\": 1\n}");
        assert!(snap.starts_with("HTTP 200 OK\n"));
        assert!(snap.contains("Content-Type: application/json"));
        assert!(snap.contains("\"id\": 1"));
    }

    #[test]
    fn test_format_snapshot_404() {
        let snap = format_snapshot(404, "text/plain", "not found");
        assert!(snap.starts_with("HTTP 404 Not Found\n"));
        assert!(snap.contains("Content-Type: text/plain"));
    }

    #[test]
    fn test_format_snapshot_plain_text() {
        let snap = format_snapshot(200, "text/plain", "Hello, world!");
        assert!(snap.contains("Content-Type: text/plain"));
    }

    // --- line_diff ---

    #[test]
    fn test_diff_identical() {
        let diff = line_diff("a\nb\nc\n", "a\nb\nc\n");
        assert!(!diff.contains("- "));
        assert!(!diff.contains("+ "));
    }

    #[test]
    fn test_diff_changed_line() {
        let diff = line_diff("a\nold\nc\n", "a\nnew\nc\n");
        assert!(diff.contains("- old\n"));
        assert!(diff.contains("+ new\n"));
        assert!(diff.contains("  a\n"));
        assert!(diff.contains("  c\n"));
    }

    #[test]
    fn test_diff_added_line() {
        let diff = line_diff("a\n", "a\nb\n");
        assert!(diff.contains("+ b\n"));
    }

    #[test]
    fn test_diff_removed_line() {
        let diff = line_diff("a\nb\n", "a\n");
        assert!(diff.contains("- b\n"));
    }

    // --- assert_snapshot_in (file I/O) ---

    #[test]
    fn test_bless_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let snap_dir = dir.path().join("snaps");

        assert_snapshot_in(
            "test_endpoint",
            200,
            "application/json",
            br#"{"name":"Alice","trace_id":"550e8400-e29b-41d4-a716-446655440000"}"#,
            &snap_dir,
            true,
        );

        let content = fs::read_to_string(snap_dir.join("test_endpoint.snap")).unwrap();
        assert!(content.starts_with("HTTP 200 OK\n"));
        assert!(content.contains("[UUID]"));
        assert!(!content.contains("550e8400"));
    }

    #[test]
    fn test_compare_passes_on_match() {
        let dir = tempfile::tempdir().unwrap();
        let snap_dir = dir.path().join("snaps");

        // Bless first
        assert_snapshot_in(
            "match_test",
            200,
            "application/json",
            br#"{"ok":true}"#,
            &snap_dir,
            true,
        );

        // Compare — should not panic
        assert_snapshot_in(
            "match_test",
            200,
            "application/json",
            br#"{"ok":true}"#,
            &snap_dir,
            false,
        );
    }

    #[test]
    #[should_panic(expected = "mismatch")]
    fn test_compare_panics_on_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let snap_dir = dir.path().join("snaps");

        // Bless with one value
        assert_snapshot_in(
            "mismatch_test",
            200,
            "application/json",
            br#"{"ok":true}"#,
            &snap_dir,
            true,
        );

        // Compare with different value — should panic
        assert_snapshot_in(
            "mismatch_test",
            200,
            "application/json",
            br#"{"ok":false}"#,
            &snap_dir,
            false,
        );
    }

    #[test]
    #[should_panic(expected = "not found")]
    fn test_compare_panics_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let snap_dir = dir.path().join("snaps");

        // No bless, no file — should panic
        assert_snapshot_in(
            "nonexistent",
            200,
            "application/json",
            br#"{"ok":true}"#,
            &snap_dir,
            false,
        );
    }
}
