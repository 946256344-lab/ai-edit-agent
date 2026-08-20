//! Native Provider 开发转储。
//!
//! 只有 debug 构建且显式开启时，才把实际请求 JSON 与原始响应写入
//! `src-tauri/target/native-provider-full-trace.jsonl`，供本机调试读取。
//! 不写 SQLite、浏览器存储或普通产品日志，记录不含 Authorization 头。

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const FULL_TRACE_ENV: &str = "NATIVE_PROVIDER_FULL_TRACE";
const FULL_TRACE_FILE_NAME: &str = "native-provider-full-trace.jsonl";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeProviderFullTraceRecord {
    record_id: u64,
    step_number: usize,
    attempt_number: usize,
    direction: String,
    adapter: String,
    http_status: Option<u16>,
    body: String,
    created_at: u128,
}

static NEXT_FULL_TRACE_RECORD_ID: AtomicU64 = AtomicU64::new(1);
static TRACE_FILE_PREPARED: AtomicBool = AtomicBool::new(false);

pub(super) fn emit_native_provider_request(
    step_number: usize,
    attempt_number: usize,
    adapter: &str,
    body: &str,
) {
    emit_native_provider_full_trace(step_number, attempt_number, "request", adapter, None, body);
}

pub(super) fn emit_native_provider_response(
    step_number: usize,
    attempt_number: usize,
    adapter: &str,
    http_status: u16,
    body: &str,
) {
    emit_native_provider_full_trace(
        step_number,
        attempt_number,
        "response",
        adapter,
        Some(http_status),
        body,
    );
}

fn emit_native_provider_full_trace(
    step_number: usize,
    attempt_number: usize,
    direction: &str,
    adapter: &str,
    http_status: Option<u16>,
    body: &str,
) {
    if !native_provider_full_trace_enabled() {
        return;
    }
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let record = NativeProviderFullTraceRecord {
        record_id: next_full_trace_record_id(),
        step_number,
        attempt_number,
        direction: direction.to_owned(),
        adapter: adapter.to_owned(),
        http_status,
        body: body.to_owned(),
        created_at,
    };
    let path = native_provider_trace_path();
    prepare_trace_file(&path);
    let _ = append_trace_record(&path, &record);
}

fn native_provider_trace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(FULL_TRACE_FILE_NAME)
}

fn prepare_trace_file(path: &Path) {
    if TRACE_FILE_PREPARED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, "");
    log::info!("native provider full trace enabled");
}

fn append_trace_record(path: &Path, record: &NativeProviderFullTraceRecord) -> Result<(), String> {
    let line = serde_json::to_string(record).map_err(|_| "native_provider_trace_serialize")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| "native_provider_trace_unavailable")?;
    writeln!(file, "{line}").map_err(|_| "native_provider_trace_unavailable")?;
    Ok(())
}

fn next_full_trace_record_id() -> u64 {
    NEXT_FULL_TRACE_RECORD_ID.fetch_add(1, Ordering::Relaxed)
}

pub(super) fn native_provider_full_trace_enabled() -> bool {
    full_trace_enabled(
        cfg!(debug_assertions),
        std::env::var(FULL_TRACE_ENV).ok().as_deref(),
    )
}

