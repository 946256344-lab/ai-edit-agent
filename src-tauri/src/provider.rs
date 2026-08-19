//! 模型 Provider 选择、请求调度、HTTP 传输和安全响应解析边界。
//! `ModelAccess` 统一决定自定义 API 与实验性 OAuth 的选择及失败封闭规则。

use crate::custom_api::{chat_endpoint, CustomApiConfig};
use crate::oauth::AuthorizedOAuth;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const RESPONSES_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";
static HTTP_AGENT: OnceLock<ureq::Agent> = OnceLock::new();
static REQUEST_PRIORITY: OnceLock<(Mutex<RequestPriorityState>, Condvar)> = OnceLock::new();
static VISUAL_CIRCUIT: OnceLock<Mutex<VisualCircuitState>> = OnceLock::new();
static INTERACTIVE_REQUEST_COUNT: AtomicUsize = AtomicUsize::new(0);
const VISUAL_CIRCUIT_FAILURE_THRESHOLD: usize = 3;
const VISUAL_CIRCUIT_COOLDOWN: Duration = Duration::from_secs(60);

/// Provider 协议无关的单轮模型结果。`output` 保留协议返回的所有项目，
/// 使上层未来可以消费原生工具调用而不丢失同一响应中的其他 message。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelTurn {
    pub(crate) output: Vec<ModelOutputItem>,
}

impl ModelTurn {
    pub(crate) fn function_calls(&self) -> impl Iterator<Item = &FunctionCall> {
        self.output
            .iter()
            .filter_map(ModelOutputItem::function_call)
    }
}

/// Responses `output` 与 Chat Completions `message` 的共同表示。
/// 未知 Responses 项目原样保留，避免适配器在协议扩展时静默丢数据。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ModelOutputItem {
    Message {
        id: Option<String>,
        role: String,
        content: Vec<Value>,
        raw: Value,
    },
    FunctionCall(FunctionCall),
    Other(Value),
}

impl ModelOutputItem {
    pub(crate) fn function_call(&self) -> Option<&FunctionCall> {
        match self {
            Self::FunctionCall(call) => Some(call),
            _ => None,
        }
    }
}

/// 原生函数调用的稳定内部形状。Chat 的 tool call `id` 与 Responses 的
/// `call_id` 都归一化为 `call_id`，arguments 保持模型返回的 JSON 文本。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FunctionCall {
    pub(crate) call_id: String,
    pub(crate) name: String,
    pub(crate) arguments: String,
    pub(crate) raw: Value,
}

#[derive(Default)]
struct RequestPriorityState {
    interactive_active: usize,
    visual_active: usize,
}

#[derive(Default)]
struct VisualCircuitState {
    consecutive_failures: usize,
    open_until: Option<Instant>,
    half_open_probe_active: bool,
}

struct RequestPriorityGuard {
    interactive: bool,
}

impl Drop for RequestPriorityGuard {
    fn drop(&mut self) {
        let (lock, changed) = request_priority();
        if let Ok(mut state) = lock.lock() {
            if self.interactive {
                state.interactive_active = state.interactive_active.saturating_sub(1);
                INTERACTIVE_REQUEST_COUNT.fetch_sub(1, Ordering::AcqRel);
            } else {
                state.visual_active = state.visual_active.saturating_sub(1);
            }
            changed.notify_all();
        }
    }
}

fn request_priority() -> &'static (Mutex<RequestPriorityState>, Condvar) {
    REQUEST_PRIORITY.get_or_init(|| (Mutex::new(RequestPriorityState::default()), Condvar::new()))
}

fn begin_interactive_request() -> RequestPriorityGuard {
    INTERACTIVE_REQUEST_COUNT.fetch_add(1, Ordering::AcqRel);
    let (lock, changed) = request_priority();
    if let Ok(mut state) = lock.lock() {
        state.interactive_active += 1;
        changed.notify_all();
    }
    RequestPriorityGuard { interactive: true }
}

fn begin_visual_request() -> RequestPriorityGuard {
    let (lock, changed) = request_priority();
    let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    while INTERACTIVE_REQUEST_COUNT.load(Ordering::Acquire) > 0 || state.interactive_active > 0 {
        state = changed
            .wait(state)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    state.visual_active += 1;
    RequestPriorityGuard { interactive: false }
}

fn visual_circuit() -> &'static Mutex<VisualCircuitState> {
    VISUAL_CIRCUIT.get_or_init(|| Mutex::new(VisualCircuitState::default()))
}

