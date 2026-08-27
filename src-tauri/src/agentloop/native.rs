//! 原生 Function Tool Agent Loop。
//!
//! 所有对话请求统一由本模块消费 Provider 的 `ModelTurn`；Responses 的完整 output
//! item 或 Chat 适配后的等价项目会进入下一轮，工具副作用仍只由 `skills::apply_skill` 执行。

use crate::audit::{
    begin_agent_run_step, finish_agent_run_step, record_agent_diagnostic,
    record_agent_timing_diagnostic, AgentTimingMetric,
};
use crate::models::{AgentEditResult, StoryboardVersion, TimelineVersion};
use crate::provider::{
    chat_completions_request, classify_model_request_failure, model_turn_from_chat_completions,
    model_turn_from_responses, post_model_payload_with_wire_observer, FunctionCall, ModelAccess,
    ModelOutputItem,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use super::context::{
    compact_tool_outputs, compression_plan, memory_item, provider_payload_tokens, summary_batches,
    truncate_text_to_tokens, COMPRESSION_TARGET_TOKENS, COMPRESSION_TRIGGER_TOKENS,
    MAX_CONTEXT_TOKENS,
};
use super::continuation::ContinuationState;
use super::policy::{request_requires_project_observation, RequestToolPolicy, OBSERVATION_TOOLS};
use super::schema::{
    AgentLoopResult, AgentLoopTerminalStatus, LoopState, AGENT_RUN_TIMEOUT, AGENT_STEP_TIMEOUT,
    MAX_STEPS,
};
use super::skills::{
    apply_skill, persisted_artifact_for_tool, safe_step_error_code, safe_tool_failure_context,
};
use super::snapshot::{build_state_snapshot, is_snapshot_message, render_snapshot_message};
use super::tools::{
    compact_tool_directory, load_tools_function_tool, native_function_tools_for_request,
    LOAD_TOOLS, READ_LOGS,
};

const NATIVE_TOOL_NAMES: &[&str] = &[
    "load_tools",
    "read_logs",
    "get_edit_status",
    "get_asset_health_summary",
    "list_assets",
    "search_assets",
    "search_asset_segments",
    "search_music",
    "list_voices",
    "get_storyboard",
    "get_timeline",
    "get_text_capabilities",
    "render_preview",
    "request_asset_analysis",
    "generate_storyboard",
    "create_timeline_draft",
    "replace_clips",
    "change_clip_duration",
    "reorder_clips",
    "replace_text_tracks",
    "download_music",
    "use_online_music",
    "replace_music_tracks",
    "synthesize_voiceover",
    "create_jianying_draft",
];

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_native_tool_loop(
    app: &AppHandle,
    connection: &Connection,
    agent_task_id: &str,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    request: &str,
    task_brief: &str,
    access: &ModelAccess,
    storyboard: Option<&StoryboardVersion>,
    timelines: &[TimelineVersion],
) -> Result<AgentLoopResult, String> {
    let run_started_at = Instant::now();
    let run_deadline = run_started_at + AGENT_RUN_TIMEOUT;
    let history = super::prompt::load_native_message_history(
        connection,
        conversation_id,
        editing_task_id,
        request,
    );
    let tool_policy = RequestToolPolicy::from_request(request);
    let initial_catalog = full_native_tool_catalog();
    let tool_directory = compact_tool_directory(&initial_catalog);
    let mut input = initial_native_input(
        history,
        request,
        &tool_policy,
        &tool_directory,
        build_state_snapshot(connection, project_id, editing_task_id),
    )?;

    let mut state = LoopState {
        app,
        connection,
        agent_task_id,
        project_id,
        editing_task_id,
        conversation_id,
        task_brief: task_brief.to_owned(),
        tool_policy: tool_policy.clone(),
        loaded_tools: std::collections::BTreeSet::new(),
        storyboard: storyboard.cloned(),
        timelines: timelines.to_vec(),
        last_outcome: None,
        last_failed_tool_error_code: None,
        successful_observation: false,
    };
    let is_custom = access.custom_config().is_some();
    let mut model_step_number = 0usize;
    let mut respond = |payload: &Value, timeout: Duration| {
        model_step_number += 1;
        let trace_enabled = super::trace::native_provider_full_trace_enabled();
        let trace_adapter = if access.custom_config().is_some() {
            "chat_completions"
        } else {
            "responses"
        };
        let trace_request = trace_enabled.then(|| {
            access.custom_config().map_or_else(
                || payload.to_string(),
                |config| chat_completions_request(config, payload).to_string(),
            )
        });
        let mut attempt_number = 0usize;
        let mut respond_once = |payload: &Value, attempt_timeout: Duration| {
            attempt_number += 1;
            let current_attempt = attempt_number;
            if let Some(trace_request) = &trace_request {
                super::trace::emit_native_provider_request(
                    model_step_number,
                    current_attempt,
                    trace_adapter,
                    trace_request,
                );
            }
            post_model_payload_with_wire_observer(
                access,
                payload,
                Some(attempt_timeout),
                &mut |status, body| {
                    if trace_enabled {
                        super::trace::emit_native_provider_response(
                            model_step_number,
                            current_attempt,
                            trace_adapter,
                            status,
                            body,
                        );
                    }
                },
            )
        };
        let mut request_cancelled = || native_task_cancelled(connection, agent_task_id);
        request_native_model_with_retry(
            payload,
            timeout,
            NATIVE_MODEL_RETRY_DELAY,
            &mut respond_once,
            &mut request_cancelled,
            &mut |observation| {
                let (kind, content) = native_model_request_diagnostic(observation);
                let _ = record_agent_diagnostic(
                    connection,
                    project_id,
                    editing_task_id,
                    conversation_id,
                    agent_task_id,
                    Some(model_step_number as i64),
                    kind,
                    &content,
                );
            },
        )
    };
    let mut invalid_call_guard = InvalidCallGuard::default();
    let mut execute = |call: &FunctionCall, step_number: usize| -> Result<Value, String> {
        match invalid_call_guard.before_call(call) {
            InvalidCallDecision::Execute => {
                let result = execute_native_tool(
                    &mut state,
                    call,
                    step_number,
                    native_render_preview_allowed(request, &tool_policy),
                )?;
                invalid_call_guard.record_result(call, &result);
                Ok(result)
            }
            InvalidCallDecision::Reject => {
                record_repeated_invalid_call(&state, call, step_number)?;
                Ok(repeated_invalid_arguments(&call.name))
            }
            InvalidCallDecision::Stop => {
                record_repeated_invalid_call(&state, call, step_number)?;
                Err("native_tool_loop_repeated_invalid_call".to_owned())
            }
        }
    };
    let mut refresh_snapshot = || {
        build_state_snapshot(connection, project_id, editing_task_id)
            .map(|snapshot| Some(render_snapshot_message(&snapshot)))
            .map_err(|_| "native_state_snapshot_refresh_failed".to_owned())
    };
    let cancelled = || native_task_cancelled(connection, agent_task_id);
    let mut receipt = NativeRunReceipt {
        requires_project_observation: request_requires_project_observation(request),
        successful_observation_this_turn: true,
        ..NativeRunReceipt::default()
    };
    receipt
        .observation_sources
        .insert("state_snapshot".to_owned());
    let loop_result = drive_native_loop(
        &mut input,
        is_custom,
        request_requires_project_observation(request),
        &tool_policy,
        &mut receipt,
        request,
        run_deadline,
        &mut respond,
        &mut execute,
        &mut refresh_snapshot,
        cancelled,
        |body, step_number| {
            let _ = record_agent_diagnostic(
                connection,
                project_id,
                editing_task_id,
                conversation_id,
                agent_task_id,
                Some(step_number as i64),
                "model_response",
                &format!("native_response_bytes={}", body.len()),
            );
        },
    );
    drop(execute);
    let _ = record_agent_timing_diagnostic(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        None,
        AgentTimingMetric::RunTotal,
        run_started_at.elapsed(),
    );
    let (result, status) = finish_native_result(
        agent_task_id,
        loop_result,
        state.last_outcome.take(),
        &receipt,
    )?;
    Ok(AgentLoopResult {
        result,
        status,
        clarification_goal: receipt.needs_confirmation.then_some("storyboard"),
    })
}

fn initial_native_input(
    history: Vec<Value>,
    request: &str,
    tool_policy: &RequestToolPolicy,
    tool_directory: &str,
    snapshot: Result<String, String>,
) -> Result<Vec<Value>, String> {
    let snapshot = snapshot.map_err(|_| "native_state_snapshot_unavailable".to_owned())?;
    let mut input = vec![
        json!({
            "role": "system",
            "content": [{
                "type": "input_text",
                "text": native_system_prompt(tool_policy, tool_directory)
            }]
        }),
        render_snapshot_message(&snapshot),
    ];
    input.extend(history);
    input.push(json!({
        "role": "user",
        "content": [{"type": "input_text", "text": request}]
    }));
    Ok(input)
}

fn finish_native_result(
    agent_task_id: &str,
    loop_result: Result<String, String>,
    last_outcome: Option<AgentEditResult>,
    receipt: &NativeRunReceipt,
) -> Result<(AgentEditResult, AgentLoopTerminalStatus), String> {
    match loop_result {
        Ok(message) => {
            let status = if receipt.needs_confirmation {
                AgentLoopTerminalStatus::NeedsClarification
            } else if !receipt.failed_tools.is_empty() && receipt.successful_tool_call {
                AgentLoopTerminalStatus::PartiallyCompleted
            } else if !receipt.failed_tools.is_empty() {
                AgentLoopTerminalStatus::Failed
            } else if !receipt.pending_tools.is_empty() {
                AgentLoopTerminalStatus::PartiallyCompleted
            } else if !receipt.unverified_requested_write_tools.is_empty()
                && receipt.successful_write_tools.is_empty()
            {
                AgentLoopTerminalStatus::Failed
            } else if !receipt.unverified_requested_write_tools.is_empty() {
                AgentLoopTerminalStatus::PartiallyCompleted
            } else {
                AgentLoopTerminalStatus::Completed
            };
            Ok((
                native_result_from_message(agent_task_id, message, last_outcome),
                status,
            ))
        }
        Err(error) => Ok(interrupted_native_result(
            agent_task_id,
            &error,
            last_outcome,
            receipt,
        )),
    }
}

fn interrupted_native_result(
    agent_task_id: &str,
    error: &str,
    last_outcome: Option<AgentEditResult>,
    receipt: &NativeRunReceipt,
) -> (AgentEditResult, AgentLoopTerminalStatus) {
    let bounded_reason = match error {
        "native_tool_loop_deadline_exceeded" => Some("本轮达到总超时"),
        "native_tool_loop_max_steps" => Some("本轮达到步骤上限"),
        "native_tool_loop_repeated_invalid_call" => {
            Some("模型连续重复了相同的无效工具参数，循环已停止")
        }
        _ => None,
    };
    let Some(reason) = bounded_reason else {
        if let Some(mut outcome) = last_outcome {
            outcome.message = if outcome.preview.is_some() {
                "预览已由工具生成并验证，但模型未能完成结果说明；预览已保留。".to_owned()
            } else {
                native_model_reply_unavailable_result(agent_task_id, receipt).message
            };
            let status = if receipt.needs_confirmation {
                AgentLoopTerminalStatus::NeedsClarification
            } else {
                AgentLoopTerminalStatus::PartiallyCompleted
            };
            return (outcome, status);
        }
        return (
            native_model_reply_unavailable_result(agent_task_id, receipt),
            AgentLoopTerminalStatus::Failed,
        );
    };
    let message = if receipt.successful_tool_call {
        format!("{reason}；已由工具确认的部分结果已保留，未完成步骤没有标记为成功。")
    } else {
        format!("{reason}，没有工具确认任何完成结果。")
    };
    let mut result = last_outcome.unwrap_or_else(|| AgentEditResult {
        agent_task_id: agent_task_id.to_owned(),
        message: String::new(),
        storyboard: None,
        timeline: None,
        preview: None,
        jianying_draft: None,
    });
    result.message = message;
    let status = if receipt.needs_confirmation {
        AgentLoopTerminalStatus::NeedsClarification
    } else if receipt.successful_tool_call {
        AgentLoopTerminalStatus::PartiallyCompleted
    } else {
        AgentLoopTerminalStatus::Failed
    };
    (result, status)
}

/// Provider 在工具返回后的总结请求失败时，保留真实失败终态并给 UI 一个诚实、
/// 不含传输细节的恢复消息。不能把此类 Native 回合抛回 Legacy 的固定“受限操作”
/// 文案，因为它可能已完成只读观察，而未发生任何本地写入。
fn native_model_reply_unavailable_result(
    agent_task_id: &str,
    receipt: &NativeRunReceipt,
) -> AgentEditResult {
    let message = if receipt.successful_observation_this_turn {
        "项目数据已读取，但模型未能生成最终回复。请检查模型连接后重试；本轮没有创建或修改 storyboard、时间线或 preview。"
    } else if receipt.tool_called {
        "模型未能根据本轮工具结果生成最终回复。请检查模型连接后重试；本轮没有确认新的本地写入。"
    } else {
        "模型未能生成回复。请检查模型连接后重试；本轮没有创建或修改 storyboard、时间线或 preview。"
    };
    AgentEditResult {
        agent_task_id: agent_task_id.to_owned(),
        message: message.to_owned(),
        storyboard: None,
        timeline: None,
        preview: None,
        jianying_draft: None,
    }
}

fn native_result_from_message(
    agent_task_id: &str,
    message: String,
    last_outcome: Option<AgentEditResult>,
) -> AgentEditResult {
    if let Some(mut outcome) = last_outcome {
        outcome.message = message;
        return outcome;
    }
    AgentEditResult {
        agent_task_id: agent_task_id.to_owned(),
        message,
        storyboard: None,
        timeline: None,
        preview: None,
        jianying_draft: None,
    }
}

fn merge_native_outcomes(
    previous: Option<AgentEditResult>,
    mut current: AgentEditResult,
    tool: &str,
) -> AgentEditResult {
    let Some(previous) = previous else {
        return current;
    };
    if tool == "generate_storyboard" {
        return current;
    }
    if current.storyboard.is_none() {
        current.storyboard = previous.storyboard;
    }
    if current.timeline.is_none() {
        current.timeline = previous.timeline;
    }
    if current.preview.is_none() {
        let invalidates_preview = matches!(
            tool,
            "create_timeline_draft"
                | "replace_clips"
                | "change_clip_duration"
                | "reorder_clips"
                | "replace_text_tracks"
                | "replace_music_tracks"
                | "use_online_music"
                | "synthesize_voiceover"
        );
        if !invalidates_preview {
            current.preview = previous.preview;
        }
    }
    if current.jianying_draft.is_none() {
        current.jianying_draft = previous.jianying_draft;
    }
    current
}

type NativeRespond<'a> = dyn FnMut(&Value, Duration) -> Result<String, String> + 'a;
type NativeExecute<'a> = dyn FnMut(&FunctionCall, usize) -> Result<Value, String> + 'a;
type NativeRefreshSnapshot<'a> = dyn FnMut() -> Result<Option<Value>, String> + 'a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvalidCallDecision {
    Execute,
    Reject,
    Stop,
}

#[derive(Debug, Default)]
struct InvalidCallGuard {
    attempts: std::collections::BTreeMap<String, usize>,
}

impl InvalidCallGuard {
    fn before_call(&mut self, call: &FunctionCall) -> InvalidCallDecision {
        let signature = native_call_signature(call);
        let Some(attempts) = self.attempts.get_mut(&signature) else {
            return InvalidCallDecision::Execute;
        };
        *attempts = attempts.saturating_add(1);
        if *attempts >= 3 {
            InvalidCallDecision::Stop
        } else {
            InvalidCallDecision::Reject
        }
    }

    fn record_result(&mut self, call: &FunctionCall, result: &Value) {
        if result["code"].as_str() == Some("invalid_arguments") {
            self.attempts.insert(native_call_signature(call), 1);
        }
    }
}

fn native_call_signature(call: &FunctionCall) -> String {
    let arguments = serde_json::from_str::<Value>(&call.arguments)
        .map(canonical_json_value)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| call.arguments.clone());
    format!("{}\0{arguments}", call.name)
}

fn canonical_json_value(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(canonical_json_value).collect()),
        Value::Object(object) => {
            let mut entries = object.into_iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            let mut canonical = serde_json::Map::new();
            for (key, value) in entries {
                canonical.insert(key, canonical_json_value(value));
            }
            Value::Object(canonical)
        }
        other => other,
    }
}

const NATIVE_MODEL_MAX_ATTEMPTS: usize = 3;
const NATIVE_MODEL_RETRY_DELAY: Duration = Duration::from_millis(350);
const NATIVE_MODEL_RETRY_CANCEL_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
enum NativeModelRequestObservation {
    RetryScheduled { code: String, attempt: usize },
    Recovered { code: String, attempts: usize },
    Failed { code: String, attempts: usize },
}

fn request_native_model_with_retry(
    payload: &Value,
    timeout: Duration,
    retry_delay: Duration,
    respond_once: &mut NativeRespond<'_>,
    cancelled: &mut dyn FnMut() -> bool,
    observe: &mut dyn FnMut(&NativeModelRequestObservation),
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let mut last_failure_code = None;
    for attempt in 1..=NATIVE_MODEL_MAX_ATTEMPTS {
        if cancelled() {
            return Err("native_tool_loop_cancelled".to_owned());
        }
        let Some(remaining) = remaining_timeout(deadline) else {
            return Err("native_tool_loop_deadline_exceeded".to_owned());
        };
        let attempt_timeout = native_model_attempt_timeout(remaining, attempt);
        let response = match respond_once(payload, attempt_timeout) {
            Ok(body) if body.trim().is_empty() => Err("Provider response was empty.".to_owned()),
            other => other,
        };
        match response {
            Ok(body) => {
                if let Some(code) = last_failure_code {
                    observe(&NativeModelRequestObservation::Recovered {
                        code,
                        attempts: attempt,
                    });
                }
                return Ok(body);
            }
            Err(error) => {
                let failure = classify_model_request_failure(&error);
                let delay = retry_delay.saturating_mul(attempt as u32);
                let can_retry = failure.retryable
                    && attempt < NATIVE_MODEL_MAX_ATTEMPTS
                    && remaining_timeout(deadline).is_some_and(|remaining| remaining > delay);
                if !can_retry {
                    observe(&NativeModelRequestObservation::Failed {
                        code: failure.code,
                        attempts: attempt,
                    });
                    return Err(error);
                }
                last_failure_code = Some(failure.code.clone());
                observe(&NativeModelRequestObservation::RetryScheduled {
                    code: failure.code.clone(),
                    attempt,
                });
                wait_for_native_model_retry(delay, deadline, cancelled)?;
            }
        }
    }
    Err("provider_unknown".to_owned())
}

