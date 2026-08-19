//! 原生只读 Agent Loop。
//!
//! 只有显式开启 `NATIVE_TOOL_LOOP` 时才使用本模块。它消费 Provider 的统一
//! `ModelTurn`，把 Responses 的完整 output item 或 Chat 适配后的等价项目带入下一轮；
//! 工具执行仍由 `skills::apply_skill` 拥有，Legacy JSON decision loop 不经过这里。

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

use super::policy::{request_requires_project_observation, LoopGoal, RequestToolPolicy};
use super::schema::{
    AgentLoopResult, AgentLoopTerminalStatus, LoopState, AGENT_RUN_TIMEOUT, AGENT_STEP_TIMEOUT,
    MAX_STEPS,
};
use super::skills::{apply_skill, safe_step_error_code, safe_tool_failure_context};
use super::tools::native_observation_function_tools;

const NATIVE_TOOL_LOOP_ENV: &str = "NATIVE_TOOL_LOOP";
const NATIVE_TOOL_NAMES: &[&str] = &["get_asset_health_summary", "list_assets", "get_timeline"];

/// NativeToolLoop 是显式 opt-in；缺省值保持 Legacy Runtime。
pub(crate) fn native_tool_loop_enabled() -> bool {
    native_tool_loop_enabled_from(std::env::var(NATIVE_TOOL_LOOP_ENV).ok().as_deref())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_configured_loop(
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
    initial_skill: Option<super::schema::InitialAgentSkill>,
) -> Result<AgentLoopResult, String> {
    if native_tool_loop_enabled() && initial_skill.is_none() {
        return run_native_tool_loop(
            app,
            connection,
            agent_task_id,
            project_id,
            editing_task_id,
            conversation_id,
            request,
            task_brief,
            access,
            storyboard,
            timelines,
        );
    }
    if let Some(initial_skill) = initial_skill {
        return super::runtime::run_agent_loop_with_initial_skill(
            app,
            connection,
            agent_task_id,
            project_id,
            editing_task_id,
            conversation_id,
            request,
            task_brief,
            access,
            storyboard,
            timelines,
            Some(initial_skill),
        );
    }
    super::runtime::run_agent_loop(
        app,
        connection,
        agent_task_id,
        project_id,
        editing_task_id,
        conversation_id,
        request,
        task_brief,
        access,
        storyboard,
        timelines,
    )
}

fn native_tool_loop_enabled_from(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

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
    let history = super::prompt::load_message_history(connection, conversation_id, request);
    let mut input = vec![json!({
        "role": "system",
        "content": [{
            "type": "input_text",
            "text": "You are a read-only local video project assistant. Answer ordinary questions directly. For current project facts, use only the provided read-only functions before answering. Treat function outputs as the only project facts. If a function returns a structured failure, explain it safely or adjust with another allowed function; do not claim an edit or artifact was created."
        }]
    })];
    input.extend(
        history
            .into_iter()
            .filter_map(|(role, content)| {
                let role = match role.as_str() {
                    "user" => "user",
                    "agent" | "assistant" => "assistant",
                    _ => return None,
                };
                Some(json!({
                    "role": role,
                    "content": [{"type": "input_text", "text": content}]
                }))
            })
            .collect::<Vec<_>>(),
    );
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
        goal: LoopGoal::Question,
        goal_locked: true,
        tool_policy: RequestToolPolicy::from_request("只读"),
        pending_clarification: None,
        run_started_at,
        run_deadline,
        history: Vec::new(),
        storyboard: storyboard.cloned(),
        timelines: timelines.to_vec(),
        last_outcome: None,
        executed_steps: Vec::new(),
        last_failed_tool_error_code: None,
        project_fact_question: false,
        successful_observation: false,
    };
    let is_custom = access.custom_config().is_some();
    let mut respond =
        |payload: &Value, timeout: Duration| post_model_payload(access, payload, Some(timeout));
    let mut execute = |call: &FunctionCall, step_number: usize| {
        execute_native_tool(&mut state, call, step_number)
    };
    let cancelled = || native_task_cancelled(connection, agent_task_id);
    let message = drive_native_loop(
        &mut input,
        is_custom,
        request_requires_project_observation(request),
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
    )?;
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
    Ok(AgentLoopResult {
        result: AgentEditResult {
            agent_task_id: agent_task_id.to_owned(),
            message,
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        },
        status: AgentLoopTerminalStatus::Completed,
        goal: LoopGoal::Question,
    })
}

