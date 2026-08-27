//! Native Provider 上下文的 token 计量与压缩选择。
//!
//! 历史事实仍来自 SQLite；本模块只在请求超过预算时划分“交给模型压缩的旧历史”和
//! 必须原样保留的当前请求、状态快照、近期消息及最近函数调用收据。

use serde_json::{json, Value};
use std::collections::BTreeSet;
use tiktoken_rs::o200k_base_singleton;

pub(super) const COMPRESSION_TRIGGER_TOKENS: usize = 40_000;
pub(super) const COMPRESSION_TARGET_TOKENS: usize = 30_000;
pub(super) const MAX_CONTEXT_TOKENS: usize = 60_000;
const RECENT_RAW_TOKENS: usize = 8_000;
pub(super) const SUMMARY_BATCH_TOKENS: usize = 18_000;
const MAX_TOOL_OUTPUT_TOKENS: usize = 4_000;

pub(super) fn token_count_text(text: &str) -> usize {
    let chunks = bounded_text_chunks(text, 12_000);
    chunks
        .iter()
        .map(|chunk| o200k_base_singleton().encode_ordinary(chunk).len())
        .sum::<usize>()
        .saturating_add(chunks.len().saturating_sub(1))
}

pub(super) fn token_count_value(value: &Value) -> usize {
    token_count_text(&value.to_string())
}

pub(super) fn provider_payload_tokens(input: &[Value], tools: &[Value]) -> usize {
    token_count_value(&json!({
        "model": "gpt-5.4",
        "store": false,
        "stream": false,
        "parallel_tool_calls": false,
        "tool_choice": "auto",
        "tools": tools,
        "input": input,
    }))
}

pub(super) fn compact_tool_outputs(input: &mut [Value]) {
    for item in input {
        if item["type"] != "function_call_output" {
            continue;
        }
        let Some(output) = item["output"].as_str() else {
            continue;
        };
        if token_count_text(output) <= MAX_TOOL_OUTPUT_TOKENS {
            continue;
        }
        item["output"] = Value::String(format!(
            "{}...[truncated]",
            truncate_text_to_tokens(output, MAX_TOOL_OUTPUT_TOKENS)
        ));
    }
}

pub(super) struct CompressionPlan {
    pub(super) older: Vec<Value>,
    pub(super) retained: Vec<Value>,
}

pub(super) fn compression_plan(input: &[Value], request: &str) -> CompressionPlan {
    let latest_call_id = input.iter().rev().find_map(|item| {
        (item["type"] == "function_call")
            .then(|| item["call_id"].as_str().map(str::to_owned))
            .flatten()
    });
    let mut protected = input
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            (index == 0
                || super::snapshot::is_snapshot_message(item)
                || is_current_user_item(item, request)
                || latest_call_id.as_ref().is_some_and(|call_id| {
                    item["call_id"].as_str() == Some(call_id)
                        && matches!(
                            item["type"].as_str(),
                            Some("function_call") | Some("function_call_output")
                        )
                }))
            .then_some(index)
        })
        .collect::<BTreeSet<_>>();

    let mut recent_tokens = 0usize;
    for (index, item) in input.iter().enumerate().rev() {
        if protected.contains(&index) {
            continue;
        }
        let item_tokens = token_count_value(item);
        if recent_tokens + item_tokens > RECENT_RAW_TOKENS {
            break;
        }
        recent_tokens += item_tokens;
        protected.insert(index);
    }

    let (older, retained) = input
        .iter()
        .cloned()
        .enumerate()
        .partition::<Vec<_>, _>(|(index, _)| !protected.contains(index));
    CompressionPlan {
        older: older.into_iter().map(|(_, item)| item).collect(),
        retained: retained.into_iter().map(|(_, item)| item).collect(),
    }
}

pub(super) fn summary_batches(items: Vec<Value>) -> Vec<String> {
    let mut batches = Vec::new();
    let mut current = String::new();
    for item in items {
        let serialized = format!("{}\n", item);
        for chunk in split_text_to_token_chunks(&serialized, SUMMARY_BATCH_TOKENS) {
            if !current.is_empty()
                && token_count_text(&current) + token_count_text(&chunk) > SUMMARY_BATCH_TOKENS
            {
                batches.push(std::mem::take(&mut current));
            }
            current.push_str(&chunk);
        }
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

pub(super) fn memory_item(summary: &str) -> Value {
    json!({
        "role": "system",
        "content": [{
            "type": "input_text",
            "text": format!(
                "Compressed conversation memory (not authoritative project state):\n{summary}"
            )
        }]
    })
}

fn is_current_user_item(item: &Value, request: &str) -> bool {
    item["role"] == "user"
        && item["content"]
            .as_array()
            .and_then(|content| content.first())
            .and_then(|content| content["text"].as_str())
            .is_some_and(|text| text == request)
}

pub(super) fn truncate_text_to_tokens(text: &str, max_tokens: usize) -> String {
    let bpe = o200k_base_singleton();
    let tokens = bounded_text_chunks(text, 12_000)
        .into_iter()
        .flat_map(|chunk| bpe.encode_ordinary(chunk))
        .take(max_tokens)
        .collect::<Vec<_>>();
    let bytes = bpe
        .decode_bytes(&tokens[..tokens.len().min(max_tokens)])
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn split_text_to_token_chunks(text: &str, max_tokens: usize) -> Vec<String> {
    let bpe = o200k_base_singleton();
    let tokens = bounded_text_chunks(text, 12_000)
        .into_iter()
        .flat_map(|chunk| bpe.encode_ordinary(chunk))
        .collect::<Vec<_>>();
    tokens
        .chunks(max_tokens)
        .map(|chunk| {
            let bytes = bpe.decode_bytes(chunk).unwrap_or_default();
            String::from_utf8_lossy(&bytes).into_owned()
        })
        .collect()
}

fn bounded_text_chunks(text: &str, max_bytes: usize) -> Vec<&str> {
    if text.len() <= max_bytes {
        return vec![text];
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        let mut end = (start + max_bytes).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_are_token_based_and_match_the_context_contract() {
        assert_eq!(COMPRESSION_TRIGGER_TOKENS, 40_000);
        assert_eq!(COMPRESSION_TARGET_TOKENS, 30_000);
        assert_eq!(MAX_CONTEXT_TOKENS, 60_000);
        assert!(token_count_text("中文上下文") < "中文上下文".len());
    }

    #[test]
    fn compression_keeps_current_request_snapshot_and_latest_call_pair() {
        let request = "current";
        let input = vec![
            json!({"role":"system","content":[{"type":"input_text","text":"system"}]}),
            super::super::snapshot::render_snapshot_message(
                super::super::snapshot::STATE_SNAPSHOT_PREFIX,
            ),
            json!({"role":"user","content":[{"type":"input_text","text":"old history ".repeat(10_000)}]}),
            json!({"type":"function_call","call_id":"latest","name":"list_assets","arguments":"{}"}),
            json!({"type":"function_call_output","call_id":"latest","output":"receipt"}),
            json!({"role":"user","content":[{"type":"input_text","text":request}]}),
        ];

        let plan = compression_plan(&input, request);

        assert!(plan
            .older
            .iter()
            .any(|item| item.to_string().contains("old history")));
        assert!(plan
            .retained
            .iter()
            .any(super::super::snapshot::is_snapshot_message));
        assert!(plan.retained.iter().any(|item| item["call_id"] == "latest"));
        assert!(plan
            .retained
            .iter()
            .any(|item| is_current_user_item(item, request)));
    }
}
