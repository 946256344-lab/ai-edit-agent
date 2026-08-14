use crate::custom_api::{chat_endpoint, CustomApiConfig};
use crate::oauth::AuthorizedOAuth;
use serde_json::{json, Value};
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

/// The active model access: the experimental OpenAI OAuth Responses flow, or a
/// user-configured OpenAI-compatible custom API (base URL + API key).
pub(crate) enum ModelAccess {
    OAuth(AuthorizedOAuth),
    Custom(CustomApiConfig),
}

impl ModelAccess {
    pub(crate) fn resolve() -> Result<Self, String> {
        if let Some(access) = Self::configured_custom(crate::custom_api::custom_config())? {
            return Ok(access);
        }
        crate::oauth::experimental_access().map(ModelAccess::OAuth)
    }

    fn configured_custom(
        config: Result<Option<CustomApiConfig>, String>,
    ) -> Result<Option<Self>, String> {
        config.map(|config| config.map(ModelAccess::Custom))
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
                format!("Experimental Agent request failed with HTTP {status}.")
            }
            _ => "Experimental Agent request failed before receiving a response.".to_owned(),
        })?
        .into_string()
        .map_err(|_| "Experimental Agent response was empty.".to_owned())
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
    chat
}

fn response_input_messages(payload: &Value) -> Value {
    let Some(input) = payload.get("input").and_then(Value::as_array) else {
        return json!([]);
    };
    let messages: Vec<Value> = input
        .iter()
        .map(|message| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user");
            let content = message
                .get("content")
                .and_then(Value::as_array)
                .map(|items| {
                    let mapped: Vec<Value> = items
                        .iter()
                        .filter_map(|item| match item.get("type").and_then(Value::as_str) {
                            Some("input_text") => {
                                item.get("text").and_then(Value::as_str).map(|text| {
                                    json!({ "type": "text", "text": text })
                                })
                            }
                            Some("input_image") => item
                                .get("image_url")
                                .and_then(Value::as_str)
                                .map(|image_url| {
                                    json!({ "type": "image_url", "image_url": { "url": image_url } })
                                }),
                            _ => None,
                        })
                        .collect();
                    Value::Array(mapped)
                })
                .unwrap_or_else(|| json!([{ "type": "text", "text": "" }]));
            json!({
                "role": role,
                "content": content
            })
        })
        .collect();
    json!(messages)
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
                        format!("Custom API request failed with HTTP {status}.")
                    }
                    _ => "Custom API request failed before receiving a response.".to_owned(),
                })?
                .into_string()
                .map_err(|_| "Custom API response was empty.".to_owned())
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

    #[test]
    fn custom_credential_errors_block_provider_fallback() {
        let result = ModelAccess::configured_custom(Err(
            "Windows Credential Manager is unavailable.".to_owned(),
        ));
        assert!(result.is_err());
    }

    #[test]
    fn missing_custom_credentials_allow_provider_fallback() {
        let result = ModelAccess::configured_custom(Ok(None)).expect("read custom provider state");
        assert!(result.is_none());
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
