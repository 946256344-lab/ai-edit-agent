//! 应用运行日志的有界只读入口。
//!
//! 模型不能选择文件路径，只能读取 tauri-plugin-log 当前活动文件的有限行范围。
//! 凭据、URL 和完整本机路径所在行在进入模型上下文前整体遮蔽。

use serde_json::{json, Value};
use std::path::Path;
use tauri::Manager;

const DEFAULT_TAIL_LINES: usize = 100;
const MAX_LOG_LINES: usize = 100;
const MAX_LOG_LINE_CHARS: usize = 500;
const MAX_LOG_OUTPUT_CHARS: usize = 3_500;

pub(super) fn read_application_logs(
    app: &tauri::AppHandle,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<Value, String> {
    let path = app
        .path()
        .app_log_dir()
        .map_err(|_| "application_log_directory_unavailable".to_owned())?
        .join(format!("{}.log", app.package_info().name));
    read_application_log_path(&path, start_line, end_line)
}

fn read_application_log_path(
    path: &Path,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<Value, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return Err("application_log_unavailable".to_owned()),
    };
    let content = String::from_utf8_lossy(&bytes);
    let lines = content.lines().collect::<Vec<_>>();
    select_log_lines(&lines, start_line, end_line)
}

fn select_log_lines(
    lines: &[&str],
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<Value, String> {
    let total_lines = lines.len();
    let (start, end) = match (start_line, end_line) {
        (None, None) if total_lines == 0 => return Ok(empty_log_result()),
        (None, None) => (
            total_lines.saturating_sub(DEFAULT_TAIL_LINES) + 1,
            total_lines,
        ),
        (Some(start), Some(end))
            if start >= 1
                && start <= end
                && end <= total_lines
                && end - start + 1 <= MAX_LOG_LINES =>
        {
            (start, end)
        }
        _ => return Err("invalid_log_line_range".to_owned()),
    };

    let mut output_chars = 0usize;
    let mut returned = Vec::new();
    let mut next_start_line = None;
    for line_number in start..=end {
        let text = sanitize_log_line(lines[line_number - 1]);
        let line_chars = text.chars().count();
        if !returned.is_empty() && output_chars + line_chars > MAX_LOG_OUTPUT_CHARS {
            next_start_line = Some(line_number);
            break;
        }
        output_chars += line_chars;
        returned.push(json!({"lineNumber": line_number, "text": text}));
    }
    let returned_end = returned.last().and_then(|line| line["lineNumber"].as_u64());
    Ok(json!({
        "tool": "read_logs",
        "status": "ok",
        "totalLines": total_lines,
        "startLine": start,
        "endLine": returned_end,
        "lines": returned,
        "truncated": next_start_line.is_some(),
        "nextStartLine": next_start_line
    }))
}

fn empty_log_result() -> Value {
    json!({
        "tool": "read_logs",
        "status": "ok",
        "totalLines": 0,
        "startLine": null,
        "endLine": null,
        "lines": [],
        "truncated": false,
        "nextStartLine": null
    })
}

fn sanitize_log_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let has_sensitive_marker = [
        "authorization",
        "bearer ",
        "api_key",
        "apikey",
        "access_token",
        "password",
        "secret",
        "http://",
        "https://",
        "file://",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    let has_windows_path = line.as_bytes().windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    }) || line.contains("\\\\");
    if has_sensitive_marker || has_windows_path {
        return "[redacted_sensitive_log_line]".to_owned();
    }
    line.chars().take(MAX_LOG_LINE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_range_is_one_based_and_inclusive() {
        let lines = ["one", "two", "three"];
        let result = select_log_lines(&lines, Some(2), Some(3)).expect("read range");
        assert_eq!(result["startLine"], 2);
        assert_eq!(result["endLine"], 3);
        assert_eq!(result["lines"][0]["text"], "two");
        assert_eq!(result["lines"][1]["text"], "three");
    }

    #[test]
    fn default_range_reads_only_the_latest_hundred_lines() {
        let owned = (1..=120)
            .map(|line| format!("line-{line}"))
            .collect::<Vec<_>>();
        let lines = owned.iter().map(String::as_str).collect::<Vec<_>>();
        let result = select_log_lines(&lines, None, None).expect("read tail");
        assert_eq!(result["startLine"], 21);
        assert_eq!(result["endLine"], 120);
        assert_eq!(result["lines"].as_array().map(Vec::len), Some(100));
    }

    #[test]
    fn invalid_or_oversized_ranges_fail_closed() {
        let lines = vec!["line"; 101];
        for range in [
            (Some(0), Some(1)),
            (Some(2), Some(1)),
            (Some(1), Some(102)),
            (Some(1), Some(101)),
            (Some(1), None),
        ] {
            assert_eq!(
                select_log_lines(&lines, range.0, range.1),
                Err("invalid_log_line_range".to_owned())
            );
        }
    }

    #[test]
    fn credentials_paths_and_urls_are_redacted_but_diagnostics_remain_readable() {
        for line in [
            r"failed at C:\Users\Example\video.mp4",
            r"failed at \\server\share\video.mp4",
            "Authorization: Bearer value",
            "provider=https://example.test/v1",
            "password=value",
        ] {
            assert_eq!(sanitize_log_line(line), "[redacted_sensitive_log_line]");
        }
        assert_eq!(
            sanitize_log_line("Local media analysis failed for asset asset-123: ffprobe_timeout"),
            "Local media analysis failed for asset asset-123: ffprobe_timeout"
        );
        assert_eq!(sanitize_log_line(&"x".repeat(700)).chars().count(), 500);
    }

    #[test]
    fn output_budget_returns_a_stable_paging_cursor() {
        let owned = (0..20).map(|_| "x".repeat(500)).collect::<Vec<_>>();
        let lines = owned.iter().map(String::as_str).collect::<Vec<_>>();
        let result = select_log_lines(&lines, Some(1), Some(20)).expect("read bounded range");
        assert_eq!(result["truncated"], true);
        assert_eq!(result["nextStartLine"], 8);
        assert_eq!(result["endLine"], 7);
    }
}