type NativeRespond<'a> = dyn FnMut(&Value, Duration) -> Result<String, String> + 'a;
type NativeExecute<'a> = dyn FnMut(&FunctionCall, usize) -> Result<Value, String> + 'a;

fn drive_native_loop(
    input: &mut Vec<Value>,
    is_custom: bool,
    requires_observation: bool,
    run_deadline: Instant,
    respond: &mut NativeRespond<'_>,
    execute: &mut NativeExecute<'_>,
    mut cancelled: impl FnMut() -> bool,
    mut observed: impl FnMut(&str, usize),
) -> Result<String, String> {
    let mut tool_called = false;
    for step_number in 1..=MAX_STEPS {
        if cancelled() {
            return Err("native_tool_loop_cancelled".to_owned());
        }
        let Some(timeout) = remaining_timeout(run_deadline) else {
            return Err("native_tool_loop_deadline_exceeded".to_owned());
        };
        let payload = json!({
            "model": "gpt-5.4",
            "store": false,
            "stream": false,
            "parallel_tool_calls": false,
            "tool_choice": "auto",
            "tools": native_observation_function_tools(),
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
            if let Some(message) = model_message_text(&turn) {
                if requires_observation && !tool_called {
                    input.push(json!({
                        "role": "system",
                        "content": [{
                            "type": "input_text",
                            "text": "This request asks about current project facts. Call one of the three read-only observation functions before answering."
                        }]
                    }));
                    continue;
                }
                return Ok(message);
            }
            return Err("native_tool_loop_response_missing_message".to_owned());
        }
        tool_called = true;

        for item in &turn.output {
            if let Some(value) = output_item_for_input(item, is_custom) {
                input.push(value);
            }
        }
        for call in calls {
            if cancelled() {
                return Err("native_tool_loop_cancelled".to_owned());
            }
            let result = execute(&call, step_number)?;
            input.push(json!({
                "type": "function_call_output",
                "call_id": call.call_id,
                "output": result.to_string(),
            }));
        }
    }
    Err("native_tool_loop_max_steps".to_owned())
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

fn execute_native_tool(
    state: &mut LoopState,
    call: &FunctionCall,
    step_number: usize,
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
            "responseInstruction": "Explain that only the three read-only observation tools are available, then answer from available facts or ask the user to rephrase."
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
            state.successful_observation = true;
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

fn parse_native_arguments(tool: &str, arguments: &str) -> Result<Value, Value> {
    let value = serde_json::from_str::<Value>(arguments).map_err(|_| invalid_arguments())?;
    let Some(object) = value.as_object() else {
        return Err(invalid_arguments());
    };
    match tool {
        "get_asset_health_summary" | "list_assets" if !object.is_empty() => {
            Err(invalid_arguments())
        }
        "get_timeline" => {
            if object.keys().any(|key| key != "timelineVersionId") {
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
        _ => Ok(value),
    }
}

fn invalid_arguments() -> Value {
    json!({
        "status": "failed",
        "operation": "native_observation",
        "stage": "argument_validation",
        "code": "invalid_arguments",
        "retryable": true,
        "responseInstruction": "Explain that the read-only observation request had invalid arguments, then retry with the documented schema or answer without a tool."
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
    fn switch_defaults_to_legacy_and_accepts_explicit_values() {
        assert!(!native_tool_loop_enabled_from(None));
        assert!(!native_tool_loop_enabled_from(Some("false")));
        assert!(native_tool_loop_enabled_from(Some("true")));
        assert!(native_tool_loop_enabled_from(Some("ON")));
    }

    #[test]
    fn ordinary_question_returns_message_without_tool_call() {
        let (message, requests, calls) = fixture_driver(vec![HELLO], json!({}));
        assert_eq!(message, "你好！有什么我可以帮你查看的吗？");
        assert!(calls.is_empty());
        assert_eq!(requests[0]["parallel_tool_calls"], false);
        assert_eq!(requests[0]["store"], false);
        assert_eq!(requests[0]["tools"].as_array().map(Vec::len), Some(3));
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
}