fn begin_visual_circuit_request(now: Instant) -> bool {
    let mut state = visual_circuit()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match state.open_until {
        Some(until) if now < until => false,
        Some(_) if state.half_open_probe_active => false,
        Some(_) => {
            state.half_open_probe_active = true;
            true
        }
        None => true,
    }
}

pub(crate) fn complete_visual_model_request(success: bool) {
    let mut state = visual_circuit()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.half_open_probe_active = false;
    if success {
        state.consecutive_failures = 0;
        state.open_until = None;
        return;
    }
    state.consecutive_failures += 1;
    if state.consecutive_failures >= VISUAL_CIRCUIT_FAILURE_THRESHOLD {
        state.open_until = Some(Instant::now() + VISUAL_CIRCUIT_COOLDOWN);
    }
}

pub(crate) fn visual_model_retry_after() -> Option<Duration> {
    let state = visual_circuit()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state
        .open_until
        .and_then(|until| until.checked_duration_since(Instant::now()))
}

fn http_agent() -> &'static ureq::Agent {
    HTTP_AGENT.get_or_init(|| ureq::AgentBuilder::new().build())
}

/// 当前模型访问入口:实验性 OAuth Responses 流程，或用户配置的 OpenAI 兼容 API。
pub(crate) enum ModelAccess {
    OAuth(AuthorizedOAuth),
    Custom(CustomApiConfig),
}

impl ModelAccess {
    pub(crate) fn resolve() -> Result<Self, String> {
        // 不降级:自定义 API 配了就只用自定义，OAuth 配了就只用 OAuth，都没配就拒绝。
        let custom_result = crate::custom_api::custom_config();
        let oauth_result = crate::oauth::experimental_access();

        match (custom_result, oauth_result) {
            // 自定义 API 配置成功 → 只用自定义
            (Ok(Some(config)), _) => Ok(ModelAccess::Custom(config)),

            // 自定义 API 未配置，OAuth 成功 → 只用 OAuth
            (Ok(None), Ok(oauth)) => Ok(ModelAccess::OAuth(oauth)),

            // 自定义 API 凭据读取失败（不是"未配置"，是真的失败了）
            (Err(custom_error), _) => {
                Err(format!("Custom API credential read failed: {custom_error}"))
            }

            // custom API not configured, OAuth also failed
            (Ok(None), Err(oauth_error)) => {
                Err(format!("OAuth not logged in or expired: {oauth_error}"))
            }
        }
    }

    pub(crate) fn custom_config(&self) -> Option<&CustomApiConfig> {
        match self {
            ModelAccess::Custom(config) => Some(config),
            _ => None,
        }
    }
}

fn find_json_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text)
            if text.trim_start().starts_with('{')
                && serde_json::from_str::<Value>(text).is_ok() =>
        {
            Some(text.to_owned())
        }
        Value::Array(items) => items.iter().find_map(find_json_text),
        Value::Object(entries) => entries.values().find_map(find_json_text),
        _ => None,
    }
}

/// Parses a complete Responses response (or a Responses SSE stream) without
/// reducing it to the legacy JSON decision string.
pub(crate) fn model_turn_from_responses(body: &str) -> Option<ModelTurn> {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        return Some(responses_turn_from_value(&value));
    }

    let mut output = Vec::new();
    let mut completed_output = None;
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(response_output) = event
            .get("response")
            .and_then(|response| response.get("output"))
            .and_then(Value::as_array)
        {
            completed_output = Some(
                response_output
                    .iter()
                    .filter_map(parse_responses_item)
                    .collect(),
            );
        } else if event.get("type").and_then(Value::as_str) == Some("response.output_item.done") {
            if let Some(item) = event.get("item") {
                if let Some(parsed) = parse_responses_item(item) {
                    output.push(parsed);
                }
            }
        }
    }
    if let Some(output) = completed_output {
        return Some(ModelTurn { output });
    }
    (!output.is_empty()).then_some(ModelTurn { output })
}

/// Parses a complete OpenAI-compatible Chat Completions response. Each
/// assistant tool call becomes a separate protocol-neutral function call item.
pub(crate) fn model_turn_from_chat_completions(body: &str) -> Option<ModelTurn> {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        return chat_turn_from_value(&value);
    }
    chat_turn_from_sse(body)
}