fn full_trace_enabled(debug_build: bool, configured: Option<&str>) -> bool {
    debug_build
        && configured.is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on" | "yes"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom_api::CustomApiConfig;
    use crate::provider::chat_completions_request;
    use serde_json::json;
    use std::fs;

    #[test]
    fn full_trace_requires_both_debug_build_and_explicit_switch() {
        assert!(!full_trace_enabled(true, None));
        assert!(!full_trace_enabled(true, Some("off")));
        assert!(!full_trace_enabled(false, Some("1")));
        assert!(full_trace_enabled(true, Some("1")));
        assert!(full_trace_enabled(true, Some("YES")));
    }

    #[test]
    fn full_trace_event_keeps_the_complete_body_without_headers() {
        let body =
            r#"{"input":"complete_prompt_marker","nested":{"value":"complete_value_marker"}}"#;
        let event = NativeProviderFullTraceRecord {
            record_id: 1,
            step_number: 2,
            attempt_number: 1,
            direction: "request".to_owned(),
            adapter: "chat_completions".to_owned(),
            http_status: None,
            body: body.to_owned(),
            created_at: 1,
        };
        let serialized = serde_json::to_string(&event).expect("serialize full trace event");
        assert!(serialized.contains("complete_prompt_marker"));
        assert!(serialized.contains("complete_value_marker"));
        assert!(!serialized.to_ascii_lowercase().contains("authorization"));
        assert!(!serialized.to_ascii_lowercase().contains("api_key"));
    }

    #[test]
    fn custom_chat_trace_keeps_wire_payload_without_custom_provider_credentials() {
        let config = CustomApiConfig {
            base_url: "https://private-provider.example/v1".to_owned(),
            model: "trace-model".to_owned(),
            coarse_visual_model: String::new(),
            api_key: "trace-secret-api-key".to_owned(),
        };
        let payload = json!({
            "store": false,
            "input": [
                {
                    "role": "user",
                    "content": [{"type": "input_text", "text": "complete-user-input-marker"}]
                },
                {
                    "type": "function_call",
                    "call_id": "call_trace_1",
                    "name": "list_assets",
                    "arguments": "{}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_trace_1",
                    "output": "{\"status\":\"ok\",\"total\":3}"
                }
            ],
            "tools": [{
                "type": "function",
                "name": "list_assets",
                "description": "List project assets.",
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "required": [],
                    "additionalProperties": false
                }
            }],
            "tool_choice": "auto",
            "parallel_tool_calls": false
        });

        let wire_body = chat_completions_request(&config, &payload).to_string();
        let event = NativeProviderFullTraceRecord {
            record_id: 1,
            step_number: 2,
            attempt_number: 1,
            direction: "request".to_owned(),
            adapter: "chat_completions".to_owned(),
            http_status: None,
            body: wire_body.clone(),
            created_at: 1,
        };
        let serialized = serde_json::to_string(&event).expect("serialize custom provider trace");
        let wire_json: serde_json::Value =
            serde_json::from_str(&wire_body).expect("parse custom provider wire body");

        assert_eq!(wire_json["model"], "trace-model");
        assert_eq!(
            wire_json["messages"][0]["content"][0]["text"],
            "complete-user-input-marker"
        );
        assert_eq!(
            wire_json["messages"][1]["tool_calls"][0]["function"]["name"],
            "list_assets"
        );
        assert_eq!(wire_json["messages"][2]["tool_call_id"], "call_trace_1");
        assert_eq!(
            wire_json["messages"][2]["content"],
            "{\"status\":\"ok\",\"total\":3}"
        );
        assert_eq!(wire_json["parallel_tool_calls"], false);
        assert!(!serialized.contains(&config.api_key));
        assert!(!serialized.contains(&config.base_url));
        assert!(!serialized.to_ascii_lowercase().contains("authorization"));
    }

    #[test]
    fn append_trace_record_writes_json_lines_without_headers() {
        let directory = std::env::temp_dir().join(format!(
            "native-provider-trace-{}",
            next_full_trace_record_id()
        ));
        fs::create_dir_all(&directory).expect("create temp trace directory");
        let path = directory.join(FULL_TRACE_FILE_NAME);
        let record = NativeProviderFullTraceRecord {
            record_id: 9,
            step_number: 1,
            attempt_number: 2,
            direction: "response".to_owned(),
            adapter: "chat_completions".to_owned(),
            http_status: Some(200),
            body: "{\"id\":\"complete-output-marker\"}".to_owned(),
            created_at: 1,
        };

        append_trace_record(&path, &record).expect("write first trace line");
        append_trace_record(&path, &record).expect("write second trace line");

        let contents = fs::read_to_string(&path).expect("read temp trace file");
        assert_eq!(contents.lines().count(), 2);
        assert!(contents.contains("complete-output-marker"));
        assert!(!contents.to_ascii_lowercase().contains("authorization"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn full_trace_records_receive_process_unique_monotonic_ids() {
        let first = next_full_trace_record_id();
        let second = next_full_trace_record_id();
        assert!(second > first);
    }

    #[test]
    fn native_provider_trace_path_stays_inside_target() {
        let path = native_provider_trace_path();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(FULL_TRACE_FILE_NAME)
        );
        assert!(path
            .components()
            .any(|component| component.as_os_str() == "target"));
    }
}
