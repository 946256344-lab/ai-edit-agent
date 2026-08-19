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
    model_turn_from_chat_completions, model_turn_from_responses, post_model_payload, FunctionCall,
    ModelAccess, ModelOutputItem,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use super::policy::{request_requires_project_observation, RequestToolPolicy, OBSERVATION_TOOLS};
use super::schema::{
    AgentLoopResult, AgentLoopTerminalStatus, LoopState, AGENT_RUN_TIMEOUT, AGENT_STEP_TIMEOUT,
    MAX_STEPS,
};
use super::skills::{
    apply_skill, persisted_artifact_for_tool, safe_step_error_code, safe_tool_failure_context,
};
use super::tools::native_function_tools_for_request;

const NATIVE_TOOL_NAMES: &[&str] = &[
    "get_edit_status",
    "get_asset_health_summary",
    "list_assets",
    "search_assets",
    "search_asset_segments",
    "search_music",
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
    let history = super::prompt::load_native_message_history(connection, conversation_id, request);
    let tool_policy = RequestToolPolicy::from_request(request);
    let render_preview_authorized = native_render_preview_allowed(request, &tool_policy);
    let mut input = vec![json!({
        "role": "system",
        "content": [{
            "type": "input_text",
            "text": "You are a local video project assistant. Answer ordinary questions directly. For current project facts, use only the provided observation functions before answering. Use artifact-producing functions only when they match the user's request and are present in tools. Treat function outputs as the only project and artifact facts. A generated storyboard with status needs_confirmation must be summarized for user review; do not create or edit a timeline until the user confirms it in a later turn. Claim an artifact was created only when its function output confirms success. If a function returns a structured failure, explain it safely or adjust with another allowed function."
        }]
    })];
    input.extend(history);
    input.push(json!({
        "role": "user",
        "content": [{"type": "input_text", "text": request}]
    }));

    let mut state = LoopState {
        app,
        connection,
        agent_task_id,
        project_id,
        editing_task_id,
        conversation_id,
        task_brief: task_brief.to_owned(),
        tool_policy: tool_policy.clone(),
        storyboard: storyboard.cloned(),
        timelines: timelines.to_vec(),
        last_outcome: None,
        last_failed_tool_error_code: None,
        successful_observation: false,
    };
    let is_custom = access.custom_config().is_some();
    let mut respond =
        |payload: &Value, timeout: Duration| post_model_payload(access, payload, Some(timeout));
    let mut execute = |call: &FunctionCall, step_number: usize| {
        execute_native_tool(&mut state, call, step_number, render_preview_authorized)
    };
    let cancelled = || native_task_cancelled(connection, agent_task_id);
    let mut preview_succeeded = false;
    let mut receipt = NativeRunReceipt {
        requires_project_observation: request_requires_project_observation(request),
        ..NativeRunReceipt::default()
    };
    let loop_result = drive_native_loop(
        &mut input,
        is_custom,
        request_requires_project_observation(request),
        &tool_policy,
        &mut preview_succeeded,
        &mut receipt,
        request,
        run_deadline,
        &mut respond,
        &mut execute,
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
        render_preview_authorized,
        preview_succeeded,
        &receipt,
    )?;
    Ok(AgentLoopResult {
        result,
        status,
        clarification_goal: receipt.needs_confirmation.then_some("storyboard"),
    })
}