fn native_model_attempt_timeout(remaining: Duration, attempt: usize) -> Duration {
    let attempts_left = NATIVE_MODEL_MAX_ATTEMPTS
        .saturating_sub(attempt)
        .saturating_add(1);
    let share = remaining / u32::try_from(attempts_left).unwrap_or(1);
    if share.is_zero() {
        remaining
    } else {
        share
    }
}

fn wait_for_native_model_retry(
    delay: Duration,
    deadline: Instant,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), String> {
    let retry_at = (Instant::now() + delay).min(deadline);
    loop {
        if cancelled() {
            return Err("native_tool_loop_cancelled".to_owned());
        }
        let Some(remaining) = retry_at.checked_duration_since(Instant::now()) else {
            return Ok(());
        };
        if remaining.is_zero() {
            return Ok(());
        }
        std::thread::sleep(NATIVE_MODEL_RETRY_CANCEL_POLL.min(remaining));
    }
}

fn native_model_request_diagnostic(
    observation: &NativeModelRequestObservation,
) -> (&'static str, String) {
    match observation {
        NativeModelRequestObservation::RetryScheduled { code, attempt } => (
            "pipeline_error",
            format!("provider_retry_code={code}_attempt={attempt}"),
        ),
        NativeModelRequestObservation::Recovered { code, attempts } => (
            "pipeline_error",
            format!("provider_recovery_code={code}_attempts={attempts}"),
        ),
        NativeModelRequestObservation::Failed { code, attempts } => (
            "pipeline_error",
            format!("provider_failure_code={code}_attempts={attempts}"),
        ),
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct NativeRunReceipt {
    requires_project_observation: bool,
    successful_observation_this_turn: bool,
    tool_called: bool,
    successful_tool_call: bool,
    needs_confirmation: bool,
    successful_write_tools: std::collections::BTreeSet<String>,
    unverified_requested_write_tools: std::collections::BTreeSet<String>,
    failed_tools: std::collections::BTreeSet<String>,
    pending_tools: std::collections::BTreeSet<String>,
    latest_timeline_version_id: Option<String>,
    preview_timeline_version_id: Option<String>,
    observation_sources: std::collections::BTreeSet<String>,
}

const TIMELINE_VERSION_WRITE_TOOLS: &[&str] = &[
    "create_timeline_draft",
    "replace_clips",
    "change_clip_duration",
    "reorder_clips",
    "replace_text_tracks",
    "download_music",
    "use_online_music",
    "replace_music_tracks",
    "synthesize_voiceover",
];

fn result_timeline_version_id(result: &Value) -> Option<&str> {
    result["timelineVersionId"].as_str().or_else(|| {
        result
            .pointer("/artifact/timelineVersionId")
            .and_then(Value::as_str)
    })
}

fn record_successful_write(
    receipt: &mut NativeRunReceipt,
    tool: &str,
    result: &Value,
    tool_policy: &RequestToolPolicy,
) {
    let timeline_version_id = result_timeline_version_id(result).map(str::to_owned);
    if tool == "render_preview" {
        let preview_is_current = timeline_version_id.as_ref().is_some_and(|version| {
            receipt
                .latest_timeline_version_id
                .as_ref()
                .is_none_or(|latest| latest == version)
        });
        if preview_is_current {
            if receipt.latest_timeline_version_id.is_none() {
                receipt.latest_timeline_version_id = timeline_version_id.clone();
            }
            receipt.preview_timeline_version_id = timeline_version_id;
            receipt.successful_write_tools.insert(tool.to_owned());
            receipt.unverified_requested_write_tools.remove(tool);
        }
        return;
    }

    if TIMELINE_VERSION_WRITE_TOOLS.contains(&tool) {
        let preview_still_current = match (
            receipt.preview_timeline_version_id.as_ref(),
            timeline_version_id.as_ref(),
        ) {
            (Some(preview), Some(updated)) => preview == updated,
            (None, _) => true,
            (Some(_), None) => false,
        };
        receipt.latest_timeline_version_id = timeline_version_id;
        if !preview_still_current {
            receipt.preview_timeline_version_id = None;
            receipt.successful_write_tools.remove("render_preview");
            if tool_policy.native_write_requested("render_preview") {
                receipt
                    .unverified_requested_write_tools
                    .insert("render_preview".to_owned());
            }
        }
    }
    receipt.successful_write_tools.insert(tool.to_owned());
    receipt.unverified_requested_write_tools.remove(tool);
}

fn drive_native_loop(
    input: &mut Vec<Value>,
    is_custom: bool,
    requires_observation: bool,
    tool_policy: &RequestToolPolicy,
    receipt: &mut NativeRunReceipt,
    request: &str,
    run_deadline: Instant,
    respond: &mut NativeRespond<'_>,
    execute: &mut NativeExecute<'_>,
    refresh_snapshot: &mut NativeRefreshSnapshot<'_>,
    mut cancelled: impl FnMut() -> bool,
    mut observed: impl FnMut(&str, usize),
) -> Result<String, String> {
    receipt.requires_project_observation = requires_observation;
    receipt.unverified_requested_write_tools.extend(
        tool_policy
            .requested_native_write_tools()
            .map(str::to_owned),
    );
    let mut storyboard_confirmation_pending = false;
    let mut tool_step_number = 0usize;
    let mut continuation = ContinuationState::default();
    let mut loaded_tools = std::collections::BTreeSet::<String>::new();
    for step_number in 1..=MAX_STEPS {
        if cancelled() {
            return Err("native_tool_loop_cancelled".to_owned());
        }
        let available_tools = full_native_tool_catalog();
        let available_names = available_tools
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        let mut exposed_tools = vec![load_tools_function_tool(&available_names)];
        exposed_tools.extend(available_tools.into_iter().filter(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| loaded_tools.contains(name))
        }));
        compact_native_context(
            input,
            &exposed_tools,
            is_custom,
            request,
            run_deadline,
            respond,
        )?;
        let Some(timeout) = remaining_timeout(run_deadline) else {
            return Err("native_tool_loop_deadline_exceeded".to_owned());
        };
        let payload = json!({
            "model": "gpt-5.4",
            "store": false,
            "stream": false,
            "parallel_tool_calls": false,
            "tool_choice": "auto",
            "tools": exposed_tools,
            "input": input,
        });
        let body = respond(&payload, timeout)?;
        observed(&body, step_number);
        if cancelled() {
            return Err("native_tool_loop_cancelled".to_owned());
        }
        let turn = if is_custom {
            model_turn_from_chat_completions(&body)
        } else {
            model_turn_from_responses(&body)
        }
        .ok_or_else(|| "native_tool_loop_response_unparseable".to_owned())?;
        let calls = turn.function_calls().cloned().collect::<Vec<_>>();
        if calls.is_empty() {
            let Some(message) = model_message_text(&turn) else {
                return Err("native_tool_loop_response_missing_message".to_owned());
            };
            if continue_after_natural_language(
                input,
                requires_observation,
                receipt,
                &mut continuation,
                step_number,
            ) {
                continue;
            }
            return Ok(message);
        }
        receipt.tool_called = true;
        let response_loads_tools = calls.iter().any(|call| call.name == LOAD_TOOLS);

        for item in &turn.output {
            if let Some(value) = output_item_for_input(item, is_custom) {
                input.push(value);
            }
        }
        let mut step_results = Vec::new();
        let mut next_loaded_tools = None;
        for call in calls {
            if cancelled() {
                return Err("native_tool_loop_cancelled".to_owned());
            }
            tool_step_number += 1;
            let is_observation = OBSERVATION_TOOLS.contains(&call.name.as_str());
            let is_project_observation =
                is_observation && !matches!(call.name.as_str(), LOAD_TOOLS | READ_LOGS);
            let executed = !(storyboard_confirmation_pending && !is_observation);
            let result = if response_loads_tools && !dynamic_tool_loaded(&call.name, &loaded_tools)
            {
                tool_not_loaded(&call.name)
            } else if !executed {
                storyboard_confirmation_required(&call.name)
            } else if call.name == LOAD_TOOLS
                && !load_request_uses_available_names(&call, &available_names)
            {
                invalid_arguments()
            } else {
                execute(&call, tool_step_number)?
            };
            let result_status = result["status"].as_str();
            if result_status == Some("needs_confirmation") {
                storyboard_confirmation_pending = true;
                receipt.needs_confirmation = true;
            }
            if call.name == LOAD_TOOLS && result_status == Some("ok") {
                next_loaded_tools = Some(
                    result["loadedTools"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                );
            }
            if is_project_observation && result_status == Some("ok") {
                receipt.successful_observation_this_turn = true;
                receipt.observation_sources.insert(call.name.clone());
            }
            if matches!(
                result_status,
                Some("ok") | Some("queued") | Some("needs_confirmation")
            ) {
                receipt.successful_tool_call = true;
                receipt.failed_tools.remove(&call.name);
                if result_status == Some("queued") {
                    receipt.pending_tools.insert(call.name.clone());
                } else {
                    receipt.pending_tools.remove(&call.name);
                }
                if result_status == Some("ok")
                    && !is_observation
                    && tool_policy.native_write_authorized(&call.name)
                {
                    record_successful_write(receipt, &call.name, &result, tool_policy);
                }
            } else {
                receipt.failed_tools.insert(call.name.clone());
            }
            step_results.push((call.name.clone(), result.clone(), is_observation));
            if executed
                && !is_observation
                && matches!(
                    result_status,
                    Some("ok") | Some("queued") | Some("needs_confirmation")
                )
            {
                if let Some(snapshot) = refresh_snapshot()? {
                    replace_state_snapshot(input, snapshot)?;
                }
            }
            input.push(json!({
                "type": "function_call_output",
                "call_id": call.call_id,
                "output": result.to_string(),
            }));
        }
        if let Some(next_loaded_tools) = next_loaded_tools {
            loaded_tools = next_loaded_tools;
        }
        continuation.record_step(&step_results);
    }
    Err("native_tool_loop_max_steps".to_owned())
}

/// 自然语言可以结束循环，但两套续步可以拦截提前收工：写工具可重试失败走修复，
/// 产物已落地但仍带质量缺口走精炼。项目事实观察门仍然优先。返回 true 表示继续循环。
fn continue_after_natural_language(
    input: &mut Vec<Value>,
    requires_observation: bool,
    receipt: &NativeRunReceipt,
    continuation: &mut ContinuationState,
    step_number: usize,
) -> bool {
    if requires_observation
        && !receipt.successful_observation_this_turn
        && receipt.failed_tools.is_empty()
    {
        input.push(json!({
            "role": "system",
            "content": [{
                "type": "input_text",
                "text": "This request asks about current project facts. Call one allowed read-only observation function before answering."
            }]
        }));
        return true;
    }
    let Some(kind) = continuation.decide(step_number, receipt.needs_confirmation) else {
        return false;
    };
    let text = continuation.take_message(kind);
    input.push(json!({
        "role": "system",
        "content": [{
            "type": "input_text",
            "text": text
        }]
    }));
    true
}

fn native_system_prompt(tool_policy: &RequestToolPolicy, tool_directory: &str) -> String {
    let mut prompt = "You are a local video project assistant. Answer ordinary questions directly. The system state snapshot is authoritative for current high-level project facts; use observation functions only when more detail is needed. For exact edit or delivery readiness decisions, such as whether export is possible, still call get_edit_status. Treat the state snapshot and function outputs as the only project and artifact facts. Before calling a business function, call load_tools with 1 to 5 names from the directory below. Each load_tools call replaces the current loaded set. Keep load_tools plus all functions needed for the immediate next steps, and reload when needs change. Use artifact-producing functions only when they match the user's request and are present in tools. A generated storyboard with status needs_confirmation must be summarized for user review; do not create or edit a timeline until the user confirms it in a later turn. Claim an artifact was created only when its function output confirms success. If a write function returns a retryable failure, call another allowed function to recover before answering; do not claim the artifact exists. If a write function succeeds with qualityWarnings, adjust with allowed functions; do not treat warnings as a finished edit. If another function returns a structured failure, explain it safely or adjust with another allowed function.".to_owned();
    if tool_policy.native_write_authorized("generate_storyboard") {
        prompt.push_str(
            " If the user asked for a video, call generate_storyboard. If they supplied copy, pass it as brief; otherwise use the current task brief. Do not assemble shots with list_assets or search_assets.",
        );
    }
    if tool_policy.native_write_authorized("synthesize_voiceover") {
        prompt.push_str(
            " If the user explicitly asked for voiceover, generate a storyboard that writes narrationText per shot. After the user confirms, call synthesize_voiceover with text null so it uses storyboard narrationText. Never speak onScreenText. voiceId and timelineVersionId may be null.",
        );
    }
    prompt.push_str(" Available tool directory:\n");
    prompt.push_str(tool_directory);
    prompt
}

fn full_native_tool_catalog() -> Vec<Value> {
    native_function_tools_for_request(true, true)
}

fn replace_state_snapshot(input: &mut [Value], replacement: Value) -> Result<(), String> {
    let indexes = input
        .iter()
        .enumerate()
        .filter_map(|(index, item)| is_snapshot_message(item).then_some(index))
        .collect::<Vec<_>>();
    if indexes.len() != 1 || !is_snapshot_message(&replacement) {
        return Err("native_state_snapshot_invariant_failed".to_owned());
    }
    input[indexes[0]] = replacement;
    Ok(())
}

fn native_render_preview_allowed(_request: &str, tool_policy: &RequestToolPolicy) -> bool {
    tool_policy.native_write_authorized("render_preview")
}

fn remaining_timeout(deadline: Instant) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(Instant::now())?;
    (!remaining.is_zero()).then_some(AGENT_STEP_TIMEOUT.min(remaining))
}

fn model_message_text(turn: &crate::provider::ModelTurn) -> Option<String> {
    let text = turn
        .output
        .iter()
        .filter_map(|item| match item {
            ModelOutputItem::Message { content, .. } => Some(content),
            _ => None,
        })
        .flat_map(|content| content.iter())
        .filter_map(|item| {
            item.get("text")
                .and_then(Value::as_str)
                .or_else(|| item.as_str())
        })
        .collect::<Vec<_>>()
        .join("");
    (!text.trim().is_empty()).then_some(text)
}

fn output_item_for_input(item: &ModelOutputItem, is_custom: bool) -> Option<Value> {
    match item {
        ModelOutputItem::Message {
            id: _,
            role,
            content,
            raw,
        } if !is_custom && raw.is_object() => Some(raw.clone()),
        ModelOutputItem::Message { role, content, .. } => Some(json!({
            "role": role,
            "content": content,
        })),
        ModelOutputItem::FunctionCall(call) if !is_custom && call.raw.is_object() => {
            Some(call.raw.clone())
        }
        ModelOutputItem::FunctionCall(call) => Some(json!({
            "type": "function_call",
            "call_id": call.call_id,
            "name": call.name,
            "arguments": call.arguments,
        })),
        ModelOutputItem::Other(raw) if !raw.is_null() => Some(raw.clone()),
        ModelOutputItem::Other(_) => None,
    }
}

fn compact_native_context(
    input: &mut Vec<Value>,
    tools: &[Value],
    is_custom: bool,
    request: &str,
    run_deadline: Instant,
    respond: &mut NativeRespond<'_>,
) -> Result<(), String> {
    compact_tool_outputs(input);
    if provider_payload_tokens(input, tools) <= COMPRESSION_TRIGGER_TOKENS {
        return Ok(());
    }

    let plan = compression_plan(input, request);
    let mut summary = String::new();
    for batch in summary_batches(plan.older) {
        let Some(timeout) = remaining_timeout(run_deadline) else {
            return Err("native_tool_loop_deadline_exceeded".to_owned());
        };
        let payload = compression_payload(&summary, &batch);
        if provider_payload_tokens(
            payload["input"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            &[],
        ) > MAX_CONTEXT_TOKENS
        {
            return Err("native_context_compression_request_too_large".to_owned());
        }
        summary = respond(&payload, timeout)
            .ok()
            .and_then(|body| {
                let turn = if is_custom {
                    model_turn_from_chat_completions(&body)
                } else {
                    model_turn_from_responses(&body)
                }?;
                model_message_text(&turn)
            })
            .map(|next| truncate_text_to_tokens(&next, 6_000))
            .ok_or_else(|| "native_context_compression_failed".to_owned())?;
    }

    if summary.trim().is_empty() {
        return Err("native_context_compression_failed".to_owned());
    }
    let mut compacted = plan.retained;
    let insert_at = compacted
        .iter()
        .position(is_snapshot_message)
        .map_or(1, |index| index + 1);
    compacted.insert(insert_at, memory_item(&summary));
    if provider_payload_tokens(&compacted, tools) > COMPRESSION_TARGET_TOKENS {
        return Err("native_context_compression_target_not_reached".to_owned());
    }
    *input = compacted;
    Ok(())
}

fn compression_payload(previous_summary: &str, history_batch: &str) -> Value {
    json!({
        "model": "gpt-5.4",
        "store": false,
        "stream": false,
        "max_output_tokens": 5_000,
        "input": [
            {
                "role": "system",
                "content": [{
                    "type": "input_text",
                    "text": "自主压缩会话记忆。必须完整保留：用户目标、用户明确约束、用户偏好、已作决定及原因、未解决问题。其他有助于继续任务的信息由你自行判断、合并和压缩。删除重复讨论、过时过程、冗长工具输出和已解决的中间问题。不得添加原历史中不存在的事实。只输出压缩后的记忆，不要解释压缩过程。"
                }]
            },
            {
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": format!("已有压缩记忆：\n{previous_summary}\n\n待合并历史（JSONL）：\n{history_batch}")
                }]
            }
        ]
    })
}