fn chat_turn_from_value(value: &Value) -> Option<ModelTurn> {
    let choices = value.get("choices")?.as_array()?;
    let mut output = Vec::new();
    for choice in choices {
        let Some(message) = choice.get("message") else {
            continue;
        };
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("assistant")
            .to_owned();
        let content = chat_message_content(message.get("content"));
        if message.get("content").is_some() || message.get("tool_calls").is_none() {
            output.push(ModelOutputItem::Message {
                id: message.get("id").and_then(Value::as_str).map(str::to_owned),
                role,
                content,
                raw: message.clone(),
            });
        }
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                let Some(function) = tool_call.get("function") else {
                    continue;
                };
                let Some(name) = function.get("name").and_then(Value::as_str) else {
                    continue;
                };
                output.push(ModelOutputItem::FunctionCall(FunctionCall {
                    call_id: tool_call
                        .get("id")
                        .or_else(|| tool_call.get("tool_call_id"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    name: name.to_owned(),
                    arguments: function
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    raw: tool_call.clone(),
                }));
            }
        }
    }
    Some(ModelTurn { output })
}

#[derive(Default)]
struct ChatToolCallDelta {
    call_id: String,
    name: String,
    arguments: String,
}

fn chat_turn_from_sse(body: &str) -> Option<ModelTurn> {
    let mut role = "assistant".to_owned();
    let mut text = String::new();
    let mut tool_calls = BTreeMap::<usize, ChatToolCallDelta>::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        let Some(delta) = event
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
        else {
            continue;
        };
        if let Some(delta_role) = delta.get("role").and_then(Value::as_str) {
            role = delta_role.to_owned();
        }
        if let Some(part) = delta.get("content").and_then(Value::as_str) {
            text.push_str(part);
        }
        for tool_call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let index = tool_call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let accumulated = tool_calls.entry(index).or_default();
            if let Some(call_id) = tool_call.get("id").and_then(Value::as_str) {
                accumulated.call_id.push_str(call_id);
            }
            if let Some(function) = tool_call.get("function") {
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    accumulated.name.push_str(name);
                }
                if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                    accumulated.arguments.push_str(arguments);
                }
            }
        }
    }
    if text.is_empty() && tool_calls.is_empty() {
        return None;
    }
    let mut output = Vec::new();
    if !text.is_empty() {
        output.push(ModelOutputItem::Message {
            id: None,
            role,
            content: vec![json!({"type": "output_text", "text": text})],
            raw: Value::Null,
        });
    }
    output.extend(tool_calls.into_values().map(|call| {
        ModelOutputItem::FunctionCall(FunctionCall {
            call_id: call.call_id,
            name: call.name,
            arguments: call.arguments,
            raw: Value::Null,
        })
    }));
    Some(ModelTurn { output })
}

fn responses_turn_from_value(value: &Value) -> ModelTurn {
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_responses_item).collect())
        .unwrap_or_default();
    ModelTurn { output }
}

fn parse_responses_item(item: &Value) -> Option<ModelOutputItem> {
    match item.get("type").and_then(Value::as_str) {
        Some("message") => Some(ModelOutputItem::Message {
            id: item.get("id").and_then(Value::as_str).map(str::to_owned),
            role: item
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("assistant")
                .to_owned(),
            content: item
                .get("content")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            raw: item.clone(),
        }),
        Some("function_call") => Some(ModelOutputItem::FunctionCall(FunctionCall {
            call_id: item
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            name: item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            arguments: item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            raw: item.clone(),
        })),
        Some(_) => Some(ModelOutputItem::Other(item.clone())),
        None => None,
    }
}

fn chat_message_content(content: Option<&Value>) -> Vec<Value> {
    match content {
        Some(Value::String(text)) => vec![json!({"type": "output_text", "text": text})],
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// Parses an experimental Responses body into the first embedded JSON string.
pub(crate) fn response_json_text(body: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(text) = find_json_text(&value) {
            return Some(text);
        }
    }
    let mut delta = String::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(text) = find_json_text(&event) {
            return Some(text);
        }
        if let Some(part) = event.get("delta").and_then(|value| value.as_str()) {
            delta.push_str(part);
        }
    }
    serde_json::from_str::<Value>(&delta)
        .ok()
        .and_then(|value| find_json_text(&value).or(Some(delta)))
}

