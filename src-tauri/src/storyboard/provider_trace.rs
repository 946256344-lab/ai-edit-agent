//! Storyboard 直连 Provider 本机轨迹（与 Native full-trace 分离）。
//!
//! - `STORYBOARD_PROVIDER_TRACE=1`（debug）：写入 `storyboard-provider-trace.jsonl`
//! - debug 构建默认：写入 `storyboard-pool-trace.jsonl`（每拍 P2 九条 + P3 所选）

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

/// debug 下默认开着，方便复测「九条里有没有更好的 / P3 选了谁」。
pub(crate) fn pool_trace_enabled() -> bool {
    cfg!(debug_assertions)
}

fn target_trace_dir() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Some(manifest_dir.join("target"))
}

fn trace_path() -> Option<PathBuf> {
    Some(target_trace_dir()?.join("storyboard-provider-trace.jsonl"))
}

fn pool_trace_path() -> Option<PathBuf> {
    Some(target_trace_dir()?.join("storyboard-pool-trace.jsonl"))
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
    append_jsonl_record(
        &path,
        json!({
            "tsMs": now_ms(),
            "phase": phase,
            "beatId": beat_id,
            "attempt": attempt,
            "direction": direction,
            "body": redact_trace_body(body),
        }),
    );
}

/// 追加选片决策一行：P2 池或 P3 选择。不含路径与图片。
pub(crate) fn append_pool_trace(phase: &str, beat_id: &str, body: &Value) {
    if !pool_trace_enabled() {
        return;
    }
    let Some(path) = pool_trace_path() else {
        return;
    };
    append_jsonl_record(
        &path,
        json!({
            "tsMs": now_ms(),
            "phase": phase,
            "beatId": beat_id,
            "body": body,
        }),
    );
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn append_jsonl_record(path: &PathBuf, record: Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(line) = serde_json::to_string(&record) else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
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
    use super::{pool_trace_enabled, redact_trace_body};
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

    #[test]
    fn pool_trace_follows_debug_assertions() {
        assert_eq!(pool_trace_enabled(), cfg!(debug_assertions));
    }
}