fn finish_native_result(
    agent_task_id: &str,
    loop_result: Result<String, String>,
    last_outcome: Option<AgentEditResult>,
    preview_requested: bool,
    preview_succeeded: bool,
    receipt: &NativeRunReceipt,
) -> Result<(AgentEditResult, AgentLoopTerminalStatus), String> {
    match loop_result {
        Ok(message) => {
            let status = if receipt.failed_tool_call && receipt.successful_tool_call {
                AgentLoopTerminalStatus::PartiallyCompleted
            } else if receipt.failed_tool_call {
                AgentLoopTerminalStatus::Failed
            } else if last_outcome
                .as_ref()
                .is_some_and(|outcome| outcome.storyboard.is_some() && outcome.timeline.is_none())
            {
                AgentLoopTerminalStatus::NeedsClarification
            } else if preview_requested && !preview_succeeded {
                AgentLoopTerminalStatus::Failed
            } else {
                AgentLoopTerminalStatus::Completed
            };
            Ok((
                native_result_from_message(agent_task_id, message, last_outcome),
                status,
            ))
        }
        Err(_error) => {
            match last_outcome {
                Some(mut outcome) if outcome.preview.is_some() => {
                    outcome.message = "本地预览已生成并验证，但模型未能完成结果说明；预览已保留，可稍后继续检查。".to_owned();
                    Ok((outcome, AgentLoopTerminalStatus::PartiallyCompleted))
                }
                _ => Ok((
                    native_model_reply_unavailable_result(agent_task_id, receipt),
                    AgentLoopTerminalStatus::Failed,
                )),
            }
        }
    }
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

type NativeRespond<'a> = dyn FnMut(&Value, Duration) -> Result<String, String> + 'a;
type NativeExecute<'a> = dyn FnMut(&FunctionCall, usize) -> Result<Value, String> + 'a;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct NativeRunReceipt {
    requires_project_observation: bool,
    successful_observation_this_turn: bool,
    tool_called: bool,
    successful_tool_call: bool,
    failed_tool_call: bool,
    needs_confirmation: bool,
}

fn drive_native_loop(
    input: &mut Vec<Value>,
    is_custom: bool,
    requires_observation: bool,
    tool_policy: &RequestToolPolicy,
    preview_succeeded: &mut bool,
    receipt: &mut NativeRunReceipt,
    request: &str,
    run_deadline: Instant,
    respond: &mut NativeRespond<'_>,
    execute: &mut NativeExecute<'_>,
    mut cancelled: impl FnMut() -> bool,
    mut observed: impl FnMut(&str, usize),
) -> Result<String, String> {
    receipt.requires_project_observation = requires_observation;
    let mut preview_tool_called = false;
    let mut storyboard_confirmation_pending = false;
    for step_number in 1..=MAX_STEPS {
        if cancelled() {
            return Err("native_tool_loop_cancelled".to_owned());
        }
        let Some(timeout) = remaining_timeout(run_deadline) else {
            return Err("native_tool_loop_deadline_exceeded".to_owned());
        };
        trim_native_input(input, request);
        let payload = json!({
            "model": "gpt-5.4",
            "store": false,
            "stream": false,
            "parallel_tool_calls": false,
            "tool_choice": "auto",
            "tools": filtered_native_tools(
                native_function_tools_for_request(
                    !storyboard_confirmation_pending
                        && native_render_preview_allowed(request, tool_policy),
                    tool_policy.has_native_write_authorization() && !storyboard_confirmation_pending,
                ),
                tool_policy,
                !storyboard_confirmation_pending && native_render_preview_allowed(request, tool_policy),
            ),
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
            if native_render_preview_allowed(request, tool_policy) && !preview_tool_called {
                input.push(json!({
                    "role": "system",
                    "content": [{
                        "type": "input_text",
                        "text": "This request explicitly asks for a preview. Call render_preview and wait for its structured result before answering; never claim a preview exists from text alone."
                    }]
                }));
                continue;
            }
            if let Some(message) = model_message_text(&turn) {
                if requires_observation
                    && !receipt.successful_observation_this_turn
                    && !receipt.failed_tool_call
                {
                    input.push(json!({
                        "role": "system",
                        "content": [{
                            "type": "input_text",
                            "text": "This request asks about current project facts. Call one allowed read-only observation function before answering."
                        }]
                    }));
                    continue;
                }
                if tool_policy.has_native_write_authorization() && !receipt.successful_tool_call {
                    input.push(json!({
                        "role": "system",
                        "content": [{
                            "type": "input_text",
                            "text": "This request explicitly asks for a local project change. Use the matching provided function tool and wait for its structured result before claiming completion."
                        }]
                    }));
                    continue;
                }
                return Ok(message);
            }
            return Err("native_tool_loop_response_missing_message".to_owned());
        }
        receipt.tool_called = true;
        preview_tool_called |= calls.iter().any(|call| call.name == "render_preview");

        for item in &turn.output {
            if let Some(value) = output_item_for_input(item, is_custom) {
                input.push(value);
            }
        }
        for call in calls {
            if cancelled() {
                return Err("native_tool_loop_cancelled".to_owned());
            }
            let result = if storyboard_confirmation_pending
                && !OBSERVATION_TOOLS.contains(&call.name.as_str())
            {
                storyboard_confirmation_required(&call.name)
            } else {
                execute(&call, step_number)?
            };
            let result_status = result["status"].as_str();
            if result_status == Some("needs_confirmation") {
                storyboard_confirmation_pending = true;
                receipt.needs_confirmation = true;
            }
            if OBSERVATION_TOOLS.contains(&call.name.as_str()) && result_status == Some("ok") {
                receipt.successful_observation_this_turn = true;
            }
            if matches!(
                result_status,
                Some("ok") | Some("queued") | Some("needs_confirmation")
            ) {
                receipt.successful_tool_call = true;
            } else {
                receipt.failed_tool_call = true;
            }
            if call.name == "render_preview"
                && result["status"] == "ok"
                && result["artifact"]["type"] == "preview"
            {
                *preview_succeeded = true;
            }
            input.push(json!({
                "type": "function_call_output",
                "call_id": call.call_id,
                "output": result.to_string(),
            }));
        }
    }
    Err("native_tool_loop_max_steps".to_owned())
}

fn native_render_preview_allowed(request: &str, tool_policy: &RequestToolPolicy) -> bool {
    if tool_policy.forbids("render_preview") || request.contains(['?', '？']) {
        return false;
    }
    let mut normalized = request
        .trim()
        .trim_matches(|character: char| matches!(character, '。' | '！' | '.' | '!' | '，' | ','))
        .trim()
        .to_lowercase();
    normalized = normalized.replace("一个", "").replace("一段", "");
    for prefix in ["请帮我", "麻烦帮我", "请", "帮我", "麻烦"] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped.trim().to_owned();
            break;
        }
    }
    matches!(
        normalized.as_str(),
        "生成预览"
            | "创建预览"
            | "渲染预览"
            | "制作预览"
            | "生成预览视频"
            | "渲染预览视频"
            | "生成低清预览"
            | "generate preview"
            | "create preview"
            | "render preview"
            | "make preview"
    )
}