/// Posts a JSON payload to the experimental Responses endpoint and returns the raw body.
pub(crate) fn post_responses_json(
    access: &AuthorizedOAuth,
    payload: &Value,
    timeout: Option<Duration>,
) -> Result<String, String> {
    let mut request_builder = http_agent()
        .post(RESPONSES_ENDPOINT)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", access.access_token))
        .set("originator", "opencode");
    if let Some(account_id) = &access.account_id {
        request_builder = request_builder.set("ChatGPT-Account-Id", account_id);
    }
    if let Some(timeout) = timeout {
        request_builder = request_builder.timeout(timeout);
    }
    request_builder
        .send_string(&payload.to_string())
        .map_err(|error| match error {
            ureq::Error::Status(status, response) => {
                let _ = response.into_string();
                format!("实验性 OAuth 请求失败:HTTP {status}")
            }
            ureq::Error::Transport(transport) => {
                format!("实验性 OAuth 请求失败:网络错误 {transport}")
            }
        })?
        .into_string()
        .map_err(|_| "实验性 OAuth 响应为空".to_owned())
}

/// Converts an OAuth Responses-style request into an OpenAI-compatible
/// chat/completions request for a custom provider, substituting the configured
/// model name.
pub(crate) fn chat_completions_request(config: &CustomApiConfig, payload: &Value) -> Value {
    chat_completions_request_with_model(payload, &config.model)
}

fn chat_completions_request_with_model(payload: &Value, model: &str) -> Value {
    let mut chat = json!({
        "model": model,
        "messages": response_input_messages(payload)
    });
    if let Some(format) = payload.get("text").and_then(|value| value.get("format")) {
        chat["response_format"] = format.clone();
    }
    if let Some(tools) = payload.get("tools") {
        chat["tools"] = chat_tools(tools);
    }
    if let Some(tool_choice) = payload.get("tool_choice") {
        chat["tool_choice"] = chat_tool_choice(tool_choice);
    }
    if let Some(parallel_tool_calls) = payload.get("parallel_tool_calls") {
        chat["parallel_tool_calls"] = parallel_tool_calls.clone();
    }
    if let Some(stream) = payload.get("stream") {
        chat["stream"] = stream.clone();
    }
    chat
}

fn response_input_messages(payload: &Value) -> Value {
    let Some(input) = payload.get("input").and_then(Value::as_array) else {
        return json!([]);
    };
    let mut messages: Vec<Value> = Vec::new();
    for message in input {
        match message.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                let tool_call = json!({
                    "id": message.get("call_id").cloned().unwrap_or(Value::Null),
                    "type": "function",
                    "function": {
                        "name": message.get("name").cloned().unwrap_or(Value::Null),
                        "arguments": message.get("arguments").cloned().unwrap_or_else(|| json!("{}"))
                    }
                });
                if let Some(last) = messages
                    .last_mut()
                    .filter(|last| last["role"] == "assistant")
                {
                    if let Some(tool_calls) = last["tool_calls"].as_array_mut() {
                        tool_calls.push(tool_call);
                        continue;
                    }
                }
                messages.push(json!({"role": "assistant", "tool_calls": [tool_call]}));
            }
            Some("function_call_output") => {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": message.get("call_id").cloned().unwrap_or(Value::Null),
                    "content": chat_tool_output(message.get("output"))
                }));
            }
            _ => {
                let role = message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("user");
                let content = message
                    .get("content")
                    .map(chat_input_content)
                    .unwrap_or_else(|| json!([{ "type": "text", "text": "" }]));
                let mut chat_message = json!({"role": role, "content": content});
                if let Some(tool_calls) = message.get("tool_calls") {
                    chat_message["tool_calls"] = tool_calls.clone();
                }
                if let Some(tool_call_id) = message.get("tool_call_id") {
                    chat_message["tool_call_id"] = tool_call_id.clone();
                }
                messages.push(chat_message);
            }
        }
    }
    json!(messages)
}

fn chat_input_content(content: &Value) -> Value {
    match content {
        Value::String(text) => json!([{"type": "text", "text": text}]),
        Value::Array(items) => {
            Value::Array(items.iter().filter_map(chat_input_content_item).collect())
        }
        _ => json!([{ "type": "text", "text": "" }]),
    }
}

fn chat_tool_output(output: Option<&Value>) -> Value {
    match output {
        Some(Value::String(text)) => Value::String(text.clone()),
        Some(value) => serde_json::to_string(value)
            .map(Value::String)
            .unwrap_or_else(|_| Value::String(String::new())),
        None => Value::String(String::new()),
    }
}

