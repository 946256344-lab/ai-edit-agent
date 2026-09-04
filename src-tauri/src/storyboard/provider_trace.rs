//! Storyboard 直连 Provider 本机轨迹（与 Native full-trace 分离）。
//!
//! 仅在 debug 构建且 `STORYBOARD_PROVIDER_TRACE=1` 时写入
//! `src-tauri/target/storyboard-provider-trace.jsonl`。

use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn storyboard_trace_enabled() -> bool {
    cfg!(debug_assertions)
        && std::env::var("STORYBOARD_PROVIDER_TRACE")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

fn trace_path() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Some(
        manifest_dir
            .join("target")
            .join("storyboard-provider-trace.jsonl"),
    )
}

/// 遮蔽明显敏感字段后追加一行 JSONL。
pub(crate) fn append_storyboard_trace(
    phase: &str,
    beat_id: Option<&str>,
    attempt: usize,
    direction: &str,
    body: &Value,
) {
    if !storyboard_trace_enabled() {
        return;
    }
    let Some(path) = trace_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let record = json!({
        "tsMs": millis,
        "phase": phase,
        "beatId": beat_id,
        "attempt": attempt,
        "direction": direction,
        "body": redact_trace_body(body),
    });
    let Ok(line) = serde_json::to_string(&record) else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let _ = writeln!(file, "{line}");
}

fn redact_trace_body(body: &Value) -> Value {
    match body {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                let lower = key.to_ascii_lowercase();
                if lower.contains("authorization")
                    || lower.contains("api_key")
                    || lower.contains("apikey")
                    || lower.contains("token")
                    || lower.contains("password")
                    || lower.contains("secret")
                {
                    out.insert(key.clone(), Value::String("[redacted]".to_owned()));
                } else if lower.contains("image") || lower == "image_url" {
                    out.insert(key.clone(), Value::String("[image omitted]".to_owned()));
                } else {
                    out.insert(key.clone(), redact_trace_body(value));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_trace_body).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact_trace_body;
    use serde_json::json;

    #[test]
    fn redacts_authorization_and_images() {
        let body = json!({
            "Authorization": "Bearer secret",
            "input": [{ "type": "input_image", "image_url": "data:image/jpeg;base64,AAA" }],
            "phase": "Phase 3"
        });
        let redacted = redact_trace_body(&body);
        assert_eq!(redacted["Authorization"], "[redacted]");
        assert_eq!(redacted["input"][0]["image_url"], "[image omitted]");
        assert_eq!(redacted["phase"], "Phase 3");
    }
}