fn execute_native_tool(
    state: &mut LoopState,
    call: &FunctionCall,
    step_number: usize,
    render_preview_authorized: bool,
) -> Result<Value, String> {
    let allowed = NATIVE_TOOL_NAMES.contains(&call.name.as_str());
    let persisted_name = if allowed {
        call.name.as_str()
    } else {
        "tool_not_allowed"
    };
    let step_id = begin_agent_run_step(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.agent_task_id,
        step_number as i64,
        persisted_name,
    )?;
    if !allowed {
        finish_agent_run_step(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.agent_task_id,
            &step_id,
            "failed",
            None,
            None,
            Some("tool_not_allowed"),
        )?;
        return Ok(json!({
            "status": "failed",
            "operation": "native_observation",
            "stage": "tool_allowlist",
            "code": "tool_not_allowed",
            "retryable": false,
            "responseInstruction": "Explain that only the allowed read-only observation or preview function tools are available, then answer from available facts or ask the user to rephrase."
        }));
    }
    if !dynamic_tool_loaded(&call.name, &state.loaded_tools) {
        finish_agent_run_step(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.agent_task_id,
            &step_id,
            "failed",
            None,
            None,
            Some("tool_not_loaded"),
        )?;
        return Ok(tool_not_loaded(&call.name));
    }
    if !native_tool_call_allowed(&call.name, &state.tool_policy, render_preview_authorized) {
        finish_agent_run_step(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.agent_task_id,
            &step_id,
            "failed",
            None,
            None,
            Some("user_restricted_tool"),
        )?;
        return Ok(json!({
            "status": "failed",
            "operation": call.name,
            "stage": "permission",
            "code": "user_restricted_tool",
            "retryable": false,
            "responseInstruction": "Explain that this operation was not authorized for the current request. Use the allowed observation functions or ask the user to explicitly request the operation; do not claim it ran."
        }));
    }
    let args = match parse_native_arguments(&call.name, &call.arguments) {
        Ok(args) => args,
        Err(error) => {
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &step_id,
                "failed",
                None,
                None,
                Some("invalid_arguments"),
            )?;
            return Ok(error);
        }
    };
    if call.name == LOAD_TOOLS {
        let requested = args["toolNames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        if !requested
            .iter()
            .all(|name| NATIVE_TOOL_NAMES.contains(name) && *name != LOAD_TOOLS)
        {
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &step_id,
                "failed",
                None,
                None,
                Some("invalid_arguments"),
            )?;
            return Ok(invalid_arguments());
        }
        state.loaded_tools = requested.iter().map(|name| (*name).to_owned()).collect();
        let result = json!({
            "tool": LOAD_TOOLS,
            "status": "ok",
            "loadedTools": requested
        });
        finish_agent_run_step(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.agent_task_id,
            &step_id,
            "completed",
            None,
            None,
            None,
        )?;
        return Ok(result);
    }
    let started_at = Instant::now();
    let previous_outcome = state.last_outcome.take();
    let result = apply_skill(state, &call.name, &args);
    let current_outcome = state.last_outcome.take();
    state.last_outcome = match (previous_outcome, current_outcome) {
        (previous, Some(current)) => Some(merge_native_outcomes(previous, current, &call.name)),
        (previous, None) => previous,
    };
    let _ = record_agent_timing_diagnostic(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.conversation_id,
        state.agent_task_id,
        Some(step_number as i64),
        AgentTimingMetric::SkillExecution,
        started_at.elapsed(),
    );
    match result {
        Ok(value) => {
            let value = match prepare_native_tool_result(&call.name, value) {
                Ok(value) => value,
                Err(error) => {
                    finish_agent_run_step(
                        state.connection,
                        state.project_id,
                        state.editing_task_id,
                        state.agent_task_id,
                        &step_id,
                        "failed",
                        None,
                        None,
                        Some("unsafe_tool_result"),
                    )?;
                    state.last_failed_tool_error_code = Some("unsafe_tool_result");
                    return Ok(error);
                }
            };
            if OBSERVATION_TOOLS.contains(&call.name.as_str())
                && !matches!(call.name.as_str(), LOAD_TOOLS | READ_LOGS)
                && value["status"] == "ok"
            {
                state.successful_observation = true;
            }
            let artifact = persisted_artifact_for_tool(state, &call.name);
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &step_id,
                "completed",
                artifact.as_ref().map(|(kind, _)| *kind),
                artifact.as_ref().map(|(_, id)| id.as_str()),
                None,
            )?;
            Ok(value)
        }
        Err(error) => {
            let code = safe_step_error_code(&error);
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &step_id,
                "failed",
                None,
                None,
                Some(code),
            )?;
            state.last_failed_tool_error_code = Some(code);
            Ok(safe_tool_failure_context(&call.name, &error))
        }
    }
}

fn record_repeated_invalid_call(
    state: &LoopState<'_>,
    call: &FunctionCall,
    step_number: usize,
) -> Result<(), String> {
    let persisted_name = if NATIVE_TOOL_NAMES.contains(&call.name.as_str()) {
        call.name.as_str()
    } else {
        "tool_not_allowed"
    };
    let step_id = begin_agent_run_step(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.agent_task_id,
        step_number as i64,
        persisted_name,
    )?;
    finish_agent_run_step(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.agent_task_id,
        &step_id,
        "failed",
        None,
        None,
        Some("repeated_invalid_arguments"),
    )
}

fn native_tool_call_allowed(
    tool: &str,
    policy: &RequestToolPolicy,
    render_preview_authorized: bool,
) -> bool {
    if policy.forbids(tool) {
        return false;
    }
    if tool == "render_preview" {
        return render_preview_authorized;
    }
    policy.native_tool_exposed(tool)
}