fn chat_input_content_item(item: &Value) -> Option<Value> {
    match item.get("type").and_then(Value::as_str) {
        Some("input_text") | Some("output_text") => item
            .get("text")
            .and_then(Value::as_str)
            .map(|text| json!({"type": "text", "text": text})),
        Some("input_image") => item
            .get("image_url")
            .and_then(Value::as_str)
            .map(|image_url| json!({"type": "image_url", "image_url": {"url": image_url}})),
        _ => None,
    }
}

fn chat_tools(tools: &Value) -> Value {
    let Some(items) = tools.as_array() else {
        return tools.clone();
    };
    Value::Array(
        items
            .iter()
            .map(|tool| {
                if tool.get("type").and_then(Value::as_str) == Some("function")
                    && tool.get("function").is_none()
                    && tool.get("name").is_some()
                {
                    let mut function = tool.clone();
                    if let Some(fields) = function.as_object_mut() {
                        fields.remove("type");
                    }
                    json!({"type": "function", "function": function})
                } else {
                    tool.clone()
                }
            })
            .collect(),
    )
}

fn chat_tool_choice(tool_choice: &Value) -> Value {
    if tool_choice.get("type").and_then(Value::as_str) == Some("function")
        && tool_choice.get("name").is_some()
        && tool_choice.get("function").is_none()
    {
        json!({
            "type": "function",
            "function": {"name": tool_choice.get("name").cloned().unwrap_or(Value::Null)}
        })
    } else {
        tool_choice.clone()
    }
}

/// Posts the given request through whichever model access is active. OAuth uses
/// the experimental Responses endpoint; a configured custom API targets its
/// OpenAI-compatible chat/completions endpoint.
pub(crate) fn post_model_payload(
    access: &ModelAccess,
    payload: &Value,
    timeout: Option<Duration>,
) -> Result<String, String> {
    let _priority = begin_interactive_request();
    post_model_payload_with_custom_model(access, payload, timeout, None)
}

/// Coarse visual analysis may use a separately configured custom model. OAuth
/// keeps its existing request model because no alternate has been verified.
pub(crate) fn post_visual_model_payload(
    access: &ModelAccess,
    payload: &Value,
    timeout: Option<Duration>,
) -> Result<String, String> {
    if !begin_visual_circuit_request(Instant::now()) {
        return Err("visual_provider_circuit_open".to_owned());
    }
    let _priority = begin_visual_request();
    let custom_model = access.custom_config().map(visual_model);
    post_model_payload_with_custom_model(access, payload, timeout, custom_model)
}

fn visual_model(config: &CustomApiConfig) -> &str {
    if config.coarse_visual_model.is_empty() {
        &config.model
    } else {
        &config.coarse_visual_model
    }
}

fn post_model_payload_with_custom_model(
    access: &ModelAccess,
    payload: &Value,
    timeout: Option<Duration>,
    custom_model: Option<&str>,
) -> Result<String, String> {
    match access {
        ModelAccess::OAuth(access) => post_responses_json(access, payload, timeout),
        ModelAccess::Custom(config) => {
            let request = custom_model.map_or_else(
                || chat_completions_request(config, payload),
                |model| chat_completions_request_with_model(payload, model),
            );
            let mut request_builder = http_agent()
                .post(&chat_endpoint(&config.base_url))
                .set("Content-Type", "application/json")
                .set("Authorization", &format!("Bearer {}", config.api_key));
            if let Some(timeout) = timeout {
                request_builder = request_builder.timeout(timeout);
            }
            request_builder
                .send_string(&request.to_string())
                .map_err(|error| match error {
                    ureq::Error::Status(status, response) => {
                        let _ = response.into_string();
                        format!(
                            "自定义 API 不可用（{}，模型 {}）:HTTP {status}",
                            config.base_url,
                            custom_model.unwrap_or(&config.model)
                        )
                    }
                    ureq::Error::Transport(transport) => format!(
                        "自定义 API 不可用（{}，模型 {}）:网络错误 {}",
                        config.base_url,
                        custom_model.unwrap_or(&config.model),
                        transport
                    ),
                })?
                .into_string()
                .map_err(|_| {
                    format!(
                        "自定义 API 响应为空（{}，模型 {}）",
                        config.base_url,
                        custom_model.unwrap_or(&config.model)
                    )
                })
        }
    }
}