fn filtered_native_tools(
    tools: Vec<Value>,
    policy: &RequestToolPolicy,
    render_preview_included: bool,
) -> Vec<Value> {
    tools
        .into_iter()
        .filter(|tool| {
            let Some(name) = tool["name"].as_str() else {
                return false;
            };
            policy.native_tool_exposed(name)
                || (name == "render_preview" && render_preview_included)
        })
        .collect()
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

const MAX_NATIVE_INPUT_CHARS: usize = 16_000;

fn trim_native_input(input: &mut Vec<Value>, request: &str) {
    trim_native_input_to_budget(input, request, MAX_NATIVE_INPUT_CHARS);
}

fn trim_native_input_to_budget(input: &mut Vec<Value>, request: &str, max_chars: usize) {
    let protected_call_id = input.iter().rev().find_map(|item| {
        (item["type"] == "function_call")
            .then(|| item["call_id"].as_str().map(str::to_owned))
            .flatten()
    });
    while input_char_count(input) > max_chars {
        let current_index = input
            .iter()
            .rposition(|item| is_current_user_item(item, request));
        let Some(remove_index) = (1..input.len()).find(|index| {
            Some(*index) != current_index
                && !protected_call_id.as_ref().is_some_and(|call_id| {
                    input[*index]["call_id"].as_str() == Some(call_id.as_str())
                        && matches!(
                            input[*index]["type"].as_str(),
                            Some("function_call") | Some("function_call_output")
                        )
                })
        }) else {
            break;
        };
        if input[remove_index]["type"] == "function_call" {
            let call_id = input[remove_index]["call_id"].clone();
            input.remove(remove_index);
            if let Some(output_index) = input.iter().position(|item| {
                item["type"] == "function_call_output" && item["call_id"] == call_id
            }) {
                input.remove(output_index);
            }
        } else if input[remove_index]["type"] == "function_call_output" {
            let call_id = input[remove_index]["call_id"].clone();
            input.remove(remove_index);
            if let Some(call_index) = input
                .iter()
                .position(|item| item["type"] == "function_call" && item["call_id"] == call_id)
            {
                input.remove(call_index);
            }
        } else {
            input.remove(remove_index);
        }
    }
}

fn is_current_user_item(item: &Value, request: &str) -> bool {
    item["role"] == "user"
        && item["content"]
            .as_array()
            .and_then(|content| content.first())
            .and_then(|content| content["text"].as_str())
            .is_some_and(|text| text == request)
}

fn input_char_count(input: &[Value]) -> usize {
    input
        .iter()
        .map(|item| item.to_string().chars().count())
        .sum()
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
    let started_at = Instant::now();
    let result = apply_skill(state, &call.name, &args);
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
            if OBSERVATION_TOOLS.contains(&call.name.as_str()) && value["status"] == "ok" {
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
    let Some(object) = value.as_object() else {
        return Err(invalid_arguments());
    };
    match tool {
        "get_edit_status"
        | "get_asset_health_summary"
        | "list_assets"
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

    fn fixture_driver(
        fixtures: Vec<&'static str>,
        execute_result: Value,
    ) -> (String, Vec<Value>, Vec<String>) {
        let mut responses = fixtures.into_iter();
        let mut requests = Vec::new();
        let mut calls = Vec::new();
        let mut preview_succeeded = false;
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
            &mut preview_succeeded,
            &mut NativeRunReceipt::default(),
            "fixture request",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            || false,
            |_body, _step| {},
        )
        .expect("fixture loop");
        drop(execute);
        (message, requests, calls)
    }

    #[test]
    fn ordinary_question_returns_message_without_tool_call() {
        let (message, requests, calls) = fixture_driver(vec![HELLO], json!({}));
        assert_eq!(message, "你好！有什么我可以帮你查看的吗？");
        assert!(calls.is_empty());
        assert_eq!(requests[0]["parallel_tool_calls"], false);
        assert_eq!(requests[0]["store"], false);
        assert_eq!(requests[0]["tools"].as_array().map(Vec::len), Some(9));
    }

    #[test]
    fn ordinary_chat_does_not_expose_native_write_tools() {
        let (_message, requests, _calls) = fixture_driver_with_policy(
            "你好",
            vec![HELLO],
            json!({}),
            RequestToolPolicy::from_request("你好"),
        );
        let names = requests[0]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<std::collections::HashSet<_>>();
        assert!(names.contains("list_assets"));
        for name in [
            "request_asset_analysis",
            "generate_storyboard",
            "create_timeline_draft",
            "replace_clips",
            "change_clip_duration",
            "reorder_clips",
        ] {
            assert!(!names.contains(name), "ordinary chat exposed {name}");
        }
    }

    #[test]
    fn advice_and_inspection_requests_only_expose_observation_tools() {
        for request in [
            "这些素材适合怎么剪？",
            "Please inspect these assets and do not modify anything.",
        ] {
            let (_message, requests, _calls) = fixture_driver_with_policy(
                request,
                vec![HELLO],
                json!({}),
                RequestToolPolicy::from_request(request),
            );
            let names = requests[0]["tools"]
                .as_array()
                .expect("tools")
                .iter()
                .filter_map(|tool| tool["name"].as_str())
                .collect::<std::collections::HashSet<_>>();
            assert!(names
                .iter()
                .all(|name| { super::super::policy::OBSERVATION_TOOLS.contains(name) }));
        }
    }

    #[test]
    fn explicit_analysis_storyboard_request_only_exposes_required_capabilities() {
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
            let names = requests[0]["tools"]
                .as_array()
                .expect("tools")
                .iter()
                .filter_map(|tool| tool["name"].as_str())
                .collect::<std::collections::HashSet<_>>();
            assert!(names.contains("request_asset_analysis"));
            assert!(names.contains("generate_storyboard"));
            for name in [
                "create_timeline_draft",
                "replace_clips",
                "change_clip_duration",
                "reorder_clips",
            ] {
                assert!(!names.contains(name), "request exposed unrelated {name}");
            }
        }
    }

    #[test]
    fn forged_native_write_call_is_rejected_without_explicit_authorization() {
        for request in ["你好", "Please inspect the assets only."] {
            let policy = RequestToolPolicy::from_request(request);
            assert!(!native_tool_call_allowed(
                "generate_storyboard",
                &policy,
                false
            ));
            assert!(!native_tool_call_allowed("replace_clips", &policy, false));
        }
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
        let mut preview_succeeded = false;
        let mut receipt = NativeRunReceipt::default();
        let message = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目有多少素材？"),
            &mut preview_succeeded,
            &mut receipt,
            "当前项目有多少素材？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
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
        let mut preview_succeeded = false;
        let mut receipt = NativeRunReceipt::default();
        let result = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目有多少素材？"),
            &mut preview_succeeded,
            &mut receipt,
            "当前项目有多少素材？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            || false,
            |_body, _step| {},
        );
        assert_eq!(
            result,
            Ok("当前无法读取素材状态，我可以稍后再试。".to_owned())
        );
        assert!(!receipt.successful_observation_this_turn);
        assert!(receipt.failed_tool_call);
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
        let mut preview_succeeded = false;
        let result = drive_native_loop(
            &mut input,
            false,
            true,
            &RequestToolPolicy::from_request("当前项目的 storyboard 是什么？"),
            &mut preview_succeeded,
            &mut NativeRunReceipt::default(),
            "当前项目的 storyboard 是什么？",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            || false,
            |_body, _step| {},
        );
        assert_eq!(result, Err("native_tool_loop_max_steps".to_owned()));
    }

    #[test]
    fn model_claim_without_tool_cannot_complete_storyboard_request() {
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
        let mut preview_succeeded = false;
        let result = drive_native_loop(
            &mut input,
            false,
            false,
            &RequestToolPolicy::from_request("生成 storyboard"),
            &mut preview_succeeded,
            &mut NativeRunReceipt::default(),
            "生成 storyboard",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            || false,
            |_body, _step| {},
        );
        assert_eq!(result, Err("native_tool_loop_max_steps".to_owned()));
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
        let mut preview_succeeded = false;
        let message = drive_native_loop(
            &mut input,
            false,
            false,
            &RequestToolPolicy::from_request("分析素材并生成 storyboard，最后创建时间线"),
            &mut preview_succeeded,
            &mut NativeRunReceipt::default(),
            "分析素材并生成 storyboard，最后创建时间线",
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
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
            &mut preview_succeeded,
            &mut NativeRunReceipt::default(),
            "我确认这个 storyboard，请创建时间线",
            Instant::now() + Duration::from_secs(5),
            &mut respond_after_confirmation,
            &mut execute,
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
        assert!(requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "render_preview"));
        assert!(requests[1]["input"].as_array().unwrap().iter().any(|item| {
            item["type"] == "function_call_output" && item["call_id"] == "call_render_preview"
        }));
    }

    #[test]
    fn preview_request_cannot_finish_from_text_without_render_call() {
        let policy = RequestToolPolicy::from_request("帮我生成一个预览");
        let (message, requests, calls) = fixture_driver_with_policy(
            "帮我生成一个预览",
            vec![HELLO, RENDER_CALL, RENDER_REPLY],
            json!({
                "tool": "render_preview",
                "status": "ok",
                "artifact": {"type": "preview", "timelineVersionId": "timeline-1"}
            }),
            policy,
        );
        assert_eq!(message, "预览已生成，可以检查节奏和字幕。");
        assert_eq!(calls, ["render_preview"]);
        assert_eq!(requests.len(), 3);
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
            vec![RENDER_CALL, RENDER_FAILURE_REPLY],
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
    fn delivery_execution_rechecks_explicit_authorization() {
        let denied = RequestToolPolicy::from_request("Explain music options");
        let authorized =
            RequestToolPolicy::from_request("Download music and create a Jianying draft");
        for tool in [
            "download_music",
            "use_online_music",
            "replace_music_tracks",
            "replace_text_tracks",
            "create_jianying_draft",
        ] {
            assert!(!native_tool_call_allowed(tool, &denied, false), "{tool}");
        }
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
    fn delivery_requests_expose_only_their_authorized_native_tools() {
        let policy = RequestToolPolicy::from_request("添加字幕并替换背景音乐");
        let tools = filtered_native_tools(
            native_function_tools_for_request(false, policy.has_native_write_authorization()),
            &policy,
            false,
        );
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<std::collections::HashSet<_>>();
        assert!(names.contains("replace_text_tracks"));
        assert!(names.contains("replace_music_tracks"));
        for unauthorized in [
            "download_music",
            "use_online_music",
            "create_jianying_draft",
            "request_asset_analysis",
            "generate_storyboard",
        ] {
            assert!(!names.contains(unauthorized), "{unauthorized}");
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
            let mut preview_succeeded = false;
            let message = drive_native_loop(
                &mut input,
                false,
                false,
                &RequestToolPolicy::default(),
                &mut preview_succeeded,
                &mut NativeRunReceipt::default(),
                "fixture request",
                Instant::now() + Duration::from_secs(5),
                &mut respond,
                &mut execute,
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
            failed_tool_call: true,
            tool_called: true,
            ..NativeRunReceipt::default()
        };
        let (_result, status) = finish_native_result(
            "task-1",
            Ok("Storyboard 已生成。".to_owned()),
            None,
            false,
            false,
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
            false,
            false,
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
    fn preview_tool_requires_positive_intent_and_request_policy_permission() {
        assert!(native_render_preview_allowed(
            "生成预览",
            &RequestToolPolicy::from_request("生成预览")
        ));
        for request in [
            "你好",
            "只查看",
            "只检查，不要生成",
            "不要生成预览",
            "怎么生成预览？",
            "解释生成预览是什么意思",
        ] {
            assert!(!native_render_preview_allowed(
                request,
                &RequestToolPolicy::from_request(request)
            ));
        }
    }

    #[test]
    fn preview_execution_rechecks_positive_authorization() {
        let policy = RequestToolPolicy::from_request("你好");
        assert!(!native_tool_call_allowed("render_preview", &policy, false));
        assert!(native_tool_call_allowed(
            "render_preview",
            &RequestToolPolicy::from_request("生成预览"),
            true
        ));
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
            true,
            false,
            &NativeRunReceipt::default(),
        )
        .expect("partial preview result");
        assert_eq!(status, AgentLoopTerminalStatus::PartiallyCompleted);
        assert!(result.preview.is_some());
        assert!(result.message.contains("预览已生成"));
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
        let mut preview_succeeded = false;
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
            &mut preview_succeeded,
            &mut NativeRunReceipt::default(),
            request,
            Instant::now() + Duration::from_secs(5),
            &mut respond,
            &mut execute,
            || false,
            |_body, _step| {},
        )
        .expect("fixture loop");
        drop(execute);
        (message, requests, calls)
    }

    #[test]
    fn input_budget_drops_old_history_but_keeps_function_call_pair() {
        let mut input = vec![
            json!({"role": "system", "content": [{"type": "input_text", "text": "身份"}]}),
            json!({"role": "user", "content": [{"type": "input_text", "text": "很长的旧问题"}]}),
            json!({"role": "assistant", "content": [{"type": "output_text", "text": "很长的旧回答"}]}),
            json!({"role": "user", "content": [{"type": "input_text", "text": "当前问题"}]}),
            json!({"type": "function_call", "call_id": "call_1", "name": "list_assets", "arguments": "{}"}),
            json!({"type": "function_call_output", "call_id": "call_1", "output": "x".repeat(1000)}),
        ];

        trim_native_input_to_budget(&mut input, "当前问题", 400);

        assert!(input.iter().any(|item| item["type"] == "function_call"));
        assert!(input
            .iter()
            .any(|item| item["type"] == "function_call_output"));
        assert!(input
            .iter()
            .any(|item| item["role"] == "user" && item["content"][0]["text"] == "当前问题"));
        assert!(!input
            .iter()
            .any(|item| item.to_string().contains("很长的旧问题")));
    }
}