fn parse_native_arguments(tool: &str, arguments: &str) -> Result<Value, Value> {
    let mut value = serde_json::from_str::<Value>(arguments).map_err(|_| invalid_arguments())?;
    match tool {
        "search_assets" => {
            coerce_blank_strings_to_null(&mut value, &["query", "kind", "tag", "collectionId"])
        }
        "search_asset_segments" => coerce_blank_strings_to_null(&mut value, &["assetId"]),
        "get_timeline" | "render_preview" | "create_jianying_draft" => {
            coerce_blank_strings_to_null(&mut value, &["timelineVersionId"]);
        }
        "synthesize_voiceover" => {
            coerce_blank_strings_to_null(&mut value, &["text", "voiceId", "timelineVersionId"])
        }
        _ => {}
    }
    let Some(object) = value.as_object() else {
        return Err(invalid_arguments());
    };
    match tool {
        "load_tools" => {
            if object.len() != 1 || !object.contains_key("toolNames") {
                return Err(invalid_arguments());
            }
            let Some(names) = object["toolNames"].as_array() else {
                return Err(invalid_arguments());
            };
            let unique = names
                .iter()
                .filter_map(Value::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            if names.is_empty()
                || names.len() > 5
                || unique.len() != names.len()
                || !names.iter().all(|name| bounded_required_string(name, 64))
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "read_logs" => {
            if object.len() != 2
                || !object.contains_key("startLine")
                || !object.contains_key("endLine")
            {
                return Err(invalid_arguments());
            }
            match (object["startLine"].as_u64(), object["endLine"].as_u64()) {
                (None, None) if object["startLine"].is_null() && object["endLine"].is_null() => {
                    Ok(value)
                }
                (Some(start), Some(end))
                    if start >= 1 && start <= end && end <= 1_000_000 && end - start + 1 <= 100 =>
                {
                    Ok(value)
                }
                _ => Err(invalid_arguments()),
            }
        }
        "get_edit_status"
        | "get_asset_health_summary"
        | "list_assets"
        | "list_voices"
        | "get_storyboard"
        | "get_text_capabilities"
            if object.is_empty() =>
        {
            Ok(value)
        }
        "get_edit_status"
        | "get_asset_health_summary"
        | "list_assets"
        | "get_storyboard"
        | "get_text_capabilities" => Err(invalid_arguments()),
        "get_timeline" => {
            if object.len() != 1 || !object.contains_key("timelineVersionId") {
                return Err(invalid_arguments());
            }
            if object
                .get("timelineVersionId")
                .is_some_and(|value| !(value.is_null() || value.is_string()))
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "request_asset_analysis" => {
            if object.len() != 1 || !object.contains_key("assetIds") {
                return Err(invalid_arguments());
            }
            let Some(asset_ids) = object["assetIds"].as_array() else {
                return Err(invalid_arguments());
            };
            if asset_ids.is_empty()
                || asset_ids.len() > 100
                || !asset_ids
                    .iter()
                    .all(|asset_id| bounded_required_string(asset_id, 200))
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "generate_storyboard" => {
            if object.len() != 1 || !object.contains_key("brief") {
                return Err(invalid_arguments());
            }
            if !nullable_bounded_string_argument(&object["brief"], 4_000) {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "create_timeline_draft" => {
            if object.is_empty() {
                Ok(value)
            } else {
                Err(invalid_arguments())
            }
        }
        "replace_clips" => {
            if object.len() != 2
                || !object.contains_key("timelineVersionId")
                || !object.contains_key("shots")
                || !nullable_timeline_id(&object["timelineVersionId"])
            {
                return Err(invalid_arguments());
            }
            let Some(shots) = object["shots"].as_array() else {
                return Err(invalid_arguments());
            };
            if shots.is_empty() || shots.len() > 100 || !shots.iter().all(valid_clip_replacement) {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "change_clip_duration" => {
            if object.len() != 2
                || !object.contains_key("timelineVersionId")
                || !object.contains_key("adjustments")
                || !nullable_timeline_id(&object["timelineVersionId"])
            {
                return Err(invalid_arguments());
            }
            let Some(adjustments) = object["adjustments"].as_array() else {
                return Err(invalid_arguments());
            };
            if adjustments.is_empty()
                || adjustments.len() > 100
                || !adjustments.iter().all(valid_clip_adjustment)
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "reorder_clips" => {
            if object.len() != 2
                || !object.contains_key("timelineVersionId")
                || !object.contains_key("order")
                || !nullable_timeline_id(&object["timelineVersionId"])
            {
                return Err(invalid_arguments());
            }
            let Some(order) = object["order"].as_array() else {
                return Err(invalid_arguments());
            };
            if order.is_empty()
                || order.len() > 100
                || !order
                    .iter()
                    .all(|index| index.as_i64().is_some_and(|index| index >= 0))
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "search_assets" => {
            const KEYS: &[&str] = &[
                "query",
                "kind",
                "minDurationMs",
                "maxDurationMs",
                "minRating",
                "favoriteOnly",
                "tag",
                "collectionId",
                "offset",
                "limit",
            ];
            if object.len() != KEYS.len() || object.keys().any(|key| !KEYS.contains(&key.as_str()))
            {
                return Err(invalid_arguments());
            }
            if !nullable_bounded_string_argument(&object["query"], 200)
                || !nullable_bounded_string_argument(&object["tag"], 200)
                || !nullable_bounded_string_argument(&object["collectionId"], 200)
                || !matches!(
                    object["kind"].as_str(),
                    None | Some("video" | "image" | "audio" | "other")
                )
                || !non_negative_integer_or_null(&object["minDurationMs"])
                || !non_negative_integer_or_null(&object["maxDurationMs"])
                || !nullable_integer_in_range(&object["minRating"], 0, 5)
                || !object["favoriteOnly"].is_boolean()
                || !bounded_integer(&object["offset"], 0, 10_000)
                || !bounded_integer(&object["limit"], 1, 20)
            {
                return Err(invalid_arguments());
            }
            if let (Some(min), Some(max)) = (
                object["minDurationMs"].as_i64(),
                object["maxDurationMs"].as_i64(),
            ) {
                if min > max {
                    return Err(invalid_arguments());
                }
            }
            Ok(value)
        }
        "search_asset_segments" => {
            const KEYS: &[&str] = &["query", "assetId", "offset", "limit"];
            if object.len() != KEYS.len() || object.keys().any(|key| !KEYS.contains(&key.as_str()))
            {
                return Err(invalid_arguments());
            }
            if !bounded_required_string(&object["query"], 200)
                || !nullable_bounded_string_argument(&object["assetId"], 200)
                || !bounded_integer(&object["offset"], 0, 10_000)
                || !bounded_integer(&object["limit"], 1, 20)
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "search_music" => {
            if object.len() != 1
                || !object.contains_key("query")
                || !bounded_required_string(&object["query"], 200)
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "list_voices" => {
            if object.is_empty() {
                Ok(value)
            } else {
                Err(invalid_arguments())
            }
        }
        "synthesize_voiceover" => {
            if object.len() != 3
                || !object.contains_key("text")
                || !object.contains_key("voiceId")
                || !object.contains_key("timelineVersionId")
                || !nullable_bounded_string_argument(&object["text"], 5_000)
                || !nullable_bounded_string_argument(&object["voiceId"], 200)
                || !nullable_timeline_id(&object["timelineVersionId"])
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "render_preview" => {
            if object.len() != 1 || !object.contains_key("timelineVersionId") {
                return Err(invalid_arguments());
            }
            if object
                .get("timelineVersionId")
                .is_some_and(|value| !(value.is_null() || value.is_string()))
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "download_music" => {
            if object.len() != 1
                || !object.contains_key("trackId")
                || !bounded_required_string(&object["trackId"], 200)
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "use_online_music" => {
            if object.len() != 2
                || !object.contains_key("trackId")
                || !object.contains_key("timelineVersionId")
                || !bounded_required_string(&object["trackId"], 200)
                || !nullable_timeline_id(&object["timelineVersionId"])
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "create_jianying_draft" => {
            if object.len() != 1
                || !object.contains_key("timelineVersionId")
                || !nullable_timeline_id(&object["timelineVersionId"])
            {
                return Err(invalid_arguments());
            }
            Ok(value)
        }
        "replace_text_tracks" => {
            if object.len() != 2
                || !object.contains_key("timelineVersionId")
                || !object.contains_key("textTracks")
                || !nullable_timeline_id(&object["timelineVersionId"])
            {
                return Err(invalid_arguments());
            }
            let Some(tracks) = object["textTracks"].as_array() else {
                return Err(invalid_arguments());
            };
            if tracks.len() > 21 || !tracks.iter().all(valid_text_track_argument) {
                return Err(invalid_arguments());
            }
            normalize_nullable_text_fields(&mut value);
            Ok(value)
        }
        "replace_music_tracks" => {
            if object.len() != 2
                || !object.contains_key("timelineVersionId")
                || !object.contains_key("musicTracks")
                || !nullable_timeline_id(&object["timelineVersionId"])
            {
                return Err(invalid_arguments());
            }
            let Some(tracks) = object["musicTracks"].as_array() else {
                return Err(invalid_arguments());
            };
            if tracks.len() > 100 || !tracks.iter().all(valid_music_track_argument) {
                return Err(invalid_arguments());
            }
            normalize_nullable_music_fields(&mut value);
            Ok(value)
        }
        _ => Err(invalid_arguments()),
    }
}

fn bounded_required_string(value: &Value, max_length: usize) -> bool {
    value
        .as_str()
        .is_some_and(|text| !text.trim().is_empty() && text.chars().count() <= max_length)
}

fn nullable_bounded_string_argument(value: &Value, max_length: usize) -> bool {
    value.is_null() || bounded_required_string(value, max_length)
}

fn coerce_blank_strings_to_null(value: &mut Value, keys: &[&str]) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for key in keys {
        if object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|text| text.trim().is_empty())
        {
            object.insert((*key).to_owned(), Value::Null);
        }
    }
}

fn nullable_timeline_id(value: &Value) -> bool {
    value.is_null() || bounded_required_string(value, 200)
}

fn valid_clip_replacement(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &["shotIndex", "assetId", "sourceStartMs", "sourceEndMs"];
    if object.len() != KEYS.len() || object.keys().any(|key| !KEYS.contains(&key.as_str())) {
        return false;
    }
    let Some(shot_index) = object["shotIndex"].as_i64() else {
        return false;
    };
    let Some(source_start_ms) = object["sourceStartMs"].as_i64() else {
        return false;
    };
    let Some(source_end_ms) = object["sourceEndMs"].as_i64() else {
        return false;
    };
    shot_index >= 0
        && bounded_required_string(&object["assetId"], 200)
        && source_start_ms >= 0
        && source_end_ms >= source_start_ms
}

fn valid_clip_adjustment(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &["shotIndex", "newDurationMs", "newSourceStartMs"];
    if object.len() != KEYS.len() || object.keys().any(|key| !KEYS.contains(&key.as_str())) {
        return false;
    }
    let Some(shot_index) = object["shotIndex"].as_i64() else {
        return false;
    };
    let duration_present = !object["newDurationMs"].is_null();
    let source_start_present = !object["newSourceStartMs"].is_null();
    let valid_duration = !duration_present
        || object["newDurationMs"]
            .as_i64()
            .is_some_and(|duration| duration > 0);
    let valid_source_start = !source_start_present
        || object["newSourceStartMs"]
            .as_i64()
            .is_some_and(|start| start >= 0);
    shot_index >= 0
        && (duration_present || source_start_present)
        && valid_duration
        && valid_source_start
}

fn valid_text_track_argument(value: &Value) -> bool {
    let Some(track) = value.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &["id", "role", "layer", "enabled", "cues"];
    if !closed_object_has_keys(track, KEYS)
        || !bounded_required_string(&track["id"], 200)
        || !matches!(
            track["role"].as_str(),
            Some("subtitle" | "headline" | "callout" | "cta" | "label")
        )
        || !bounded_integer(&track["layer"], 0, 20)
        || !track["enabled"].is_boolean()
    {
        return false;
    }
    track["cues"]
        .as_array()
        .is_some_and(|cues| cues.len() <= 100 && cues.iter().all(valid_text_cue_argument))
}

fn valid_text_cue_argument(value: &Value) -> bool {
    let Some(cue) = value.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &[
        "id",
        "templateId",
        "startMs",
        "endMs",
        "text",
        "style",
        "layout",
        "entrance",
        "exit",
        "loopAnimation",
    ];
    if !closed_object_has_keys(cue, KEYS)
        || !bounded_required_string(&cue["id"], 200)
        || !nullable_text_template(&cue["templateId"])
        || !bounded_required_string(&cue["text"], 280)
        || !valid_nullable_text_style(&cue["style"])
        || !valid_nullable_text_layout(&cue["layout"])
        || !valid_nullable_text_animation(&cue["entrance"])
        || !valid_nullable_text_animation(&cue["exit"])
        || !valid_nullable_text_animation(&cue["loopAnimation"])
    {
        return false;
    }
    cue["startMs"]
        .as_i64()
        .zip(cue["endMs"].as_i64())
        .is_some_and(|(start, end)| start >= 0 && end > start)
}

fn nullable_text_template(value: &Value) -> bool {
    value.is_null()
        || matches!(
            value.as_str(),
            Some(
                "subtitle_safe"
                    | "headline_rise"
                    | "headline_pop"
                    | "headline_drop"
                    | "callout_card"
                    | "cta_card"
            )
        )
}

fn valid_nullable_text_style(value: &Value) -> bool {
    if value.is_null() {
        return true;
    }
    let Some(style) = value.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &[
        "fontKey",
        "fontSize",
        "bold",
        "color",
        "strokeColor",
        "strokeWidth",
        "shadow",
        "backgroundColor",
        "alignment",
        "letterSpacing",
        "lineSpacing",
    ];
    closed_object_has_keys(style, KEYS)
        && bounded_required_string(&style["fontKey"], 200)
        && bounded_number(&style["fontSize"], 0.01, 0.30)
        && style["bold"].is_boolean()
        && valid_hex_color(&style["color"])
        && nullable_hex_color(&style["strokeColor"])
        && bounded_number(&style["strokeWidth"], 0.0, 10.0)
        && style["shadow"].is_boolean()
        && nullable_hex_color(&style["backgroundColor"])
        && matches!(
            style["alignment"].as_str(),
            Some("left" | "center" | "right")
        )
        && bounded_integer(&style["letterSpacing"], -100, 100)
        && bounded_integer(&style["lineSpacing"], -100, 100)
}

fn valid_nullable_text_layout(value: &Value) -> bool {
    if value.is_null() {
        return true;
    }
    let Some(layout) = value.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &["anchor", "x", "y", "maxWidth", "safeArea"];
    closed_object_has_keys(layout, KEYS)
        && matches!(layout["anchor"].as_str(), Some("top" | "center" | "bottom"))
        && bounded_number(&layout["x"], 0.0, 1.0)
        && bounded_number(&layout["y"], 0.0, 1.0)
        && bounded_number(&layout["maxWidth"], 0.20, 1.0)
        && matches!(
            layout["safeArea"].as_str(),
            Some("title_safe" | "action_safe")
        )
}

fn valid_nullable_text_animation(value: &Value) -> bool {
    if value.is_null() {
        return true;
    }
    let Some(animation) = value.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &["templateId", "durationMs", "intensity"];
    closed_object_has_keys(animation, KEYS)
        && matches!(
            animation["templateId"].as_str(),
            Some("fade" | "slide_up" | "slide_down" | "pop" | "wipe")
        )
        && animation["durationMs"]
            .as_i64()
            .is_some_and(|value| value >= 0)
        && bounded_number(&animation["intensity"], 0.0, 1.0)
}

fn valid_music_track_argument(value: &Value) -> bool {
    let Some(track) = value.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &["id", "enabled", "cues"];
    if !closed_object_has_keys(track, KEYS)
        || !bounded_required_string(&track["id"], 200)
        || !track["enabled"].is_boolean()
    {
        return false;
    }
    track["cues"]
        .as_array()
        .is_some_and(|cues| cues.len() <= 100 && cues.iter().all(valid_music_cue_argument))
}

fn valid_music_cue_argument(value: &Value) -> bool {
    let Some(cue) = value.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &[
        "id",
        "assetId",
        "sourceStartMs",
        "sourceEndMs",
        "timelineStartMs",
        "timelineEndMs",
        "loopEnabled",
        "volume",
        "fadeInMs",
        "fadeOutMs",
    ];
    if !closed_object_has_keys(cue, KEYS)
        || !bounded_required_string(&cue["id"], 200)
        || !bounded_required_string(&cue["assetId"], 200)
        || !(cue["loopEnabled"].is_null() || cue["loopEnabled"].is_boolean())
        || !bounded_number(&cue["volume"], 0.0, 2.0)
        || !non_negative_integer_or_null(&cue["fadeInMs"])
        || !non_negative_integer_or_null(&cue["fadeOutMs"])
    {
        return false;
    }
    let Some(source_start) = cue["sourceStartMs"].as_i64() else {
        return false;
    };
    let Some(source_end) = cue["sourceEndMs"].as_i64() else {
        return false;
    };
    let Some(timeline_start) = cue["timelineStartMs"].as_i64() else {
        return false;
    };
    let Some(timeline_end) = cue["timelineEndMs"].as_i64() else {
        return false;
    };
    source_start >= 0
        && source_end > source_start
        && timeline_start >= 0
        && timeline_end > timeline_start
}

fn closed_object_has_keys(object: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && object.keys().all(|key| keys.contains(&key.as_str()))
}

fn bounded_number(value: &Value, minimum: f64, maximum: f64) -> bool {
    value
        .as_f64()
        .is_some_and(|number| number.is_finite() && (minimum..=maximum).contains(&number))
}

fn valid_hex_color(value: &Value) -> bool {
    value.as_str().is_some_and(|color| {
        color.len() == 7
            && color.starts_with('#')
            && color[1..]
                .chars()
                .all(|character| character.is_ascii_hexdigit())
    })
}

fn nullable_hex_color(value: &Value) -> bool {
    value.is_null() || valid_hex_color(value)
}

fn normalize_nullable_text_fields(value: &mut Value) {
    let Some(tracks) = value["textTracks"].as_array_mut() else {
        return;
    };
    for track in tracks {
        let Some(cues) = track["cues"].as_array_mut() else {
            continue;
        };
        for cue in cues {
            for key in ["style", "layout"] {
                if cue[key].is_null() {
                    cue.as_object_mut().expect("validated text cue").remove(key);
                }
            }
        }
    }
}

fn normalize_nullable_music_fields(value: &mut Value) {
    let Some(tracks) = value["musicTracks"].as_array_mut() else {
        return;
    };
    for track in tracks {
        let Some(cues) = track["cues"].as_array_mut() else {
            continue;
        };
        for cue in cues {
            for key in ["loopEnabled", "fadeInMs", "fadeOutMs"] {
                if cue[key].is_null() {
                    cue.as_object_mut()
                        .expect("validated music cue")
                        .remove(key);
                }
            }
        }
    }
}

fn non_negative_integer_or_null(value: &Value) -> bool {
    value.is_null() || value.as_i64().is_some_and(|number| number >= 0)
}

fn nullable_integer_in_range(value: &Value, minimum: i64, maximum: i64) -> bool {
    value.is_null()
        || value
            .as_i64()
            .is_some_and(|number| (minimum..=maximum).contains(&number))
}

fn bounded_integer(value: &Value, minimum: i64, maximum: i64) -> bool {
    value
        .as_i64()
        .is_some_and(|number| (minimum..=maximum).contains(&number))
}

fn prepare_native_tool_result(tool: &str, mut result: Value) -> Result<Value, Value> {
    let status_allowed = match (tool, result["status"].as_str()) {
        (_, Some("ok")) => true,
        ("request_asset_analysis", Some("queued")) => true,
        ("generate_storyboard", Some("needs_confirmation")) => true,
        _ => false,
    };
    if result["tool"] != tool || !status_allowed {
        return Err(unsafe_tool_result());
    }
    redact_native_scope_fields(&mut result);
    Ok(result)
}

fn redact_native_scope_fields(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(redact_native_scope_fields),
        Value::Object(object) => {
            for key in [
                "projectId",
                "conversationId",
                "editingTaskId",
                "sourcePath",
                "localPath",
                "previewPath",
                "thumbnailPath",
                "keyframeGridPath",
                "draftDirectory",
                "outputDirectory",
            ] {
                object.remove(key);
            }
            object.values_mut().for_each(redact_native_scope_fields);
        }
        _ => {}
    }
}

fn unsafe_tool_result() -> Value {
    json!({
        "status": "failed",
        "operation": "native_observation",
        "stage": "result_validation",
        "code": "unsafe_tool_result",
        "retryable": false,
        "responseInstruction": "Explain that the observation result could not be safely verified. Do not claim a project fact that was not returned by a safe tool."
    })
}

fn load_request_uses_available_names(call: &FunctionCall, available_names: &[String]) -> bool {
    serde_json::from_str::<Value>(&call.arguments)
        .ok()
        .and_then(|arguments| arguments["toolNames"].as_array().cloned())
        .is_some_and(|names| {
            names.iter().all(|name| {
                name.as_str()
                    .is_some_and(|name| available_names.iter().any(|available| available == name))
            })
        })
}

fn dynamic_tool_loaded(tool: &str, loaded_tools: &std::collections::BTreeSet<String>) -> bool {
    tool == LOAD_TOOLS || loaded_tools.contains(tool)
}

fn tool_not_loaded(tool: &str) -> Value {
    json!({
        "status": "failed",
        "operation": tool,
        "stage": "dynamic_tool_selection",
        "code": "tool_not_loaded",
        "retryable": true,
        "responseInstruction": "Call load_tools with this tool name, keeping any other tools needed for the immediate next steps, then retry."
    })
}

fn invalid_arguments() -> Value {
    json!({
        "status": "failed",
        "operation": "native_observation",
        "stage": "argument_validation",
        "code": "invalid_arguments",
        "retryable": true,
        "responseInstruction": "Explain that the function tool request had invalid arguments, then retry with the documented schema or answer without a tool."
    })
}

fn repeated_invalid_arguments(tool: &str) -> Value {
    json!({
        "status": "failed",
        "operation": tool,
        "stage": "argument_validation",
        "code": "repeated_invalid_arguments",
        "retryable": false,
        "responseInstruction": "The same function and arguments already failed validation. Do not repeat them. Use the documented bounds, choose another allowed function, or stop with an honest explanation."
    })
}

fn storyboard_confirmation_required(tool: &str) -> Value {
    json!({
        "status": "failed",
        "operation": tool,
        "stage": "confirmation",
        "code": "storyboard_confirmation_required",
        "retryable": true,
        "facts": ["A new storyboard is ready for user review and has not been confirmed."],
        "recovery": "Wait for the user to confirm the storyboard in a later turn before creating or editing a timeline.",
        "responseInstruction": "Summarize that the storyboard is ready for review. Do not claim a timeline or preview was created."
    })
}

fn native_task_cancelled(connection: &Connection, agent_task_id: &str) -> bool {
    connection
        .query_row(
            "SELECT status FROM agent_tasks WHERE id = ?1",
            params![agent_task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
        .is_some_and(|status| status == "cancelled")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{PreviewQualityReport, PreviewResult};
    use crate::provider::{model_turn_from_responses, ModelOutputItem};

    const HELLO: &str = include_str!("../../tests/fixtures/native_loop_hello.v1.json");
    const LIST_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_list_assets_call.v1.json");
    const LIST_REPLY: &str =
        include_str!("../../tests/fixtures/native_loop_list_assets_reply.v1.json");
    const FAILURE_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_failure_call.v1.json");
    const FAILURE_REPLY: &str =
        include_str!("../../tests/fixtures/native_loop_failure_reply.v1.json");
    const RENDER_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_render_preview_call.v1.json");
    const RENDER_REPLY: &str =
        include_str!("../../tests/fixtures/native_loop_render_preview_reply.v1.json");
    const RENDER_FAILURE_REPLY: &str =
        include_str!("../../tests/fixtures/native_loop_render_preview_failure_reply.v1.json");
    const MAIN_CHAIN_ANALYSIS_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_main_chain_analysis_call.v1.json");
    const MAIN_CHAIN_STORYBOARD_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_main_chain_storyboard_call.v1.json");
    const MAIN_CHAIN_TIMELINE_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_main_chain_timeline_call.v1.json");
    const MAIN_CHAIN_CONFIRMATION_REPLY: &str =
        include_str!("../../tests/fixtures/native_loop_main_chain_confirmation_reply.v1.json");
    const MAIN_CHAIN_FINAL_REPLY: &str =
        include_str!("../../tests/fixtures/native_loop_main_chain_final_reply.v1.json");
    const COMPOSITE_OBSERVE_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_composite_observe_call.v1.json");
    const COMPOSITE_STORYBOARD_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_composite_storyboard_call.v1.json");
    const COMPOSITE_TIMELINE_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_composite_timeline_call.v1.json");
    const COMPOSITE_TEXT_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_composite_text_call.v1.json");
    const COMPOSITE_PREVIEW_CALL: &str =
        include_str!("../../tests/fixtures/native_loop_composite_preview_call.v1.json");
    const COMPOSITE_FINAL_REPLY: &str =
        include_str!("../../tests/fixtures/native_loop_composite_final_reply.v1.json");

    fn fixture_driver(
        fixtures: Vec<&'static str>,
        execute_result: Value,
    ) -> (String, Vec<Value>, Vec<String>) {
        let mut responses = fixtures.into_iter();
        let mut requests = Vec::new();
        let mut calls = Vec::new();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "fixture request"}]
        })];
        let mut respond = |payload: &Value, _timeout: Duration| {
            requests.push(payload.clone());
            Ok::<_, String>(responses.next().expect("fixture response").to_owned())
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            calls.push(call.name.clone());
            Ok::<_, String>(execute_result.clone())
        };
        let message = drive_native_loop(
            &mut input,
            false,
            false,
            &RequestToolPolicy::default(),
            &mut NativeRunReceipt::default(),
            "fixture request",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("fixture loop");
        drop(execute);
        (message, requests, calls)
    }

    fn loadable_names(payload: &Value) -> std::collections::HashSet<&str> {
        payload["tools"]
            .as_array()
            .and_then(|tools| tools.iter().find(|tool| tool["name"] == LOAD_TOOLS))
            .and_then(|tool| {
                tool.pointer("/parameters/properties/toolNames/items/enum")
                    .and_then(Value::as_array)
            })
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    }

    #[test]
    fn ordinary_question_returns_message_without_tool_call() {
        let (message, requests, calls) = fixture_driver(vec![HELLO], json!({}));
        assert_eq!(message, "你好！有什么我可以帮你查看的吗？");
        assert!(calls.is_empty());
        assert_eq!(requests[0]["parallel_tool_calls"], false);
        assert_eq!(requests[0]["store"], false);
        assert_eq!(requests[0]["tools"].as_array().map(Vec::len), Some(1));
        assert_eq!(requests[0]["tools"][0]["name"], LOAD_TOOLS);
    }

    #[test]
    fn load_tools_replaces_the_visible_business_tool_set() {
        let function_call = |call_id: &str, name: &str, arguments: &str| {
            json!({
                "id": format!("response-{call_id}"),
                "output": [{
                    "type": "function_call",
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments
                }]
            })
            .to_string()
        };
        let mut responses = vec![
            function_call(
                "load-one",
                LOAD_TOOLS,
                r#"{"toolNames":["list_assets","read_logs"]}"#,
            ),
            function_call("list", "list_assets", "{}"),
            function_call("load-two", LOAD_TOOLS, r#"{"toolNames":["get_timeline"]}"#),
            HELLO.to_owned(),
        ]
        .into_iter();
        let mut requests = Vec::new();
        let mut input = vec![
            json!({"role":"user","content":[{"type":"input_text","text":"读取日志并检查素材"}]}),
        ];
        let mut respond = |payload: &Value, _timeout: Duration| {
            requests.push(payload.clone());
            Ok::<_, String>(responses.next().expect("dynamic response"))
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            if call.name == LOAD_TOOLS {
                let arguments: Value = serde_json::from_str(&call.arguments).expect("load args");
                Ok::<_, String>(json!({
                    "tool": LOAD_TOOLS,
                    "status": "ok",
                    "loadedTools": arguments["toolNames"]
                }))
            } else {
                Ok(json!({"tool":call.name,"status":"ok"}))
            }
        };
        drive_native_loop(
            &mut input,
            false,
            false,
            &RequestToolPolicy::from_request("读取日志并检查素材"),
            &mut NativeRunReceipt::default(),
            "读取日志并检查素材",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("dynamic tool loop");

        let visible = |payload: &Value| {
            payload["tools"]
                .as_array()
                .expect("tools")
                .iter()
                .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
                .collect::<std::collections::HashSet<String>>()
        };
        assert_eq!(
            visible(&requests[0]),
            [LOAD_TOOLS.to_owned()].into_iter().collect()
        );
        assert_eq!(
            visible(&requests[1]),
            [
                LOAD_TOOLS.to_owned(),
                "list_assets".to_owned(),
                READ_LOGS.to_owned(),
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(visible(&requests[2]), visible(&requests[1]));
        assert_eq!(
            visible(&requests[3]),
            [LOAD_TOOLS.to_owned(), "get_timeline".to_owned()]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn newly_loaded_tools_cannot_execute_in_the_same_provider_response() {
        let response = json!({
            "id": "response-multi-call",
            "output": [
                {
                    "type": "function_call",
                    "call_id": "load-call",
                    "name": LOAD_TOOLS,
                    "arguments": r#"{"toolNames":["list_assets"]}"#
                },
                {
                    "type": "function_call",
                    "call_id": "hidden-call",
                    "name": "list_assets",
                    "arguments": "{}"
                }
            ]
        })
        .to_string();
        let mut responses = vec![response.as_str(), HELLO].into_iter();
        let mut input =
            vec![json!({"role":"user","content":[{"type":"input_text","text":"检查素材"}]})];
        let mut executed = Vec::new();
        let mut respond = |_payload: &Value, _timeout: Duration| {
            Ok::<_, String>(responses.next().expect("provider response").to_owned())
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            executed.push(call.name.clone());
            Ok::<_, String>(json!({
                "tool": LOAD_TOOLS,
                "status": "ok",
                "loadedTools": ["list_assets"]
            }))
        };

        drive_native_loop(
            &mut input,
            false,
            false,
            &RequestToolPolicy::from_request("检查素材"),
            &mut NativeRunReceipt::default(),
            "检查素材",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("reject hidden call and continue");
        drop(execute);

        assert_eq!(executed, [LOAD_TOOLS]);
        assert!(input.iter().any(|item| {
            item["call_id"] == "hidden-call"
                && item["output"]
                    .as_str()
                    .is_some_and(|output| output.contains("tool_not_loaded"))
        }));
    }

    #[test]
    fn one_model_turn_with_two_function_calls_uses_distinct_tool_step_numbers() {
        let call = json!({
            "id": "resp_two_tools",
            "output": [
                {
                    "type": "function_call",
                    "call_id": "call_get_timeline",
                    "name": "get_timeline",
                    "arguments": "{\"timelineVersionId\":null}"
                },
                {
                    "type": "function_call",
                    "call_id": "call_list_voices",
                    "name": "list_voices",
                    "arguments": "{}"
                }
            ]
        })
        .to_string();
        let mut responses = vec![call, HELLO.to_owned()].into_iter();
        let mut steps = Vec::new();
        let mut names = Vec::new();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "用这个文案生成配音 Hello factory."}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| {
            Ok::<_, String>(responses.next().expect("two-tool response"))
        };
        let mut execute = |call: &FunctionCall, step_number: usize| {
            names.push(call.name.clone());
            steps.push(step_number);
            Ok::<_, String>(json!({"tool": call.name, "status": "ok"}))
        };
        let message = drive_native_loop(
            &mut input,
            false,
            false,
            &RequestToolPolicy::from_request("用这个文案生成配音 Hello factory."),
            &mut NativeRunReceipt::default(),
            "用这个文案生成配音 Hello factory.",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("two tools in one turn");
        assert_eq!(names, ["get_timeline", "list_voices"]);
        assert_eq!(steps, [1, 2]);
        assert!(message.contains("你好") || !message.trim().is_empty());
    }

    #[test]
    fn ordinary_chat_receives_the_complete_tool_directory() {
        let (_message, requests, _calls) = fixture_driver_with_policy(
            "你好",
            vec![HELLO],
            json!({}),
            RequestToolPolicy::from_request("你好"),
        );
        let names = loadable_names(&requests[0]);
        assert!(names.contains("list_assets"));
        for name in [
            "request_asset_analysis",
            "generate_storyboard",
            "create_timeline_draft",
            "replace_clips",
            "change_clip_duration",
            "reorder_clips",
            "replace_text_tracks",
            "replace_music_tracks",
            "render_preview",
        ] {
            assert!(names.contains(name), "ordinary chat omitted {name}");
        }
        for name in [
            "download_music",
            "use_online_music",
            "synthesize_voiceover",
            "create_jianying_draft",
        ] {
            assert!(names.contains(name), "ordinary chat omitted {name}");
        }
    }

    #[test]
    fn explicit_read_only_request_keeps_directory_complete_but_execution_closed() {
        for request in [
            "Please inspect these assets and do not modify anything.",
            "Please inspect these assets only.",
            "Only inspect these assets.",
            "Please only inspect these assets.",
        ] {
            let (_message, requests, _calls) = fixture_driver_with_policy(
                request,
                vec![HELLO],
                json!({}),
                RequestToolPolicy::from_request(request),
            );
            let names = loadable_names(&requests[0]);
            assert!(names.contains("generate_storyboard"));
            assert!(!native_tool_call_allowed(
                "generate_storyboard",
                &RequestToolPolicy::from_request(request),
                false
            ));
        }
    }

    #[test]
    fn local_edit_requests_expose_the_full_reversible_toolset() {
        for request in [
            "分析这些素材并生成 storyboard",
            "Analyze these assets and generate a storyboard",
        ] {
            let (_message, requests, _calls) = fixture_driver_with_policy(
                request,
                vec![MAIN_CHAIN_ANALYSIS_CALL, HELLO],
                json!({
                    "tool": "request_asset_analysis",
                    "status": "queued",
                    "queuedCount": 1
                }),
                RequestToolPolicy::from_request(request),
            );
            let names = loadable_names(&requests[0]);
            assert!(names.contains("request_asset_analysis"));
            assert!(names.contains("generate_storyboard"));
            for name in [
                "create_timeline_draft",
                "replace_clips",
                "change_clip_duration",
                "reorder_clips",
                "replace_text_tracks",
                "replace_music_tracks",
                "render_preview",
            ] {
                assert!(names.contains(name), "request omitted reversible {name}");
            }
        }
    }

    #[test]
    fn local_write_calls_need_no_keyword_but_explicit_denials_are_rechecked() {
        let policy = RequestToolPolicy::from_request("你好");
        assert!(native_tool_call_allowed(
            "generate_storyboard",
            &policy,
            false
        ));
        assert!(native_tool_call_allowed("replace_clips", &policy, false));

        let policy = RequestToolPolicy::from_request("Don't only inspect; edit the clips.");
        assert!(!policy.read_only);
        assert!(native_tool_call_allowed("replace_clips", &policy, false));

        for (request, tool) in [
            ("不要做 30 秒剪辑", "create_timeline_draft"),
            ("Do not add subtitles", "replace_text_tracks"),
            ("不要替换片段", "replace_clips"),
            ("不要调整片段时长", "change_clip_duration"),
            ("不要重排片段", "reorder_clips"),
            ("不要替换背景音乐", "replace_music_tracks"),
        ] {
            let policy = RequestToolPolicy::from_request(request);
            assert!(!native_tool_call_allowed(tool, &policy, false));
        }
    }

    #[test]
    fn diagnostic_logs_are_normally_available_but_respect_explicit_denial() {
        let ordinary = RequestToolPolicy::from_request("检查当前项目");
        assert!(native_tool_call_allowed(READ_LOGS, &ordinary, false));

        let authorized = RequestToolPolicy::from_request("读取运行日志");
        assert!(native_tool_call_allowed(READ_LOGS, &authorized, false));

        let denied = RequestToolPolicy::from_request("不要读取日志");
        assert!(!native_tool_call_allowed(READ_LOGS, &denied, false));
    }

    #[test]
    fn project_fact_question_executes_read_tool_then_replies() {
        let (message, requests, calls) = fixture_driver(
            vec![LIST_CALL, LIST_REPLY],
            json!({
                "tool": "list_assets",
                "status": "ok",
                "assets": [{"id": "asset-1", "analysisStatus": "ready"}]
            }),
        );
        assert_eq!(message, "项目中有 1 个素材。");
        assert_eq!(calls, ["list_assets"]);
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["input"].as_array().map(Vec::len), Some(4));
        assert!(requests[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["type"] == "function_call" && item["name"] == "list_assets" }));
        assert!(requests[1]["input"].as_array().unwrap().iter().any(|item| {
            item["type"] == "function_call_output" && item["call_id"] == "call_list_assets"
        }));
    }

    #[test]
    fn successful_list_assets_observation_opens_project_fact_gate() {
        let mut responses = vec![LIST_CALL, LIST_REPLY].into_iter();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "当前项目有多少素材？"}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| {
            Ok::<_, String>(responses.next().expect("observation fixture").to_owned())
        };
        let mut execute = |_call: &FunctionCall, _step: usize| {
            Ok::<_, String>(json!({
                "tool": "list_assets",
                "status": "ok",
                "assets": [{"id": "asset-1"}]
            }))
        };
        let mut receipt = NativeRunReceipt::default();
        let message = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目有多少素材？"),
            &mut receipt,
            "当前项目有多少素材？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("successful observation should allow final reply");
        assert_eq!(message, "项目中有 1 个素材。");
        assert!(receipt.requires_project_observation);
        assert!(receipt.successful_observation_this_turn);
        assert!(receipt.successful_tool_call);
    }

    #[test]
    fn diagnostic_log_read_does_not_open_the_project_fact_gate() {
        let read_logs_call = json!({
            "id":"read-logs-response",
            "output":[{
                "type":"function_call",
                "call_id":"read-logs-call",
                "name":READ_LOGS,
                "arguments":"{\"startLine\":null,\"endLine\":null}"
            }]
        })
        .to_string();
        let mut responses = vec![read_logs_call.as_str(), HELLO, LIST_CALL, LIST_REPLY].into_iter();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "当前项目有多少素材？"}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| {
            Ok::<_, String>(
                responses
                    .next()
                    .expect("diagnostic observation response")
                    .to_owned(),
            )
        };
        let mut calls = Vec::new();
        let mut execute = |call: &FunctionCall, _step: usize| {
            calls.push(call.name.clone());
            Ok::<_, String>(if call.name == READ_LOGS {
                json!({"tool":READ_LOGS,"status":"ok","lines":[]})
            } else {
                json!({"tool":"list_assets","status":"ok","assets":[]})
            })
        };
        let mut receipt = NativeRunReceipt::default();
        let message = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目有多少素材？"),
            &mut receipt,
            "当前项目有多少素材？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("project fact loop");
        assert_eq!(message, "项目中有 1 个素材。");
        assert_eq!(calls, [READ_LOGS, "list_assets"]);
        assert!(receipt.successful_observation_this_turn);
        assert!(!receipt.observation_sources.contains(READ_LOGS));
        assert!(receipt.observation_sources.contains("list_assets"));
    }

    #[test]
    fn failed_observation_does_not_satisfy_project_fact_gate() {
        let mut response_count = 0;
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "当前项目有多少素材？"}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| {
            response_count += 1;
            if response_count == 1 {
                Ok::<_, String>(FAILURE_CALL.to_owned())
            } else {
                Ok::<_, String>(FAILURE_REPLY.to_owned())
            }
        };
        let mut execute = |_call: &FunctionCall, _step: usize| {
            Ok::<_, String>(json!({
                "tool": "list_assets",
                "status": "failed",
                "code": "asset_store_unavailable",
                "retryable": true,
                "responseInstruction": "请稍后重试。"
            }))
        };
        let mut receipt = NativeRunReceipt::default();
        let result = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目有多少素材？"),
            &mut receipt,
            "当前项目有多少素材？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        );
        assert_eq!(
            result,
            Ok("当前无法读取素材状态，我可以稍后再试。".to_owned())
        );
        assert!(!receipt.successful_observation_this_turn);
        assert!(receipt.failed_tools.contains("list_assets"));
    }

    #[test]
    fn generate_storyboard_does_not_satisfy_project_fact_gate() {
        let mut response_count = 0;
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "当前项目的 storyboard 是什么？"}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| {
            response_count += 1;
            if response_count == 1 {
                Ok::<_, String>(MAIN_CHAIN_STORYBOARD_CALL.to_owned())
            } else {
                Ok::<_, String>(MAIN_CHAIN_CONFIRMATION_REPLY.to_owned())
            }
        };
        let mut execute = |_call: &FunctionCall, _step: usize| {
            Ok::<_, String>(json!({
                "tool": "generate_storyboard",
                "status": "needs_confirmation",
                "storyboardVersionId": "storyboard-1"
            }))
        };
        let result = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目的 storyboard 是什么？"),
            &mut NativeRunReceipt::default(),
            "当前项目的 storyboard 是什么？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        );
        assert_eq!(result, Err("native_tool_loop_max_steps".to_owned()));
    }

    #[test]
    fn model_claim_without_tool_ends_loop_but_receipt_cannot_complete_request() {
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "生成 storyboard"}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| {
            Ok::<_, String>(
                json!({
                    "id": "response-claim",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "Storyboard 已生成。"}]
                    }]
                })
                .to_string(),
            )
        };
        let mut execute =
            |_call: &FunctionCall, _step: usize| panic!("the model claim must not execute a tool");
        let mut receipt = NativeRunReceipt::default();
        let result = drive_native_loop(
            &mut input,
            false,
            false,
            &RequestToolPolicy::from_request("生成 storyboard"),
            &mut receipt,
            "生成 storyboard",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        );
        assert_eq!(result, Ok("Storyboard 已生成。".to_owned()));
        let (_result, status) = finish_native_result("task-1", result, None, &receipt)
            .expect("receipt must reject an unverified completion claim");
        assert_eq!(status, AgentLoopTerminalStatus::Failed);
    }

    #[test]
    fn composite_main_chain_fixture_runs_analysis_storyboard_then_timeline() {
        let mut pre_confirmation_responses = vec![
            MAIN_CHAIN_ANALYSIS_CALL,
            MAIN_CHAIN_STORYBOARD_CALL,
            MAIN_CHAIN_CONFIRMATION_REPLY,
        ]
        .into_iter();
        let mut requests = Vec::new();
        let calls = std::cell::RefCell::new(Vec::new());
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "分析素材并生成 storyboard，最后创建时间线"}]
        })];
        let mut respond = |payload: &Value, _timeout: Duration| {
            requests.push(payload.clone());
            Ok::<_, String>(
                pre_confirmation_responses
                    .next()
                    .expect("composite fixture response")
                    .to_owned(),
            )
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            calls.borrow_mut().push(call.name.clone());
            let result = match call.name.as_str() {
                "request_asset_analysis" => {
                    json!({"tool":"request_asset_analysis","status":"queued","queuedCount":1})
                }
                "generate_storyboard" => json!({
                    "tool":"generate_storyboard",
                    "status":"needs_confirmation",
                    "storyboardVersionId":"storyboard-1"
                }),
                "create_timeline_draft" => json!({
                    "tool":"create_timeline_draft",
                    "status":"ok",
                    "timelineVersionId":"timeline-1"
                }),
                _ => unreachable!("unexpected composite tool"),
            };
            prepare_native_tool_result(&call.name, result)
                .map_err(|_| "unsafe composite fixture result".to_owned())
        };
        let message = drive_native_loop(
            &mut input,
            false,
            false,
            &RequestToolPolicy::from_request("分析素材并生成 storyboard，最后创建时间线"),
            &mut NativeRunReceipt::default(),
            "分析素材并生成 storyboard，最后创建时间线",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("composite native loop");
        assert_eq!(
            message,
            "素材分析已请求，Storyboard 已生成，请确认后再创建时间线。"
        );
        assert_eq!(
            calls.borrow().as_slice(),
            ["request_asset_analysis", "generate_storyboard"]
        );
        assert_eq!(requests.len(), 3);
        let confirmation_input = requests[2]["input"].as_array().expect("confirmation input");
        for call_id in ["call_request_asset_analysis", "call_generate_storyboard"] {
            assert!(confirmation_input
                .iter()
                .any(|item| item["type"] == "function_call_output" && item["call_id"] == call_id));
        }
        assert!(requests[2]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .all(|tool| super::super::policy::OBSERVATION_TOOLS
                .contains(&tool["name"].as_str().unwrap())));

        let mut post_confirmation_responses =
            vec![MAIN_CHAIN_TIMELINE_CALL, MAIN_CHAIN_FINAL_REPLY].into_iter();
        let mut confirmed_input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "我确认这个 storyboard，请创建时间线"}]
        })];
        let mut respond_after_confirmation = |payload: &Value, _timeout: Duration| {
            requests.push(payload.clone());
            Ok::<_, String>(
                post_confirmation_responses
                    .next()
                    .expect("post-confirmation fixture response")
                    .to_owned(),
            )
        };
        let confirmed_message = drive_native_loop(
            &mut confirmed_input,
            false,
            false,
            &RequestToolPolicy::from_request("我确认这个 storyboard，请创建时间线"),
            &mut NativeRunReceipt::default(),
            "我确认这个 storyboard，请创建时间线",
            Instant::now() + Duration::from_secs(5),
            &mut respond_after_confirmation,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("post-confirmation native loop");
        assert_eq!(
            confirmed_message,
            "素材已分析，Storyboard 已生成，并已创建时间线。"
        );
        assert_eq!(
            calls.borrow().last().map(String::as_str),
            Some("create_timeline_draft")
        );
        let final_input = requests.last().unwrap()["input"]
            .as_array()
            .expect("final input");
        for call_id in ["call_create_timeline_draft"] {
            assert!(final_input
                .iter()
                .any(|item| item["type"] == "function_call_output" && item["call_id"] == call_id));
        }
    }

    #[test]
    fn composite_edit_fixture_crosses_confirmation_before_timeline_text_and_preview() {
        let request = "检查素材，做 30 秒剪辑，加字幕并生成预览。";
        let policy = RequestToolPolicy::from_request(request);
        let mut responses = vec![
            COMPOSITE_OBSERVE_CALL,
            COMPOSITE_STORYBOARD_CALL,
            MAIN_CHAIN_CONFIRMATION_REPLY,
        ]
        .into_iter();
        let mut requests = Vec::new();
        let calls = std::cell::RefCell::new(Vec::new());
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": request}]
        })];
        let mut respond = |payload: &Value, _timeout: Duration| {
            requests.push(payload.clone());
            Ok::<_, String>(responses.next().expect("composite response").to_owned())
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            calls.borrow_mut().push(call.name.clone());
            Ok::<_, String>(match call.name.as_str() {
                "list_assets" => json!({"tool":"list_assets","status":"ok","assets":[]}),
                "generate_storyboard" => {
                    json!({"tool":"generate_storyboard","status":"needs_confirmation","storyboardVersionId":"storyboard-1"})
                }
                _ => unreachable!("unexpected composite call"),
            })
        };
        let mut receipt = NativeRunReceipt::default();
        let message = drive_native_loop(
            &mut input,
            false,
            true,
            &policy,
            &mut receipt,
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("composite loop");
        assert_eq!(
            message,
            "素材分析已请求，Storyboard 已生成，请确认后再创建时间线。"
        );
        assert_eq!(
            calls.borrow().as_slice(),
            ["list_assets", "generate_storyboard"]
        );
        assert_eq!(requests.len(), 3);
        for payload in requests.iter().take(2) {
            let names = loadable_names(payload);
            for required in [
                "generate_storyboard",
                "create_timeline_draft",
                "replace_text_tracks",
                "render_preview",
            ] {
                assert!(
                    names.contains(required),
                    "missing composite tool {required}"
                );
            }
        }
        assert!(receipt.needs_confirmation);
        assert!(receipt.successful_observation_this_turn);

        let confirmed_request = "我确认这个 storyboard；创建时间线，加字幕并生成预览。";
        let confirmed_policy = RequestToolPolicy::from_request(confirmed_request);
        let mut confirmed_responses = vec![
            COMPOSITE_TIMELINE_CALL,
            COMPOSITE_TEXT_CALL,
            COMPOSITE_PREVIEW_CALL,
            COMPOSITE_FINAL_REPLY,
        ]
        .into_iter();
        let mut confirmed_input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": confirmed_request}]
        })];
        let mut respond_confirmed = |_payload: &Value, _timeout: Duration| {
            Ok::<_, String>(
                confirmed_responses
                    .next()
                    .expect("confirmed response")
                    .to_owned(),
            )
        };
        let mut execute_confirmed = |call: &FunctionCall, _step: usize| {
            calls.borrow_mut().push(call.name.clone());
            Ok::<_, String>(match call.name.as_str() {
                "create_timeline_draft" => {
                    json!({"tool":"create_timeline_draft","status":"ok","timelineVersionId":"timeline-1"})
                }
                "replace_text_tracks" => {
                    json!({"tool":"replace_text_tracks","status":"ok","timelineVersionId":"timeline-2","qualityWarnings":[]})
                }
                "render_preview" => {
                    json!({"tool":"render_preview","status":"ok","artifact":{"type":"preview","timelineVersionId":"timeline-2"}})
                }
                _ => unreachable!("unexpected confirmed composite call"),
            })
        };
        let mut confirmed_receipt = NativeRunReceipt::default();
        let confirmed_message = drive_native_loop(
            &mut confirmed_input,
            false,
            false,
            &confirmed_policy,
            &mut confirmed_receipt,
            confirmed_request,
            Instant::now() + Duration::from_secs(5),
            &mut respond_confirmed,
            &mut execute_confirmed,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("confirmed composite loop");
        assert_eq!(
            confirmed_message,
            "已检查素材并完成 30 秒剪辑、字幕和预览。"
        );
        assert_eq!(
            calls.borrow().as_slice(),
            [
                "list_assets",
                "generate_storyboard",
                "create_timeline_draft",
                "replace_text_tracks",
                "render_preview"
            ]
        );
        assert!(confirmed_receipt
            .successful_write_tools
            .contains("render_preview"));
    }

    #[test]
    fn timeline_edit_after_preview_invalidates_preview_completion_receipt() {
        let request = "生成预览并替换字幕";
        let policy = RequestToolPolicy::from_request(request);
        let preview_call = json!({
            "id": "resp_preview_first",
            "output": [{
                "type": "function_call",
                "call_id": "call_preview_first",
                "name": "render_preview",
                "arguments": "{\"timelineVersionId\":null}"
            }]
        })
        .to_string();
        let edit_call = json!({
            "id": "resp_edit_after_preview",
            "output": [{
                "type": "function_call",
                "call_id": "call_edit_after_preview",
                "name": "replace_text_tracks",
                "arguments": "{}"
            }]
        })
        .to_string();
        let mut responses = vec![preview_call, edit_call, HELLO.to_owned()].into_iter();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": request}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| {
            Ok::<_, String>(responses.next().expect("preview invalidation response"))
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            Ok::<_, String>(match call.name.as_str() {
                "render_preview" => json!({
                    "tool":"render_preview",
                    "status":"ok",
                    "artifact":{"type":"preview","timelineVersionId":"timeline-1"}
                }),
                "replace_text_tracks" => json!({
                    "tool":"replace_text_tracks",
                    "status":"ok",
                    "timelineVersionId":"timeline-2",
                    "qualityWarnings":[]
                }),
                _ => unreachable!("unexpected preview invalidation tool"),
            })
        };
        let mut receipt = NativeRunReceipt::default();
        let message = drive_native_loop(
            &mut input,
            false,
            false,
            &policy,
            &mut receipt,
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("preview invalidation loop");

        assert_eq!(message, "你好！有什么我可以帮你查看的吗？");
        assert!(!receipt.successful_write_tools.contains("render_preview"));
        assert!(receipt
            .successful_write_tools
            .contains("replace_text_tracks"));
        assert!(receipt
            .unverified_requested_write_tools
            .contains("render_preview"));
        let (_, status) = finish_native_result("task-1", Ok(message), None, &receipt)
            .expect("finish preview invalidation result");
        assert_eq!(status, AgentLoopTerminalStatus::PartiallyCompleted);
    }

    #[test]
    fn natural_language_ends_the_turn_without_fixed_goal_correction() {
        let request = "生成 storyboard";
        let policy = RequestToolPolicy::from_request(request);
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": request}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| Ok::<_, String>(HELLO.to_owned());
        let mut execute = |_call: &FunctionCall, _step: usize| {
            unreachable!("natural language must end without a tool call")
        };
        let mut receipt = NativeRunReceipt::default();
        let result = drive_native_loop(
            &mut input,
            false,
            false,
            &policy,
            &mut receipt,
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        );
        assert_eq!(result, Ok("你好！有什么我可以帮你查看的吗？".to_owned()));
        assert!(!receipt.successful_tool_call);
    }

    #[test]
    fn composite_request_with_only_one_verified_write_is_partially_completed() {
        let request = "生成 storyboard 并创建时间线";
        let policy = RequestToolPolicy::from_request(request);
        let mut responses = vec![MAIN_CHAIN_STORYBOARD_CALL, HELLO].into_iter();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": request}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| {
            Ok::<_, String>(
                responses
                    .next()
                    .expect("partial composite response")
                    .to_owned(),
            )
        };
        let mut execute = |_call: &FunctionCall, _step: usize| {
            Ok::<_, String>(json!({
                "tool": "generate_storyboard",
                "status": "ok",
                "storyboardVersionId": "storyboard-1"
            }))
        };
        let mut receipt = NativeRunReceipt::default();
        let message = drive_native_loop(
            &mut input,
            false,
            false,
            &policy,
            &mut receipt,
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("natural language ends the composite loop");
        let (_result, status) = finish_native_result("task-1", Ok(message), None, &receipt)
            .expect("receipt determines the truthful terminal status");
        assert_eq!(status, AgentLoopTerminalStatus::PartiallyCompleted);
    }

    #[test]
    fn identical_invalid_arguments_are_canonicalized_then_rejected_and_stopped() {
        let first = FunctionCall {
            call_id: "call-1".to_owned(),
            name: "search_asset_segments".to_owned(),
            arguments: "{\"query\":\"供应链\",\"assetId\":null,\"offset\":0,\"limit\":30}"
                .to_owned(),
            raw: json!({}),
        };
        let reordered = FunctionCall {
            call_id: "call-2".to_owned(),
            name: first.name.clone(),
            arguments: "{ \"limit\": 30, \"offset\": 0, \"assetId\": null, \"query\": \"供应链\" }"
                .to_owned(),
            raw: json!({}),
        };
        let corrected = FunctionCall {
            call_id: "call-3".to_owned(),
            name: first.name.clone(),
            arguments: "{\"query\":\"供应链\",\"assetId\":null,\"offset\":0,\"limit\":20}"
                .to_owned(),
            raw: json!({}),
        };
        let mut guard = InvalidCallGuard::default();
        assert_eq!(guard.before_call(&first), InvalidCallDecision::Execute);
        guard.record_result(&first, &invalid_arguments());
        assert_eq!(
            native_call_signature(&first),
            native_call_signature(&reordered)
        );
        assert_eq!(guard.before_call(&reordered), InvalidCallDecision::Reject);
        assert_eq!(guard.before_call(&first), InvalidCallDecision::Stop);
        assert_eq!(guard.before_call(&corrected), InvalidCallDecision::Execute);
    }

    #[test]
    fn recovered_tool_failure_does_not_force_partial_completion() {
        let request = "生成预览";
        let policy = RequestToolPolicy::from_request(request);
        let adjusted_render_call = RENDER_CALL.replace(
            "{\\\"timelineVersionId\\\":null}",
            "{\\\"timelineVersionId\\\":\\\"timeline-1\\\"}",
        );
        let mut responses = vec![
            RENDER_CALL.to_owned(),
            adjusted_render_call,
            RENDER_REPLY.to_owned(),
        ]
        .into_iter();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": request}]
        })];
        let mut attempts = 0;
        let mut respond = |_payload: &Value, _timeout: Duration| {
            Ok::<_, String>(responses.next().expect("recovery response"))
        };
        let mut execute = |_call: &FunctionCall, _step: usize| {
            attempts += 1;
            Ok::<_, String>(if attempts == 1 {
                json!({"tool":"render_preview","status":"failed","code":"invalid_arguments"})
            } else {
                json!({"tool":"render_preview","status":"ok","artifact":{"type":"preview","timelineVersionId":"timeline-1"}})
            })
        };
        let mut receipt = NativeRunReceipt::default();
        let message = drive_native_loop(
            &mut input,
            false,
            false,
            &policy,
            &mut receipt,
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        );
        let (_result, status) =
            finish_native_result("task-1", message, None, &receipt).expect("recovered receipt");
        assert_eq!(status, AgentLoopTerminalStatus::Completed);
        assert_eq!(attempts, 2);
        assert!(receipt.failed_tools.is_empty());
    }

    #[test]
    fn merging_verified_outcomes_preserves_earlier_artifacts() {
        let storyboard = StoryboardVersion {
            id: "storyboard-1".to_owned(),
            project_id: "project-1".to_owned(),
            editing_task_id: "task-1".to_owned(),
            version_number: 1,
            brief: "brief".to_owned(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 30_000,
            script_mode: "key_message".to_owned(),
            beats: Vec::new(),
            uncovered_beat_ids: Vec::new(),
            shots: Vec::new(),
            created_at: 1,
        };
        let earlier = AgentEditResult {
            agent_task_id: "task-1".to_owned(),
            message: "storyboard".to_owned(),
            storyboard: Some(storyboard),
            timeline: None,
            preview: None,
            jianying_draft: None,
        };
        let later = AgentEditResult {
            agent_task_id: "task-1".to_owned(),
            message: "observation".to_owned(),
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        };
        let merged = merge_native_outcomes(Some(earlier), later, "get_edit_status");
        assert!(merged.storyboard.is_some());
        assert_eq!(merged.message, "observation");
    }

    #[test]
    fn step_limit_preserves_real_partial_artifact_from_receipt() {
        let storyboard = StoryboardVersion {
            id: "storyboard-1".to_owned(),
            project_id: "project-1".to_owned(),
            editing_task_id: "task-1".to_owned(),
            version_number: 1,
            brief: "30 second edit".to_owned(),
            title: "Draft".to_owned(),
            summary: "Draft".to_owned(),
            target_duration_ms: 30_000,
            script_mode: "key_message".to_owned(),
            beats: Vec::new(),
            uncovered_beat_ids: Vec::new(),
            shots: Vec::new(),
            created_at: 1,
        };
        let outcome = AgentEditResult {
            agent_task_id: "agent-task-1".to_owned(),
            message: "ignored".to_owned(),
            storyboard: Some(storyboard),
            timeline: None,
            preview: None,
            jianying_draft: None,
        };
        let receipt = NativeRunReceipt {
            tool_called: true,
            successful_tool_call: true,
            ..NativeRunReceipt::default()
        };
        let (result, status) = finish_native_result(
            "agent-task-1",
            Err("native_tool_loop_max_steps".to_owned()),
            Some(outcome),
            &receipt,
        )
        .expect("partial receipt result");
        assert_eq!(status, AgentLoopTerminalStatus::PartiallyCompleted);
        assert!(result.storyboard.is_some());
        assert!(result.message.contains("步骤上限"));
    }

    #[test]
    fn safe_tool_error_is_returned_and_model_can_explain() {
        let failure = json!({
            "status": "failed",
            "operation": "list_assets",
            "code": "unavailable_media",
            "retryable": true,
            "responseInstruction": "Explain only the supplied facts."
        });
        let (message, requests, calls) = fixture_driver(vec![FAILURE_CALL, FAILURE_REPLY], failure);
        assert_eq!(message, "当前无法读取素材状态，我可以稍后再试。");
        assert_eq!(calls, ["list_assets"]);
        let output = requests[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .expect("function output");
        assert!(output["output"]
            .as_str()
            .unwrap()
            .contains("unavailable_media"));
        assert!(!output["output"].as_str().unwrap().contains("C:\\"));
    }

    #[test]
    fn transient_provider_failure_after_tool_output_retries_without_reexecuting_tool() {
        let mut response_attempts = 0;
        let mut calls = Vec::new();
        let mut observations = Vec::new();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "当前项目有多少素材？"}]
        })];
        let mut respond_once = |_payload: &Value, _timeout: Duration| {
            response_attempts += 1;
            match response_attempts {
                1 => Ok::<_, String>(LIST_CALL.to_owned()),
                2 => Err(
                    "自定义 API 不可用（https://sensitive.example/v1，模型 private-model）:HTTP 429"
                        .to_owned(),
                ),
                3 => Ok(LIST_REPLY.to_owned()),
                _ => panic!("unexpected provider attempt"),
            }
        };
        let mut respond = |payload: &Value, timeout: Duration| {
            let mut not_cancelled = || false;
            request_native_model_with_retry(
                payload,
                timeout,
                Duration::ZERO,
                &mut respond_once,
                &mut not_cancelled,
                &mut |observation| observations.push(observation.clone()),
            )
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            calls.push(call.name.clone());
            Ok::<_, String>(json!({
                "tool": "list_assets",
                "status": "ok",
                "result": {"total": 1, "items": []}
            }))
        };

        let message = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目有多少素材？"),
            &mut NativeRunReceipt::default(),
            "当前项目有多少素材？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("transient provider failure should recover");
        drop(execute);
        drop(respond);

        assert_eq!(message, "项目中有 1 个素材。");
        assert_eq!(response_attempts, 3);
        assert_eq!(calls, ["list_assets"]);
        assert!(matches!(
            observations.as_slice(),
            [
                NativeModelRequestObservation::RetryScheduled { code, attempt: 1 },
                NativeModelRequestObservation::Recovered { code: recovered, attempts: 2 }
            ] if code == "provider_http_429" && recovered == "provider_http_429"
        ));
    }

    #[test]
    fn empty_provider_response_after_tool_output_retries_without_reexecuting_tool() {
        let mut response_attempts = 0;
        let mut calls = Vec::new();
        let mut observations = Vec::new();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "当前项目有多少素材？"}]
        })];
        let mut respond_once = |_payload: &Value, _timeout: Duration| {
            response_attempts += 1;
            match response_attempts {
                1 => Ok::<_, String>(LIST_CALL.to_owned()),
                2 => Ok(String::new()),
                3 => Ok(LIST_REPLY.to_owned()),
                _ => panic!("unexpected provider attempt"),
            }
        };
        let mut respond = |payload: &Value, timeout: Duration| {
            let mut not_cancelled = || false;
            request_native_model_with_retry(
                payload,
                timeout,
                Duration::ZERO,
                &mut respond_once,
                &mut not_cancelled,
                &mut |observation| observations.push(observation.clone()),
            )
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            calls.push(call.name.clone());
            Ok::<_, String>(json!({
                "tool": "list_assets",
                "status": "ok",
                "result": {"total": 1, "items": []}
            }))
        };

        let message = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目有多少素材？"),
            &mut NativeRunReceipt::default(),
            "当前项目有多少素材？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("empty provider response should recover");
        drop(execute);
        drop(respond);

        assert_eq!(message, "项目中有 1 个素材。");
        assert_eq!(response_attempts, 3);
        assert_eq!(calls, ["list_assets"]);
        assert!(matches!(
            observations.as_slice(),
            [
                NativeModelRequestObservation::RetryScheduled { code, attempt: 1 },
                NativeModelRequestObservation::Recovered { code: recovered, attempts: 2 }
            ] if code == "provider_empty_response" && recovered == "provider_empty_response"
        ));
    }

    #[test]
    fn permanent_provider_failure_after_tool_output_is_not_retried_or_reexecuted() {
        let mut response_attempts = 0;
        let mut calls = Vec::new();
        let mut observations = Vec::new();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": "当前项目有多少素材？"}]
        })];
        let mut respond_once = |_payload: &Value, _timeout: Duration| {
            response_attempts += 1;
            match response_attempts {
                1 => Ok::<_, String>(LIST_CALL.to_owned()),
                2 => Err(
                    "自定义 API 不可用（https://sensitive.example/v1，模型 private-model）:HTTP 400"
                        .to_owned(),
                ),
                _ => panic!("permanent provider failure must not be retried"),
            }
        };
        let mut respond = |payload: &Value, timeout: Duration| {
            let mut not_cancelled = || false;
            request_native_model_with_retry(
                payload,
                timeout,
                Duration::ZERO,
                &mut respond_once,
                &mut not_cancelled,
                &mut |observation| observations.push(observation.clone()),
            )
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            calls.push(call.name.clone());
            Ok::<_, String>(json!({
                "tool": "list_assets",
                "status": "ok",
                "result": {"total": 1, "items": []}
            }))
        };

        let error = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目有多少素材？"),
            &mut NativeRunReceipt::default(),
            "当前项目有多少素材？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect_err("HTTP 400 should fail without retrying");
        drop(execute);
        drop(respond);

        assert!(error.contains("HTTP 400"));
        assert_eq!(response_attempts, 2);
        assert_eq!(calls, ["list_assets"]);
        assert!(matches!(
            observations.as_slice(),
            [NativeModelRequestObservation::Failed { code, attempts: 1 }]
                if code == "provider_http_400"
        ));
        let (_, diagnostic) = native_model_request_diagnostic(&observations[0]);
        assert!(!diagnostic.contains("sensitive"));
        assert!(!diagnostic.contains("private-model"));
    }

    #[test]
    fn cancellation_during_retry_backoff_stops_before_the_next_provider_attempt() {
        let mut response_attempts = 0;
        let mut cancellation_checks = 0;
        let mut observations = Vec::new();
        let mut respond_once = |_payload: &Value, _timeout: Duration| {
            response_attempts += 1;
            Err::<String, _>("实验性 OAuth 请求失败:HTTP 429".to_owned())
        };
        let mut cancelled = || {
            cancellation_checks += 1;
            cancellation_checks > 1
        };

        let error = request_native_model_with_retry(
            &json!({"input": []}),
            Duration::from_secs(5),
            Duration::from_millis(100),
            &mut respond_once,
            &mut cancelled,
            &mut |observation| observations.push(observation.clone()),
        )
        .expect_err("cancellation must stop retry backoff");

        assert_eq!(error, "native_tool_loop_cancelled");
        assert_eq!(response_attempts, 1);
        assert!(cancellation_checks >= 2);
        assert!(matches!(
            observations.as_slice(),
            [NativeModelRequestObservation::RetryScheduled { code, attempt: 1 }]
                if code == "provider_http_429"
        ));
    }

    #[test]
    fn retryable_network_failure_splits_step_budget_so_later_attempts_can_run() {
        let mut attempt_timeouts = Vec::new();
        let mut observations = Vec::new();
        let step_budget = Duration::from_secs(120);
        let mut respond_once = |_payload: &Value, timeout: Duration| {
            attempt_timeouts.push(timeout);
            Err::<String, _>(
                "自定义 API 不可用（https://sensitive.example/v1，模型 private-model）:网络错误 connection reset"
                    .to_owned(),
            )
        };

        let error = request_native_model_with_retry(
            &json!({"input": []}),
            step_budget,
            Duration::ZERO,
            &mut respond_once,
            &mut || false,
            &mut |observation| observations.push(observation.clone()),
        )
        .expect_err("exhausted retries still fail");

        assert!(error.contains("网络错误"));
        assert_eq!(attempt_timeouts.len(), 3);
        assert!(
            attempt_timeouts
                .iter()
                .all(|timeout| *timeout < step_budget && *timeout >= Duration::from_secs(30)),
            "each hung HTTP attempt must leave time for later retries: {attempt_timeouts:?}"
        );
        assert!(matches!(
            observations.last(),
            Some(NativeModelRequestObservation::Failed { code, attempts: 3 })
                if code == "provider_network"
        ));
    }

    #[test]
    fn native_model_attempt_timeout_keeps_two_thirds_of_a_fresh_step_for_later_tries() {
        let first = native_model_attempt_timeout(Duration::from_secs(120), 1);
        let second = native_model_attempt_timeout(Duration::from_secs(80), 2);
        let last = native_model_attempt_timeout(Duration::from_secs(40), 3);
        assert_eq!(first, Duration::from_secs(40));
        assert_eq!(second, Duration::from_secs(40));
        assert_eq!(last, Duration::from_secs(40));
    }

    #[test]
    fn responses_fixture_keeps_assistant_output_items_for_next_input() {
        let turn = model_turn_from_responses(LIST_CALL).expect("tool fixture");
        assert!(matches!(turn.output[0], ModelOutputItem::Message { .. }));
        assert!(matches!(turn.output[1], ModelOutputItem::FunctionCall(_)));
    }

    #[test]
    fn timeline_question_calls_get_timeline() {
        let (message, _requests, calls) = fixture_driver(
            vec![
                include_str!("../../tests/fixtures/native_loop_timeline_call.v1.json"),
                include_str!("../../tests/fixtures/native_loop_timeline_reply.v1.json"),
            ],
            json!({"tool": "get_timeline", "status": "ok", "timeline": null}),
        );
        assert_eq!(calls, ["get_timeline"]);
        assert_eq!(message, "当前任务还没有时间线。");
    }

    #[test]
    fn preview_request_exposes_render_tool_and_returns_model_summary_after_execution() {
        let policy = RequestToolPolicy::from_request("生成预览");
        let (message, requests, calls) = fixture_driver_with_policy(
            "生成预览",
            vec![RENDER_CALL, RENDER_REPLY],
            json!({
                "tool": "render_preview",
                "status": "ok",
                "artifact": {
                    "type": "preview",
                    "timelineVersionId": "timeline-1",
                    "versionNumber": 2,
                    "qualityCheckCount": 0
                }
            }),
            policy,
        );
        assert_eq!(message, "预览已生成，可以检查节奏和字幕。");
        assert_eq!(calls, ["render_preview"]);
        assert_eq!(requests.len(), 2);
        assert!(loadable_names(&requests[0]).contains("render_preview"));
        assert!(requests[1]["input"].as_array().unwrap().iter().any(|item| {
            item["type"] == "function_call_output" && item["call_id"] == "call_render_preview"
        }));
    }

    #[test]
    fn preview_claim_without_tool_ends_loop_but_receipt_cannot_complete_request() {
        let request = "帮我生成一个预览";
        let policy = RequestToolPolicy::from_request(request);
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": request}]
        })];
        let mut respond = |_payload: &Value, _timeout: Duration| Ok::<_, String>(HELLO.to_owned());
        let mut execute = |_call: &FunctionCall, _step: usize| {
            unreachable!("natural language must finish without forcing a tool call")
        };
        let mut receipt = NativeRunReceipt::default();
        let message = drive_native_loop(
            &mut input,
            false,
            false,
            &policy,
            &mut receipt,
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("natural language ends the native loop");
        let (_result, status) = finish_native_result("task-1", Ok(message), None, &receipt)
            .expect("receipt evaluates preview completion");
        assert_eq!(status, AgentLoopTerminalStatus::Failed);
        assert!(receipt.successful_write_tools.is_empty());
    }

    #[test]
    fn read_only_preview_request_omits_render_tool() {
        let policy = RequestToolPolicy::from_request("只检查，不要生成");
        let (_message, requests, _calls) =
            fixture_driver_with_policy("只检查，不要生成", vec![HELLO], json!({}), policy);
        assert!(!requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "render_preview"));
    }

    #[test]
    fn read_only_request_omits_main_chain_tools() {
        let policy = RequestToolPolicy::from_request("只读查看素材状态");
        let (_message, requests, _calls) =
            fixture_driver_with_policy("只读查看素材状态", vec![HELLO], json!({}), policy);
        let names = requests[0]["tools"].as_array().expect("tools");
        for name in [
            "request_asset_analysis",
            "generate_storyboard",
            "create_timeline_draft",
            "replace_clips",
            "change_clip_duration",
            "reorder_clips",
        ] {
            assert!(!names.iter().any(|tool| tool["name"] == name), "{name}");
        }
    }

    #[test]
    fn missing_timeline_returns_safe_failure_and_model_explains_it() {
        let policy = RequestToolPolicy::from_request("生成预览");
        let (message, requests, calls) = fixture_driver_with_policy(
            "生成预览",
            vec![
                RENDER_CALL,
                RENDER_FAILURE_REPLY,
                RENDER_FAILURE_REPLY,
                RENDER_FAILURE_REPLY,
            ],
            json!({
                "status": "failed",
                "operation": "render_preview",
                "code": "missing_timeline",
                "retryable": true,
                "recovery": "请先创建内部时间线。"
            }),
            policy,
        );
        assert_eq!(message, "当前没有时间线，所以还不能生成预览。");
        assert_eq!(calls, ["render_preview"]);
        assert_eq!(requests.len(), 4);
        let output = requests[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .expect("function output")["output"]
            .as_str()
            .expect("function output text");
        assert!(output.contains("missing_timeline"));
        assert!(!message.contains("已生成"));
    }

    #[test]
    fn storyboard_phase2_failure_tells_the_model_to_retry_generate_storyboard() {
        let failure = safe_tool_failure_context(
            "generate_storyboard",
            "storyboard_phase2_empty: no beat received a valid shot from its top candidates.",
        );
        assert_eq!(failure["code"], "storyboard_selection_failed");
        assert_eq!(failure["retryable"], true);
        assert!(failure["recovery"]
            .as_str()
            .is_some_and(|text| text.contains("generate_storyboard")
                && text.contains("Do not assemble shots")));
    }

    #[test]
    fn unconfigured_voice_provider_returns_a_closed_failure() {
        let failure = safe_tool_failure_context(
            "list_voices",
            "ElevenLabs voice Provider is not configured.",
        );
        assert_eq!(failure["code"], "voice_provider_unconfigured");
        assert_eq!(failure["retryable"], false);
        assert!(failure["recovery"]
            .as_str()
            .is_some_and(|text| text.contains("ElevenLabs")));
    }

    #[test]
    fn render_preview_arguments_are_scope_free_and_strictly_validated() {
        assert!(parse_native_arguments("render_preview", "{\"timelineVersionId\":null}").is_ok());
        assert!(
            parse_native_arguments("render_preview", "{\"timelineVersionId\":\"timeline-1\"}")
                .is_ok()
        );
        assert!(parse_native_arguments("render_preview", "{}").is_err());
        assert!(parse_native_arguments("render_preview", "{\"projectId\":\"project-1\"}").is_err());
        assert!(parse_native_arguments("render_preview", "{\"timelineVersionId\":42}").is_err());
    }

    #[test]
    fn main_chain_arguments_are_scope_free_and_strictly_bounded() {
        assert!(
            parse_native_arguments("request_asset_analysis", r#"{"assetIds":["asset-1"]}"#).is_ok()
        );
        assert!(parse_native_arguments("request_asset_analysis", r#"{"assetIds":[]}"#).is_err());
        assert!(parse_native_arguments(
            "request_asset_analysis",
            r#"{"assetIds":["asset-1"],"projectId":"project-1"}"#
        )
        .is_err());
        assert!(parse_native_arguments("generate_storyboard", r#"{"brief":null}"#).is_ok());
        assert!(parse_native_arguments("generate_storyboard", r#"{"brief":""}"#).is_err());
        assert!(parse_native_arguments("create_timeline_draft", "{}").is_ok());
        assert!(parse_native_arguments("create_timeline_draft", r#"{"projectId":"p"}"#).is_err());

        let replacement = json!({
            "timelineVersionId": null,
            "shots": [{
                "shotIndex": 0,
                "assetId": "asset-1",
                "sourceStartMs": 0,
                "sourceEndMs": 1_000
            }]
        });
        assert!(parse_native_arguments("replace_clips", &replacement.to_string()).is_ok());
        let mut invalid_replacement = replacement.clone();
        invalid_replacement["shots"][0]["sourceStartMs"] = json!(-1);
        assert!(parse_native_arguments("replace_clips", &invalid_replacement.to_string()).is_err());

        let adjustment = json!({
            "timelineVersionId": null,
            "adjustments": [{
                "shotIndex": 0,
                "newDurationMs": 1_000,
                "newSourceStartMs": null
            }]
        });
        assert!(parse_native_arguments("change_clip_duration", &adjustment.to_string()).is_ok());
        assert!(parse_native_arguments(
            "change_clip_duration",
            r#"{"timelineVersionId":null,"adjustments":[{"shotIndex":0,"newDurationMs":null,"newSourceStartMs":null}]}"#
        )
        .is_err());

        let order = json!({"timelineVersionId": null, "order": [1, 0]});
        assert!(parse_native_arguments("reorder_clips", &order.to_string()).is_ok());
        assert!(parse_native_arguments(
            "reorder_clips",
            r#"{"timelineVersionId":null,"order":[]}"#
        )
        .is_err());
    }

    #[test]
    fn native_write_tool_selection_includes_the_delivery_batch() {
        let tools = native_function_tools_for_request(false, true);
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<std::collections::HashSet<_>>();
        for name in [
            "request_asset_analysis",
            "generate_storyboard",
            "create_timeline_draft",
            "replace_clips",
            "change_clip_duration",
            "reorder_clips",
            "replace_text_tracks",
            "replace_music_tracks",
            "download_music",
            "use_online_music",
            "synthesize_voiceover",
            "create_jianying_draft",
        ] {
            assert!(names.contains(name));
        }
        let read_only_tools = native_function_tools_for_request(false, false);
        assert!(read_only_tools.iter().all(|tool| ![
            "request_asset_analysis",
            "generate_storyboard",
            "create_timeline_draft",
            "replace_clips",
            "change_clip_duration",
            "reorder_clips",
            "replace_text_tracks",
            "replace_music_tracks",
            "download_music",
            "use_online_music",
            "synthesize_voiceover",
            "create_jianying_draft",
        ]
        .contains(&tool["name"].as_str().unwrap())));
    }

    #[test]
    fn prepare_native_result_accepts_existing_main_chain_success_states() {
        let queued = prepare_native_tool_result(
            "request_asset_analysis",
            json!({"tool":"request_asset_analysis","status":"queued","queuedCount":1}),
        )
        .expect("queued analysis result");
        assert_eq!(queued["status"], "queued");
        let storyboard = prepare_native_tool_result(
            "generate_storyboard",
            json!({"tool":"generate_storyboard","status":"needs_confirmation","storyboardVersionId":"sb-1"}),
        )
        .expect("storyboard result");
        assert_eq!(storyboard["status"], "needs_confirmation");
        assert!(prepare_native_tool_result(
            "request_asset_analysis",
            json!({"tool":"request_asset_analysis","status":"failed"}),
        )
        .is_err());
    }

    #[test]
    fn remaining_observation_arguments_are_strictly_bounded() {
        for tool in ["get_edit_status", "get_storyboard", "get_text_capabilities"] {
            assert!(parse_native_arguments(tool, "{}").is_ok(), "{tool}");
            assert!(
                parse_native_arguments(tool, "{\"unexpected\":true}").is_err(),
                "{tool}"
            );
        }

        assert!(parse_native_arguments("search_music", "{\"query\":\"calm\"}").is_ok());
        assert!(parse_native_arguments("search_music", "{}").is_err());
        assert!(parse_native_arguments("search_music", "{\"query\":\"\"}").is_err());
        let long_query = "x".repeat(201);
        assert!(
            parse_native_arguments("search_music", &json!({"query": long_query}).to_string())
                .is_err()
        );

        let asset_search = json!({
            "query": null,
            "kind": "video",
            "minDurationMs": 0,
            "maxDurationMs": 60_000,
            "minRating": null,
            "favoriteOnly": false,
            "tag": null,
            "collectionId": null,
            "offset": 0,
            "limit": 20
        });
        assert!(parse_native_arguments("search_assets", &asset_search.to_string()).is_ok());
        let with_asset_search_value = |key: &str, value: Value| {
            let mut object = asset_search.as_object().unwrap().clone();
            object.insert(key.to_owned(), value);
            Value::Object(object)
        };
        for invalid in [
            json!({}),
            with_asset_search_value("kind", json!("document")),
            with_asset_search_value("minDurationMs", json!(-1)),
            with_asset_search_value("minRating", json!(6)),
            with_asset_search_value("offset", json!(10_001)),
            with_asset_search_value("limit", json!(0)),
        ] {
            assert!(parse_native_arguments("search_assets", &invalid.to_string()).is_err());
        }

        let segment_search = json!({
            "query": "street",
            "assetId": null,
            "offset": 0,
            "limit": 12
        });
        assert!(
            parse_native_arguments("search_asset_segments", &segment_search.to_string()).is_ok()
        );
        let blank_filters = parse_native_arguments(
            "search_assets",
            r#"{"query":"factory","kind":"video","minDurationMs":0,"maxDurationMs":30000,"minRating":0,"favoriteOnly":false,"tag":"","collectionId":"","offset":0,"limit":10}"#,
        )
        .expect("blank search filters become null");
        assert!(blank_filters["tag"].is_null());
        assert!(blank_filters["collectionId"].is_null());
        let blank_segment = parse_native_arguments(
            "search_asset_segments",
            r#"{"query":"factory","assetId":"","offset":0,"limit":10}"#,
        )
        .expect("blank assetId becomes null");
        assert!(blank_segment["assetId"].is_null());
        for invalid in [
            json!({"query":"street"}),
            json!({"query":"", "assetId":null, "offset":0, "limit":12}),
            json!({"query":"street", "assetId":42, "offset":0, "limit":12}),
            json!({"query":"street", "assetId":null, "offset":0, "limit":21}),
        ] {
            assert!(parse_native_arguments("search_asset_segments", &invalid.to_string()).is_err());
        }
    }

    #[test]
    fn delivery_arguments_are_scope_free_and_strictly_bounded() {
        assert!(parse_native_arguments("download_music", r#"{"trackId":"track-1"}"#).is_ok());
        assert!(parse_native_arguments("download_music", r#"{"trackId":""}"#).is_err());
        assert!(parse_native_arguments(
            "download_music",
            r#"{"trackId":"track-1","projectId":"project-1"}"#
        )
        .is_err());
        assert!(parse_native_arguments(
            "use_online_music",
            r#"{"trackId":"track-1","timelineVersionId":null}"#
        )
        .is_ok());
        assert!(
            parse_native_arguments("create_jianying_draft", r#"{"timelineVersionId":null}"#)
                .is_ok()
        );
        assert!(parse_native_arguments("list_voices", "{}").is_ok());
        assert!(parse_native_arguments(
            "synthesize_voiceover",
            r#"{"text":"Hello factory.","voiceId":null,"timelineVersionId":null}"#
        )
        .is_ok());
        let blank_voiceover = parse_native_arguments(
            "synthesize_voiceover",
            r#"{"text":"","voiceId":null,"timelineVersionId":null}"#,
        )
        .expect("blank narration becomes null");
        assert!(blank_voiceover["text"].is_null());

        let text_tracks = json!({
            "timelineVersionId": null,
            "textTracks": [{
                "id": "subtitle-1",
                "role": "subtitle",
                "layer": 0,
                "enabled": true,
                "cues": [{
                    "id": "cue-1", "templateId": null, "startMs": 0, "endMs": 1_000,
                    "text": "hello", "style": null, "layout": null,
                    "entrance": null, "exit": null, "loopAnimation": null
                }]
            }]
        });
        assert!(parse_native_arguments("replace_text_tracks", &text_tracks.to_string()).is_ok());
        let mut invalid_text_tracks = text_tracks.clone();
        invalid_text_tracks["textTracks"][0]["cues"][0]["jianyingCompatibility"] =
            json!("deliverable");
        assert!(
            parse_native_arguments("replace_text_tracks", &invalid_text_tracks.to_string())
                .is_err()
        );

        let music_tracks = json!({
            "timelineVersionId": null,
            "musicTracks": [{
                "id": "music-1", "enabled": true,
                "cues": [{
                    "id": "cue-1", "assetId": "asset-1", "sourceStartMs": 0,
                    "sourceEndMs": 1_000, "timelineStartMs": 0, "timelineEndMs": 1_000,
                    "loopEnabled": false, "volume": 0.35, "fadeInMs": 0, "fadeOutMs": 0
                }]
            }]
        });
        assert!(parse_native_arguments("replace_music_tracks", &music_tracks.to_string()).is_ok());
        let mut invalid_music_tracks = music_tracks.clone();
        invalid_music_tracks["musicTracks"][0]["cues"][0]["licenseUrl"] = json!("untrusted");
        assert!(
            parse_native_arguments("replace_music_tracks", &invalid_music_tracks.to_string())
                .is_err()
        );
    }

    #[test]
    fn dynamic_loader_and_log_range_arguments_are_strictly_bounded() {
        assert!(
            parse_native_arguments(LOAD_TOOLS, r#"{"toolNames":["read_logs","list_assets"]}"#)
                .is_ok()
        );
        for arguments in [
            r#"{"toolNames":[]}"#,
            r#"{"toolNames":["read_logs","read_logs"]}"#,
            r#"{"toolNames":["a","b","c","d","e","f"]}"#,
        ] {
            assert!(parse_native_arguments(LOAD_TOOLS, arguments).is_err());
        }

        assert!(parse_native_arguments(READ_LOGS, r#"{"startLine":null,"endLine":null}"#).is_ok());
        assert!(parse_native_arguments(READ_LOGS, r#"{"startLine":1,"endLine":100}"#).is_ok());
        for arguments in [
            r#"{"startLine":1,"endLine":null}"#,
            r#"{"startLine":2,"endLine":1}"#,
            r#"{"startLine":1,"endLine":101}"#,
        ] {
            assert!(parse_native_arguments(READ_LOGS, arguments).is_err());
        }

        let available = [READ_LOGS.to_owned(), "list_assets".to_owned()];
        let allowed_call = FunctionCall {
            call_id: "allowed".to_owned(),
            name: LOAD_TOOLS.to_owned(),
            arguments: r#"{"toolNames":["read_logs"]}"#.to_owned(),
            raw: Value::Null,
        };
        let denied_call = FunctionCall {
            call_id: "denied".to_owned(),
            name: LOAD_TOOLS.to_owned(),
            arguments: r#"{"toolNames":["create_jianying_draft"]}"#.to_owned(),
            raw: Value::Null,
        };
        assert!(load_request_uses_available_names(&allowed_call, &available));
        assert!(!load_request_uses_available_names(&denied_call, &available));

        let loaded = [READ_LOGS.to_owned()].into_iter().collect();
        assert!(dynamic_tool_loaded(LOAD_TOOLS, &loaded));
        assert!(dynamic_tool_loaded(READ_LOGS, &loaded));
        assert!(!dynamic_tool_loaded("list_assets", &loaded));
    }

    #[test]
    fn delivery_execution_rechecks_risk_classification() {
        let denied = RequestToolPolicy::from_request("Explain music options");
        let authorized =
            RequestToolPolicy::from_request("Download music and create a Jianying draft");
        for tool in [
            "download_music",
            "use_online_music",
            "create_jianying_draft",
        ] {
            assert!(!native_tool_call_allowed(tool, &denied, false), "{tool}");
        }
        assert!(native_tool_call_allowed(
            "replace_music_tracks",
            &denied,
            false
        ));
        assert!(native_tool_call_allowed(
            "replace_text_tracks",
            &denied,
            false
        ));
        assert!(native_tool_call_allowed(
            "download_music",
            &authorized,
            false
        ));
        assert!(native_tool_call_allowed(
            "create_jianying_draft",
            &authorized,
            false
        ));
    }

    #[test]
    fn complete_directory_does_not_hide_sensitive_tools() {
        let policy = RequestToolPolicy::from_request("添加字幕并替换背景音乐");
        let tools = full_native_tool_catalog();
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<std::collections::HashSet<_>>();
        assert!(names.contains("replace_text_tracks"));
        assert!(names.contains("replace_music_tracks"));
        assert!(names.contains("request_asset_analysis"));
        assert!(names.contains("generate_storyboard"));
        for visible_but_execution_gated in [
            "download_music",
            "use_online_music",
            "synthesize_voiceover",
            "create_jianying_draft",
        ] {
            assert!(
                names.contains(visible_but_execution_gated),
                "{visible_but_execution_gated}"
            );
            assert!(!native_tool_call_allowed(
                visible_but_execution_gated,
                &policy,
                false
            ));
        }
    }

    #[test]
    fn model_can_select_each_remaining_observation_tool() {
        let cases = [
            ("get_edit_status", json!({})),
            (
                "search_assets",
                json!({
                    "query": null, "kind": null, "minDurationMs": null, "maxDurationMs": null,
                    "minRating": null, "favoriteOnly": false, "tag": null, "collectionId": null,
                    "offset": 0, "limit": 12
                }),
            ),
            (
                "search_asset_segments",
                json!({"query":"street", "assetId":null, "offset":0, "limit":12}),
            ),
            ("search_music", json!({"query":"calm"})),
            ("get_storyboard", json!({})),
            ("get_text_capabilities", json!({})),
        ];

        for (tool, arguments) in cases {
            let call_id = format!("call_{tool}");
            let call = json!({
                "id": format!("resp_{tool}"),
                "output": [{
                    "type": "function_call",
                    "call_id": call_id,
                    "name": tool,
                    "arguments": arguments.to_string()
                }]
            })
            .to_string();
            let reply = json!({
                "id": format!("reply_{tool}"),
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type":"output_text", "text":"已读取。"}]
                }]
            })
            .to_string();
            let mut fixtures = vec![call, reply].into_iter();
            let mut input = vec![json!({
                "role": "user",
                "content": [{"type": "input_text", "text": "fixture request"}]
            })];
            let mut selected = Vec::new();
            let mut respond = |_payload: &Value, _timeout: Duration| {
                Ok::<_, String>(fixtures.next().expect("fixture response"))
            };
            let mut execute = |call: &FunctionCall, _step: usize| {
                selected.push(call.name.clone());
                Ok::<_, String>(json!({"tool":call.name,"status":"ok","result":{}}))
            };
            let message = drive_native_loop(
                &mut input,
                false,
                false,
                &RequestToolPolicy::default(),
                &mut NativeRunReceipt::default(),
                "fixture request",
                Instant::now() + Duration::from_secs(5),
                &mut respond,
                &mut execute,
                &mut || Ok(None),
                || false,
                |_body, _step| {},
            )
            .expect("native tool selection");
            drop(execute);
            assert_eq!(message, "已读取。");
            assert_eq!(selected, [tool]);
        }
    }

    #[test]
    fn native_observation_results_keep_safe_envelopes_and_remove_scope_fields() {
        let result = prepare_native_tool_result(
            "get_storyboard",
            json!({
                "tool": "get_storyboard",
                "status": "ok",
                "storyboard": {
                    "id": "storyboard-1",
                    "projectId": "project-1",
                    "editingTaskId": "task-1",
                    "shots": [{"assetId":"asset-1", "sourcePath":"C:\\private\\clip.mp4"}]
                }
            }),
        )
        .expect("safe storyboard result");
        let encoded = result.to_string();
        assert!(!encoded.contains("projectId"));
        assert!(!encoded.contains("editingTaskId"));
        assert!(!encoded.contains("sourcePath"));
        assert!(encoded.contains("asset-1"));

        assert!(prepare_native_tool_result(
            "search_assets",
            json!({"tool":"search_assets","status":"ok","results":{}})
        )
        .is_ok());
        assert!(prepare_native_tool_result(
            "search_assets",
            json!({"tool":"get_storyboard","status":"ok","results":{}})
        )
        .is_err());
    }

    #[test]
    fn native_result_keeps_model_reply_instead_of_last_outcome_message() {
        let outcome = AgentEditResult {
            agent_task_id: "task-1".to_owned(),
            message: "deterministic artifact message".to_owned(),
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        };
        let result = native_result_from_message(
            "task-1",
            "model summarized the real receipt".to_owned(),
            Some(outcome),
        );
        assert_eq!(result.message, "model summarized the real receipt");
    }

    #[test]
    fn model_claim_after_tool_failure_cannot_be_completed() {
        let receipt = NativeRunReceipt {
            tool_called: true,
            failed_tools: ["generate_storyboard".to_owned()].into_iter().collect(),
            ..NativeRunReceipt::default()
        };
        let (_result, status) = finish_native_result(
            "task-1",
            Ok("Storyboard 已生成。".to_owned()),
            None,
            &receipt,
        )
        .expect("safe failed terminal result");
        assert_eq!(status, AgentLoopTerminalStatus::Failed);
    }

    #[test]
    fn model_reply_failure_after_observation_keeps_native_safe_failure_message() {
        let receipt = NativeRunReceipt {
            requires_project_observation: true,
            successful_observation_this_turn: true,
            tool_called: true,
            successful_tool_call: true,
            ..NativeRunReceipt::default()
        };
        let (result, status) = finish_native_result(
            "task-1",
            Err("custom provider transport detail must not reach the UI".to_owned()),
            None,
            &receipt,
        )
        .expect("native failure is persisted without falling back to Legacy text");
        assert_eq!(status, AgentLoopTerminalStatus::Failed);
        assert!(result.message.contains("项目数据已读取"));
        assert!(!result.message.contains("transport"));
        assert!(result.storyboard.is_none());
        assert!(result.timeline.is_none());
        assert!(result.preview.is_none());
    }

    #[test]
    fn preview_tool_is_broadly_available_unless_request_policy_denies_it() {
        for request in [
            "生成预览",
            "你好",
            "怎么生成预览？",
            "解释生成预览是什么意思",
        ] {
            assert!(native_render_preview_allowed(
                request,
                &RequestToolPolicy::from_request(request)
            ));
        }
        for request in ["只查看", "只检查，不要生成", "不要生成预览"] {
            assert!(!native_render_preview_allowed(
                request,
                &RequestToolPolicy::from_request(request)
            ));
        }
    }

    #[test]
    fn preview_execution_rechecks_request_policy_permission() {
        let policy = RequestToolPolicy::from_request("你好");
        assert!(!native_tool_call_allowed("render_preview", &policy, false));
        assert!(native_tool_call_allowed("render_preview", &policy, true));
        let denied = RequestToolPolicy::from_request("不要生成预览");
        assert!(!native_tool_call_allowed("render_preview", &denied, true));
    }

    #[test]
    fn verified_preview_survives_model_summary_failure() {
        let outcome = AgentEditResult {
            agent_task_id: "task-1".to_owned(),
            message: "unused".to_owned(),
            storyboard: None,
            timeline: None,
            preview: Some(PreviewResult {
                timeline_version_id: "timeline-1".to_owned(),
                preview_path: "redacted".to_owned(),
                quality_report: PreviewQualityReport { checks: Vec::new() },
            }),
            jianying_draft: None,
        };
        let (result, status) = finish_native_result(
            "task-1",
            Err("native_tool_loop_response_unparseable".to_owned()),
            Some(outcome),
            &NativeRunReceipt::default(),
        )
        .expect("partial preview result");
        assert_eq!(status, AgentLoopTerminalStatus::PartiallyCompleted);
        assert!(result.preview.is_some());
        assert!(result.message.contains("预览已由工具生成并验证"));
    }

    fn fixture_driver_with_policy(
        request: &str,
        fixtures: Vec<&'static str>,
        execute_result: Value,
        policy: RequestToolPolicy,
    ) -> (String, Vec<Value>, Vec<String>) {
        let mut responses = fixtures.into_iter();
        let mut requests = Vec::new();
        let mut calls = Vec::new();
        let mut input = vec![json!({
            "role": "user",
            "content": [{"type": "input_text", "text": request}]
        })];
        let mut respond = |payload: &Value, _timeout: Duration| {
            requests.push(payload.clone());
            Ok::<_, String>(responses.next().expect("fixture response").to_owned())
        };
        let mut execute = |call: &FunctionCall, _step: usize| {
            calls.push(call.name.clone());
            Ok::<_, String>(execute_result.clone())
        };
        let message = drive_native_loop(
            &mut input,
            false,
            false,
            &policy,
            &mut NativeRunReceipt::default(),
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("fixture loop");
        drop(execute);
        (message, requests, calls)
    }

    #[test]
    fn initial_input_injects_exactly_one_snapshot_between_system_and_history() {
        let policy = RequestToolPolicy::default();
        let snapshot = format!(
            "{}\nstoryboard: v3",
            crate::agentloop::snapshot::STATE_SNAPSHOT_PREFIX
        );
        let history = vec![json!({
            "role": "assistant",
            "content": [{"type": "output_text", "text": "旧回答"}]
        })];

        let input = initial_native_input(history, "当前问题", &policy, "", Ok(snapshot))
            .expect("build initial Native input");

        assert_eq!(
            input
                .iter()
                .filter(|item| is_snapshot_message(item))
                .count(),
            1
        );
        assert_eq!(input.iter().position(is_snapshot_message), Some(1));
        assert_eq!(input[2]["role"], "assistant");
        assert_eq!(input.last().expect("current user item")["role"], "user");
    }

    #[test]
    fn initial_input_fails_closed_when_snapshot_build_fails() {
        let error = initial_native_input(
            Vec::new(),
            "当前问题",
            &RequestToolPolicy::default(),
            "",
            Err("fixture contains a private path".to_owned()),
        )
        .expect_err("missing snapshot must stop the Native loop");
        assert_eq!(error, "native_state_snapshot_unavailable");
        assert!(!error.contains("private path"));
    }

    #[test]
    fn successful_write_refreshes_the_only_snapshot_before_the_next_request() {
        let request = "生成 storyboard";
        let policy = RequestToolPolicy::from_request(request);
        let initial_snapshot = format!(
            "{}\nstoryboard: v3",
            crate::agentloop::snapshot::STATE_SNAPSHOT_PREFIX
        );
        let mut input =
            initial_native_input(Vec::new(), request, &policy, "", Ok(initial_snapshot))
                .expect("build snapshot fixture input");
        let mut responses = [MAIN_CHAIN_STORYBOARD_CALL, MAIN_CHAIN_CONFIRMATION_REPLY].into_iter();
        let mut requests = Vec::new();
        let mut respond = |payload: &Value, _timeout: Duration| {
            requests.push(payload.clone());
            Ok::<_, String>(
                responses
                    .next()
                    .expect("snapshot refresh response")
                    .to_owned(),
            )
        };
        let mut execute = |_call: &FunctionCall, _step: usize| {
            Ok::<_, String>(json!({
                "tool": "generate_storyboard",
                "status": "needs_confirmation",
                "versionNumber": 4
            }))
        };
        let mut refresh = || {
            Ok(Some(render_snapshot_message(&format!(
                "{}\nstoryboard: v4",
                crate::agentloop::snapshot::STATE_SNAPSHOT_PREFIX
            ))))
        };

        drive_native_loop(
            &mut input,
            false,
            false,
            &policy,
            &mut NativeRunReceipt::default(),
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut refresh,
            || false,
            |_body, _step| {},
        )
        .expect("write followed by refreshed summary");
        drop(respond);

        assert_eq!(requests.len(), 2);
        for request_payload in &requests {
            let items = request_payload["input"].as_array().expect("Provider input");
            assert_eq!(
                items
                    .iter()
                    .filter(|item| is_snapshot_message(item))
                    .count(),
                1
            );
        }
        assert!(requests[0].to_string().contains("storyboard: v3"));
        assert!(!requests[1].to_string().contains("storyboard: v3"));
        assert!(requests[1].to_string().contains("storyboard: v4"));
    }

    #[test]
    fn snapshot_observation_allows_a_direct_fact_answer_without_nudge() {
        let request = "当前项目有多少素材？";
        let policy = RequestToolPolicy::from_request(request);
        let snapshot = format!(
            "{}\n素材: total=2",
            crate::agentloop::snapshot::STATE_SNAPSHOT_PREFIX
        );
        let mut input = initial_native_input(Vec::new(), request, &policy, "", Ok(snapshot))
            .expect("build observed input");
        let mut request_count = 0;
        let mut respond = |_payload: &Value, _timeout: Duration| {
            request_count += 1;
            Ok::<_, String>(HELLO.to_owned())
        };
        let mut execute = |_call: &FunctionCall, _step: usize| {
            panic!("snapshot-backed direct answer must not call a tool")
        };
        let mut receipt = NativeRunReceipt {
            successful_observation_this_turn: true,
            ..NativeRunReceipt::default()
        };
        receipt
            .observation_sources
            .insert("state_snapshot".to_owned());

        drive_native_loop(
            &mut input,
            false,
            true,
            &policy,
            &mut receipt,
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            &mut || Ok(None),
            || false,
            |_body, _step| {},
        )
        .expect("snapshot opens project fact gate");

        assert_eq!(request_count, 1);
        assert!(receipt.observation_sources.contains("state_snapshot"));
    }

    #[test]
    fn compression_plan_moves_old_history_but_keeps_function_call_pair() {
        let mut input = vec![
            json!({"role": "system", "content": [{"type": "input_text", "text": "身份"}]}),
            render_snapshot_message(&format!(
                "{}\n素材: total=2",
                crate::agentloop::snapshot::STATE_SNAPSHOT_PREFIX
            )),
            json!({"role": "user", "content": [{"type": "input_text", "text": "old question ".repeat(5_000)}]}),
            json!({"role": "assistant", "content": [{"type": "output_text", "text": "old answer ".repeat(5_000)}]}),
            json!({"role": "user", "content": [{"type": "input_text", "text": "当前问题"}]}),
            json!({"type": "function_call", "call_id": "call_1", "name": "list_assets", "arguments": "{}"}),
            json!({"type": "function_call_output", "call_id": "call_1", "output": "x".repeat(1000)}),
        ];

        let plan = compression_plan(&input, "当前问题");
        input = plan.retained;

        assert!(input.iter().any(|item| item["type"] == "function_call"));
        assert!(input
            .iter()
            .any(|item| item["type"] == "function_call_output"));
        assert!(input
            .iter()
            .any(|item| item["role"] == "user" && item["content"][0]["text"] == "当前问题"));
        assert_eq!(
            input
                .iter()
                .filter(|item| is_snapshot_message(item))
                .count(),
            1
        );
        assert!(!input
            .iter()
            .any(|item| item.to_string().contains("old question")));
    }

    #[test]
    fn token_budget_truncates_huge_tool_output_before_dropping_current_turn() {
        let mut input = vec![
            json!({"role": "system", "content": [{"type": "input_text", "text": "身份"}]}),
            render_snapshot_message(&format!(
                "{}\n素材: total=2",
                crate::agentloop::snapshot::STATE_SNAPSHOT_PREFIX
            )),
            json!({"role": "user", "content": [{"type": "input_text", "text": "用这个文案生成视频"}]}),
            json!({"type": "function_call", "call_id": "call_1", "name": "list_assets", "arguments": "{}"}),
            json!({"type": "function_call_output", "call_id": "call_1", "output": "log entry ".repeat(6_000)}),
        ];
        compact_tool_outputs(&mut input);
        let output = input
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .expect("kept tool output")["output"]
            .as_str()
            .expect("output text");
        assert!(super::super::context::token_count_text(output) <= 4_020);
        assert!(output.contains("[truncated]"));
        assert!(input.iter().any(
            |item| item["role"] == "user" && item["content"][0]["text"] == "用这个文案生成视频"
        ));
        assert_eq!(
            input
                .iter()
                .filter(|item| is_snapshot_message(item))
                .count(),
            1
        );
    }

    #[test]
    fn oversized_context_uses_model_memory_and_reaches_the_token_target() {
        let request = "current request";
        let mut input = vec![
            json!({"role":"system","content":[{"type":"input_text","text":"system"}]}),
            render_snapshot_message(crate::agentloop::snapshot::STATE_SNAPSHOT_PREFIX),
            json!({"role":"user","content":[{"type":"input_text","text":"old history ".repeat(25_000)}]}),
            json!({"role":"user","content":[{"type":"input_text","text":request}]}),
        ];
        let tools = vec![load_tools_function_tool(&["read_logs".to_owned()])];
        assert!(provider_payload_tokens(&input, &tools) > COMPRESSION_TRIGGER_TOKENS);
        let mut calls = 0usize;
        let mut respond = |payload: &Value, _timeout: Duration| {
            calls += 1;
            let instruction = payload["input"][0]["content"][0]["text"]
                .as_str()
                .expect("compression instruction");
            for required in [
                "用户目标",
                "用户明确约束",
                "用户偏好",
                "已作决定",
                "未解决问题",
            ] {
                assert!(instruction.contains(required), "{required}");
            }
            Ok(json!({
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "用户目标：继续当前任务。约束：无新增事实。偏好：简洁。已作决定：保留当前方案。未解决问题：继续验证。"
                    }]
                }]
            })
            .to_string())
        };

        compact_native_context(
            &mut input,
            &tools,
            false,
            request,
            Instant::now() + Duration::from_secs(10),
            &mut respond,
        )
        .expect("compact context");

        assert!(calls > 0);
        assert!(provider_payload_tokens(&input, &tools) <= COMPRESSION_TARGET_TOKENS);
        assert!(input
            .iter()
            .any(|item| item.to_string().contains("Compressed conversation memory")));
        assert!(input
            .iter()
            .any(|item| { item["role"] == "user" && item["content"][0]["text"] == request }));
    }

    #[test]
    fn compression_failure_stops_without_deleting_history() {
        let request = "current request";
        let mut input = vec![
            json!({"role":"system","content":[{"type":"input_text","text":"system"}]}),
            render_snapshot_message(crate::agentloop::snapshot::STATE_SNAPSHOT_PREFIX),
            json!({"role":"assistant","content":[{"type":"output_text","text":"old history ".repeat(25_000)}]}),
            json!({"role":"user","content":[{"type":"input_text","text":request}]}),
        ];
        let tools = vec![load_tools_function_tool(&["read_logs".to_owned()])];
        let mut respond =
            |_payload: &Value, _timeout: Duration| Err("provider unavailable".to_owned());

        let original = input.clone();
        let result = compact_native_context(
            &mut input,
            &tools,
            false,
            request,
            Instant::now() + Duration::from_secs(10),
            &mut respond,
        );

        assert_eq!(result, Err("native_context_compression_failed".to_owned()));
        assert_eq!(input, original);
    }

    #[test]
    fn full_catalog_is_stable_across_request_wording() {
        let policy = RequestToolPolicy::from_request("用这个文案生成视频");
        let tools = full_native_tool_catalog();
        let names = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<std::collections::HashSet<_>>();
        for name in ["generate_storyboard", "create_timeline_draft"] {
            assert!(names.contains(name), "{name}");
        }
        assert!(names.contains("synthesize_voiceover"));
        assert!(!native_tool_call_allowed(
            "synthesize_voiceover",
            &policy,
            false
        ));
    }
}