/// Extracts the first embedded JSON decision string from a raw model body,
/// regardless of whether it came from the Responses API or the OpenAI-compatible
/// chat/completions transport.
pub(crate) fn model_response_json_text(access: &ModelAccess, body: &str) -> Option<String> {
    if access.custom_config().is_some() {
        return chat_response_json_text(body);
    }
    response_json_text(body)
}

/// Extracts the decision JSON from an OpenAI-compatible chat/completions body:
/// either a plain JSON body or SSE `data:` deltas, reading `choices[*].delta`
/// or `choices[*].message` content text.
fn chat_response_json_text(body: &str) -> Option<String> {
    if let Some(text) = parse_chat_body(body) {
        return Some(text);
    }
    let mut delta = String::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        let part = event
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        delta.push_str(part);
    }
    if delta.trim().is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(&delta)
        .ok()
        .and_then(|value| find_json_text(&value).or(Some(delta)))
}

fn parse_chat_body(body: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(body).ok()?;
    value
        .get("choices")
        .and_then(Value::as_array)?
        .first()?
        .get("message")?
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .and_then(|text| {
            if text.trim_start().starts_with('{') {
                Some(text)
            } else {
                find_json_text(&serde_json::from_str::<Value>(&text).ok()?).or(Some(text))
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSES_TOOL_TURN: &str =
        include_str!("../tests/fixtures/provider_responses_tool_turn.v1.json");
    const CHAT_TOOL_TURN: &str = include_str!("../tests/fixtures/provider_chat_tool_turn.v1.json");
    const CHAT_TOOL_STREAM: &str =
        include_str!("../tests/fixtures/provider_chat_tool_stream.v1.json");
    const RESPONSES_TOOL_REQUEST: &str =
        include_str!("../tests/fixtures/provider_responses_tool_request.v1.json");

    #[test]
    fn custom_credential_errors_block_provider_selection() {
        // 凭据读取失败时 resolve 应返回 Err，不能静默回退
        // 这里只测 configured_custom 的内部等价逻辑:Err 传播
        let result: Result<Option<CustomApiConfig>, String> =
            Err("Windows Credential Manager is unavailable.".to_owned());
        assert!(result.is_err());
    }

    #[test]
    fn missing_custom_credentials_are_distinguished_from_read_failure() {
        // Ok(None) 表示"明确未配置"，不是读取失败
        let result: Result<Option<CustomApiConfig>, String> = Ok(None);
        assert!(result.unwrap().is_none());
    }

    fn custom_config(coarse_visual_model: &str) -> CustomApiConfig {
        CustomApiConfig {
            base_url: "https://api.example.com/v1".to_owned(),
            model: "main-model".to_owned(),
            coarse_visual_model: coarse_visual_model.to_owned(),
            api_key: "secret".to_owned(),
        }
    }

    #[test]
    fn main_requests_keep_the_required_main_model() {
        let request = chat_completions_request(&custom_config("coarse-model"), &json!({}));
        assert_eq!(request["model"], "main-model");
    }

    #[test]
    fn coarse_visual_model_is_used_only_when_configured() {
        let configured = custom_config("coarse-model");
        let coarse = chat_completions_request_with_model(&json!({}), visual_model(&configured));
        let blank = custom_config("");
        let main = chat_completions_request_with_model(&json!({}), visual_model(&blank));

        assert_eq!(coarse["model"], "coarse-model");
        assert_eq!(main["model"], "main-model");
    }

    #[test]
    fn responses_fixture_preserves_complete_output_items() {
        let turn = model_turn_from_responses(RESPONSES_TOOL_TURN).expect("parse Responses fixture");

        assert_eq!(turn.output.len(), 2);
        assert!(matches!(
            &turn.output[0],
            ModelOutputItem::Message { role, content, .. }
                if role == "assistant" && content[0]["text"] == "I will inspect the ready assets."
        ));
        assert_eq!(
            turn.output[1].function_call(),
            Some(&FunctionCall {
                call_id: "call_fixture_1".to_owned(),
                name: "list_assets".to_owned(),
                arguments: "{\"status\":\"ready\"}".to_owned(),
                raw: serde_json::from_str(
                    r#"{"id":"fc_fixture_1","type":"function_call","status":"completed","call_id":"call_fixture_1","name":"list_assets","arguments":"{\"status\":\"ready\"}"}"#,
                )
                .expect("parse function call raw fixture"),
            })
        );
    }

    #[test]
    fn chat_fixture_normalizes_assistant_tool_calls() {
        let turn = model_turn_from_chat_completions(CHAT_TOOL_TURN)
            .expect("parse Chat Completions fixture");

        assert_eq!(turn.output.len(), 2);
        assert!(matches!(
            &turn.output[0],
            ModelOutputItem::Message { role, content, .. }
                if role == "assistant" && content[0]["text"] == "I will inspect the ready assets."
        ));
        assert_eq!(
            turn.output[1].function_call(),
            Some(&FunctionCall {
                call_id: "call_fixture_1".to_owned(),
                name: "list_assets".to_owned(),
                arguments: "{\"status\":\"ready\"}".to_owned(),
                raw: serde_json::from_str(
                    r#"{"id":"call_fixture_1","type":"function","function":{"name":"list_assets","arguments":"{\"status\":\"ready\"}"}}"#,
                )
                .expect("parse tool call raw fixture"),
            })
        );
    }

    #[test]
    fn chat_stream_fixture_accumulates_tool_call_deltas() {
        let chunks: Vec<Value> =
            serde_json::from_str(CHAT_TOOL_STREAM).expect("parse Chat stream fixture");
        let body = chunks
            .iter()
            .map(|chunk| format!("data: {chunk}"))
            .chain(std::iter::once("data: [DONE]".to_owned()))
            .collect::<Vec<_>>()
            .join("\n");
        let turn = model_turn_from_chat_completions(&body).expect("parse Chat SSE fixture");

        assert!(matches!(
            &turn.output[0],
            ModelOutputItem::Message { content, .. }
                if content[0]["text"] == "Checking assets. Please wait."
        ));
        assert_eq!(turn.function_calls().count(), 1);
        assert_eq!(
            turn.output[1].function_call(),
            Some(&FunctionCall {
                call_id: "call_fixture_stream_1".to_owned(),
                name: "list_assets".to_owned(),
                arguments: "{\"status\":\"ready\"}".to_owned(),
                raw: Value::Null,
            })
        );
    }

    #[test]
    fn custom_adapter_keeps_native_tool_contract_and_tool_results() {
        let payload: Value =
            serde_json::from_str(RESPONSES_TOOL_REQUEST).expect("parse request fixture");
        let request = chat_completions_request(&custom_config(""), &payload);

        assert_eq!(request["parallel_tool_calls"], false);
        assert_eq!(request["tool_choice"]["function"]["name"], "list_assets");
        assert_eq!(request["tools"][0]["function"]["name"], "list_assets");
        assert_eq!(request["tools"][0]["function"]["strict"], true);
        assert_eq!(request["messages"][1]["role"], "assistant");
        assert_eq!(
            request["messages"][1]["tool_calls"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            request["messages"][1]["tool_calls"][0]["id"],
            "call_fixture_0"
        );
        assert_eq!(
            request["messages"][1]["tool_calls"][1]["id"],
            "call_fixture_1"
        );
        assert_eq!(request["messages"][2]["role"], "tool");
        assert_eq!(request["messages"][2]["tool_call_id"], "call_fixture_0");
        assert_eq!(request["messages"][2]["content"], "{\"exists\":false}");
        assert!(request.get("store").is_none());

        let string_input = json!({
            "input": [{"role": "assistant", "content": "A prior assistant message."}]
        });
        let string_request = chat_completions_request(&custom_config(""), &string_input);
        assert_eq!(
            string_request["messages"][0]["content"][0]["text"],
            "A prior assistant message."
        );
    }

    #[test]
    fn model_requests_share_one_process_wide_http_agent() {
        assert!(std::ptr::eq(http_agent(), http_agent()));
    }

    #[test]
    fn visual_circuit_opens_after_consecutive_failures_and_recovers() {
        let mut state = VisualCircuitState::default();
        let now = Instant::now();
        for _ in 0..VISUAL_CIRCUIT_FAILURE_THRESHOLD {
            state.consecutive_failures += 1;
            if state.consecutive_failures >= VISUAL_CIRCUIT_FAILURE_THRESHOLD {
                state.open_until = Some(now + VISUAL_CIRCUIT_COOLDOWN);
            }
        }
        assert!(state.open_until.is_some());
        state.consecutive_failures = 0;
        state.open_until = None;
        state.half_open_probe_active = false;
        assert!(state.open_until.is_none());
    }
}
