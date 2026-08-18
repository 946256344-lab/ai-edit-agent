//! 有界循环、路由决策与步骤管理。
//!
//! `decide_conversation_route` 是首次模型决策入口，决定直接响应、澄清或进入 Agent
//! loop；`run_agent_loop` 执行有界多步循环，每步调用模型、执行技能、验证产物门。

use crate::audit::{begin_agent_run_step, finish_agent_run_step};
use crate::audit::{record_agent_diagnostic, record_agent_timing_diagnostic, AgentTimingMetric};
use crate::db::now_millis;
use crate::models::{
    AgentEditResult, PendingClarificationSnapshot, StoryboardVersion, TimelineVersion,
};
use crate::provider::{model_response_json_text, post_model_payload, ModelAccess};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::AppHandle;

use super::policy::{
    corrective_message, fast_goal, honest_no_change, honest_no_change_with_diagnostic,
    model_unavailable_message, parse_declared_goal, pinned_goal_allows_response,
    request_requires_project_observation, run_deadline_message, LoopGoal, RequestToolPolicy,
    EDIT_TOOLS, OBSERVATION_TOOLS,
};
use super::prompt::{
    build_agent_state_snapshot, build_step_prompt, deterministic_prerequisite_hints,
    load_message_history, load_pending_clarification, render_history,
};
use super::schema::{
    AgentLoopControl, AgentLoopResult, AgentLoopTerminalStatus, AgentStep, ExecutedStepSummary,
    InitialAgentSkill, LoopState, AGENT_RUN_TIMEOUT, AGENT_STEP_TIMEOUT, MAX_STEPS,
};
use super::skills::{
    apply_skill, persisted_artifact_for_tool, produced_artifact_for_tool, safe_failure_explanation,
    safe_step_error_code, safe_tool_failure_context,
    should_redirect_storyboard_after_failed_generation,
};

// ──────────────────────────────────────────────────────────────────────────────
// 会话路由决策
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) enum ConversationRouteDecision {
    Respond {
        message: String,
        resolved_clarification_id: Option<String>,
    },
    Clarify(String),
    Run {
        goal: LoopGoal,
        tool: String,
        args: Value,
        project_fact_question: bool,
        resolved_clarification_id: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationRouteResponse {
    route: String,
    goal: Option<String>,
    goal_reasoning: Option<String>,
    is_question: Option<bool>,
    tool: Option<String>,
    answer: Option<String>,
    question: Option<String>,
    clarification_action: Option<String>,
    information_scope: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn decide_conversation_route(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    request: &str,
    task_brief: &str,
    access: &ModelAccess,
    storyboard: Option<&StoryboardVersion>,
    timelines: &[TimelineVersion],
) -> Result<ConversationRouteDecision, String> {
    let route_deadline = Instant::now() + AGENT_RUN_TIMEOUT;
    let history = load_message_history(connection, conversation_id, request);
    let history_text = render_history(&history);
    let latest_run = connection
        .query_row(
            "SELECT status, tool_name, result_json FROM agent_tasks WHERE project_id = ?1 AND editing_task_id = ?2 AND conversation_id = ?3 AND tool_name NOT IN ('analyze_asset', 'analyze_asset_visual_batch', 'get_edit_status') ORDER BY created_at DESC LIMIT 1",
            params![project_id, editing_task_id, conversation_id],
            |row| {
                Ok(json!({
                    "status": row.get::<_, String>(0)?,
                    "tool": row.get::<_, String>(1)?,
                    "result": row
                        .get::<_, Option<String>>(2)?
                        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
                }))
            },
        )
        .optional()
        .map_err(|_| "Run status could not be read.".to_owned())?;
    let artifacts = json!({
        "storyboard": storyboard.map(|value| json!({"id": value.id, "versionNumber": value.version_number})),
        "timeline": timelines.iter().max_by_key(|value| value.version_number).map(|value| json!({"id": value.id, "versionNumber": value.version_number})),
    });
    let pending_clarification =
        load_pending_clarification(connection, project_id, editing_task_id, conversation_id)?;
    let tool_policy = RequestToolPolicy::from_request(request);
    let pinned_goal = fast_goal(request);
    let prompt = format!(
        "You route one user turn for a local video-editing Agent. Decide whether to respond now, ask a clarification now, or start a real Agent run. Do not claim any artifact that is absent from the authoritative snapshot.\n\n\
         Current request: {request}\nTask brief: {task_brief}\nRecent conversation:\n{history_text}\n\n\
         Latest scoped run: {latest_run}\nScoped artifacts: {artifacts}\nPending clarification: {pending_clarification}\nBackend-pinned goal: {pinned_goal}\n\
         User-denied side-effect tools for this request: {denied_tools}. Never choose one of these tools or declare a goal whose deliverable requires one of them.\n\n\
         Artifact boundary responsibilities:\n\
         - storyboard: Shot selection and narrative structure FROM raw media. Chooses which video/image segments to use and their source time ranges. This is the FIRST creative step that turns raw footage into a story outline.\n\
         - timeline: Edits an EXISTING storyboard structure. Includes: adjusting clip durations, reordering clips, adding/editing text tracks (subtitles, captions, 字幕, 文案), adding/editing music tracks, color grading references. Timeline operations work on already-selected shots.\n\
         - preview: Renders a playable video file from a timeline for review.\n\
         - jianyingDraft: Exports the timeline to Jianying Pro format for final delivery.\n\n\
         Important distinctions:\n\
         - Text/subtitle editing (字幕, 配音文本, 文案整理, subtitle, caption) belongs to goal=timeline with tool=replace_text_tracks, NOT goal=storyboard.\n\
         - Music editing (音乐, 背景音乐, music) belongs to goal=timeline with tool=replace_music_tracks, NOT goal=storyboard.\n\
         - Shot selection or narrative restructuring from raw media belongs to goal=storyboard.\n\
         - Editing existing shot durations/order belongs to goal=timeline.\n\n\
         Return one JSON object. route must be respond, clarify, or run.\n\
         Valid goal values (required for route=run): question, storyboard, timeline, preview, jianying. Choose based on the artifact boundary above.\n\
         - goal=question: answering a question by observing project state (use informationScope=general or project)\n\
         - goal=storyboard: creating initial shot selection from raw media (first tool: generate_storyboard)\n\
         - goal=timeline: editing existing storyboard/timeline structure (tools: create_timeline_draft, replace_text_tracks, replace_music_tracks)\n\
         - goal=preview: rendering video preview (first tool: render_preview)\n\
         - goal=jianying: exporting to Jianying format (first tool: create_jianying_draft)\n\n\
         Route decision rules:\n\
         - respond: only for general conversational answers that need no tool or side effect. Include goal=question, isQuestion=true, informationScope=general, answer.\n\
         - clarify: only when a genuinely required input is missing. Include question.\n\
         - run: for observation requiring project details, media analysis, storyboard/timeline edits, preview, or Jianying delivery. Include goal (one of the 5 valid values above), goalReasoning (explain WHY this request belongs to the chosen artifact boundary), isQuestion=false unless this is an observation question, and choose the FIRST tool now. Tool arguments stay at the JSON top level.\n\
         When pendingClarification is not null, respond and run must include clarificationAction=keep or resolve. Resolve only when this turn answers or explicitly abandons that question; keep it for unrelated turns. A new clarify route replaces the old question.\n\
         A long narration/script supplied after the Agent requested a creative goal is normally a creative input, even when its heading is a rhetorical question. Exact completion facts come only from latestRun/artifacts. The backend-pinned goal, when not null, is authoritative.\n\n\
         Available first tools: {tools}. Return JSON only.",
        latest_run = latest_run.unwrap_or(Value::Null),
        artifacts = artifacts,
        pending_clarification = serde_json::to_value(&pending_clarification).map_err(|e| e.to_string())?,
        pinned_goal = pinned_goal.map(|goal| goal.code()).unwrap_or("pending"),
        denied_tools = tool_policy.prompt_label(),
        tools = OBSERVATION_TOOLS
            .iter()
            .chain(EDIT_TOOLS.iter())
            .copied()
            .filter(|tool| !tool_policy.forbids(tool))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let request_body = json!({
        "model": "gpt-5.4",
        "store": false,
        "stream": true,
        "input": [{ "role": "user", "content": [{ "type": "input_text", "text": prompt }] }],
        "text": { "format": { "type": "json_object" } }
    });
    let body = post_model_payload(access, &request_body, Some(AGENT_RUN_TIMEOUT))?;
    let text = model_response_json_text(access, &body)
        .ok_or_else(|| "Route response had no JSON.".to_owned())?;
    let mut raw: Value =
        serde_json::from_str(&text).map_err(|_| "Route response was malformed.".to_owned())?;
    let mut response: ConversationRouteResponse = serde_json::from_value(raw.clone())
        .map_err(|_| "Route response schema invalid.".to_owned())?;

    // 日志记录路由决策的原始值
    log::info!(
        "Route decision received: route={}, goal={:?}, isQuestion={:?}, tool={:?}, pinnedGoal={:?}",
        response.route,
        response.goal,
        response.is_question,
        response.tool,
        pinned_goal.map(|g| g.code())
    );

    // 纠偏重试：验证失败时把错误原因反馈给模型。
    match try_build_route_decision(
        &response,
        &raw,
        pinned_goal,
        &tool_policy,
        &pending_clarification,
    ) {
        Ok(d) => return Ok(d),
        Err(hint) => {
            let timeout = remaining_model_timeout(route_deadline, Instant::now())
                .ok_or_else(|| "Route correction budget exhausted.".to_owned())?;
            let prev = serde_json::to_string(&raw).map_err(|e| format!("Correction ctx: {e}"))?;
            let cp = format!("{prompt}\n\nYour previous response: {prev}\n\nIssue: {hint} Return corrected JSON only.");
            let cb = json!({"model":"gpt-5.4","store":false,"stream":true,"input":[{"role":"user","content":[{"type":"input_text","text":cp}]}],"text":{"format":{"type":"json_object"}}});
            let rb = post_model_payload(access, &cb, Some(timeout))?;
            let rt = model_response_json_text(access, &rb)
                .ok_or_else(|| "Route correction had no JSON.".to_owned())?;
            raw = serde_json::from_str(&rt)
                .map_err(|_| "Route correction was malformed.".to_owned())?;
            response = serde_json::from_value(raw.clone())
                .map_err(|_| "Route correction schema invalid.".to_owned())?;

            // 日志记录纠偏后的值
            log::info!(
                "Route correction received: route={}, goal={:?}, isQuestion={:?}, tool={:?}",
                response.route,
                response.goal,
                response.is_question,
                response.tool
            );
        }
    }
    try_build_route_decision(
        &response,
        &raw,
        pinned_goal,
        &tool_policy,
        &pending_clarification,
    )
}

pub(super) fn clarification_resolution(
    pending: Option<&PendingClarificationSnapshot>,
    action: Option<&str>,
) -> Result<Option<String>, String> {
    match (pending, action) {
        (None, None | Some("keep")) | (Some(_), Some("keep")) => Ok(None),
        (Some(pending), Some("resolve")) => Ok(Some(pending.id.clone())),
        _ => Err("Clarification action was invalid.".to_owned()),
    }
}

/// 路由决策构建器；失败原因字符串即作为纠偏提示。
#[rustfmt::skip]
fn try_build_route_decision(
    response: &ConversationRouteResponse, raw: &Value, pinned_goal: Option<LoopGoal>,
    tool_policy: &RequestToolPolicy, pending_clarification: &Option<PendingClarificationSnapshot>,
) -> Result<ConversationRouteDecision, String> {
    match response.route.as_str() {
        "respond" => {
            let resolved = clarification_resolution(pending_clarification.as_ref(), response.clarification_action.as_deref())?;
            let answer = response.answer.as_deref().unwrap_or("").to_owned();
            if answer.trim().is_empty() || response.tool.is_some()
                || parse_declared_goal(response.goal.as_deref(), response.is_question) != Some(LoopGoal::Question)
                || !question_scope_allows_route(response.information_scope.as_deref(), "respond")
                || !pinned_goal_allows_response(pinned_goal) {
                return Err("route=respond: answer empty, tool present, or wrong goal/scope/pinned.".to_owned());
            }
            Ok(ConversationRouteDecision::Respond { message: answer, resolved_clarification_id: resolved })
        }
        "clarify" => {
            let question = response.question.clone().or_else(|| response.answer.clone()).unwrap_or_default();
            if question.trim().is_empty() || response.tool.is_some() {
                return Err("route=clarify: empty question or tool present.".to_owned());
            }
            Ok(ConversationRouteDecision::Clarify(question))
        }
        "run" => {
            let resolved = clarification_resolution(pending_clarification.as_ref(), response.clarification_action.as_deref())?;
            let declared = parse_declared_goal(response.goal.as_deref(), response.is_question);
            // pinned_goal 优先；declared 仅作后备，避免 pinned 存在时因模型漏填 goal 而失败
            let goal = pinned_goal.or(declared)
                .ok_or_else(|| {
                    log::warn!(
                        "Route validation failed: goal parsing failed. raw_goal={:?}, isQuestion={:?}, pinned={:?}",
                        response.goal,
                        response.is_question,
                        pinned_goal.map(|g| g.code())
                    );
                    "route=run: goal must be question/storyboard/timeline/preview/jianying.".to_owned()
                })?;
            if tool_policy.forbids_goal(goal) {
                return Err(format!("goal='{}' is user-denied.", goal.code()));
            }
            // 当 goal 不是由 pinned_goal 决定时，要求模型提供 goalReasoning
            if pinned_goal.is_none() && goal != LoopGoal::Question {
                let reasoning = response.goal_reasoning.as_deref().unwrap_or("").trim();
                if reasoning.is_empty() {
                    return Err("route=run: goalReasoning required when goal is not pinned by backend.".to_owned());
                }
                // 基础验证：reasoning 应该提及目标产物
                let reasoning_lower = reasoning.to_lowercase();
                let mentions_goal = match goal {
                    LoopGoal::Storyboard => reasoning_lower.contains("storyboard") || reasoning_lower.contains("shot") || reasoning_lower.contains("narrative"),
                    LoopGoal::Timeline => reasoning_lower.contains("timeline") || reasoning_lower.contains("edit") || reasoning_lower.contains("text") || reasoning_lower.contains("music") || reasoning_lower.contains("duration") || reasoning_lower.contains("字幕") || reasoning_lower.contains("文案"),
                    LoopGoal::Preview => reasoning_lower.contains("preview") || reasoning_lower.contains("render") || reasoning_lower.contains("预览"),
                    LoopGoal::JianyingDraft => reasoning_lower.contains("jianying") || reasoning_lower.contains("剪映") || reasoning_lower.contains("draft"),
                    _ => true,
                };
                if !mentions_goal {
                    return Err(format!("route=run: goalReasoning does not mention the chosen goal artifact '{}'.", goal.code()));
                }
            }
            if goal == LoopGoal::Question && !question_scope_allows_route(response.information_scope.as_deref(), "run") {
                return Err("route=run question: informationScope must be general or project.".to_owned());
            }
            let tool = response.tool.clone().unwrap_or_default();
            if !OBSERVATION_TOOLS.contains(&tool.as_str()) && !EDIT_TOOLS.contains(&tool.as_str()) {
                let allowed = OBSERVATION_TOOLS.iter().chain(EDIT_TOOLS.iter()).copied()
                    .filter(|t| !tool_policy.forbids(t)).collect::<Vec<_>>().join(", ");
                return Err(format!("tool='{tool}' not in allowed list. Use one of: {allowed}."));
            }
            if tool_policy.forbids(&tool) { return Err(format!("tool='{tool}' is user-denied.")); }
            let pfq = goal == LoopGoal::Question && response.information_scope.as_deref() == Some("project");
            if pfq && !OBSERVATION_TOOLS.contains(&tool.as_str()) {
                return Err("project-scoped question must start with an observation tool.".to_owned());
            }
            Ok(ConversationRouteDecision::Run { goal, tool, args: super::schema::step_args(raw), project_fact_question: pfq, resolved_clarification_id: resolved })
        }
        r => Err(format!("route='{r}' unknown. Must be respond, clarify, or run.")),
    }
}

pub(super) fn question_scope_allows_route(scope: Option<&str>, route: &str) -> bool {
    matches!(
        (scope, route),
        (Some("general"), "respond" | "run") | (Some("project"), "run")
    )
}

// ──────────────────────────────────────────────────────────────────────────────
// 有界 Agent 循环
// ──────────────────────────────────────────────────────────────────────────────

pub(crate) fn run_agent_loop(
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
    run_agent_loop_with_initial_skill(
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
        None,
    )
}

pub(crate) fn run_agent_loop_with_initial_skill(
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
    initial_skill: Option<InitialAgentSkill>,
) -> Result<AgentLoopResult, String> {
    // Router 已经选择的首个技能直接作为 step 1 复用；不能再次请求模型而产生两次不一致决策。
    let history = load_message_history(connection, conversation_id, request);
    let tool_policy = RequestToolPolicy::from_request(request);
    let initial_goal = initial_skill
        .as_ref()
        .map(|skill| skill.goal)
        .filter(|goal| !tool_policy.forbids_goal(*goal))
        .or_else(|| fast_goal(request));
    let goal = initial_goal.unwrap_or(LoopGoal::Question);
    let routed_project_fact_question = initial_skill
        .as_ref()
        .is_some_and(|skill| skill.project_fact_question);
    let project_fact_question = routed_project_fact_question
        || (request_requires_project_observation(request)
            && initial_goal.map_or(true, |goal| goal == LoopGoal::Question));
    let run_started_at = Instant::now();
    let run_deadline = run_started_at + AGENT_RUN_TIMEOUT;
    let pending_clarification =
        load_pending_clarification(connection, project_id, editing_task_id, conversation_id)?;
    let mut state = LoopState {
        app,
        connection,
        agent_task_id,
        project_id,
        editing_task_id,
        conversation_id,
        task_brief: task_brief.to_owned(),
        goal,
        goal_locked: initial_goal.is_some(),
        tool_policy,
        pending_clarification,
        run_started_at,
        run_deadline,
        history,
        storyboard: storyboard.cloned(),
        timelines: timelines.to_vec(),
        last_outcome: None,
        executed_steps: Vec::new(),
        last_failed_tool_error_code: None,
        project_fact_question,
        successful_observation: false,
    };
    let mut transcript: Vec<Value> = vec![json!({
        "role": "user",
        "content": format!("Agent request: {request}\nTask brief: {task_brief}")
    })];

    let mut terminated = false;
    let mut failed = false;
    let mut needs_clarification = false;
    if let Some(initial_skill) = initial_skill {
        execute_initial_skill(&mut state, &mut transcript, initial_skill)?;
    }
    let first_model_step = first_model_step(&state.executed_steps);
    for step_index in first_model_step..MAX_STEPS {
        match run_step(&mut state, access, &mut transcript, step_index + 1) {
            Ok(AgentLoopControl::Done) => {
                terminated = true;
                break;
            }
            Ok(AgentLoopControl::PartiallyDone) => {
                terminated = true;
                failed = true;
                break;
            }
            Ok(AgentLoopControl::Failed) => {
                terminated = true;
                failed = state.goal == LoopGoal::Question
                    || !state.goal.satisfied_by(&state.last_outcome);
                if failed {
                    let message = if let Some(code) = state.last_failed_tool_error_code {
                        honest_no_change_with_diagnostic(state.goal, code)
                    } else {
                        honest_no_change(state.goal)
                    };
                    state.last_outcome = Some(finalize_result_helper(
                        agent_task_id,
                        state.last_outcome.take(),
                        &message,
                    ));
                }
                break;
            }
            Ok(AgentLoopControl::ExplainedFailure) => {
                terminated = true;
                failed = true;
                break;
            }
            Ok(AgentLoopControl::NeedsClarification) => {
                terminated = true;
                needs_clarification = true;
                break;
            }
            Ok(AgentLoopControl::DeadlineExceeded) => {
                terminated = true;
                failed = state.goal == LoopGoal::Question
                    || !state.goal.satisfied_by(&state.last_outcome);
                let _ = record_agent_diagnostic(
                    state.connection,
                    state.project_id,
                    state.editing_task_id,
                    state.conversation_id,
                    state.agent_task_id,
                    None,
                    "pipeline_error",
                    "run_deadline_exceeded",
                );
                if failed {
                    state.last_outcome = Some(finalize_result_helper(
                        agent_task_id,
                        state.last_outcome.take(),
                        &run_deadline_message(state.goal),
                    ));
                }
                break;
            }
            Ok(AgentLoopControl::Continue) => {}
            Err(error) => {
                log::warn!("AI agent-loop step aborted by a model error: {error}");
                terminated = true;
                failed = state.goal == LoopGoal::Question
                    || !state.goal.satisfied_by(&state.last_outcome);
                if failed {
                    state.last_outcome = Some(finalize_result_helper(
                        agent_task_id,
                        state.last_outcome.take(),
                        &model_unavailable_message(state.goal, &error),
                    ));
                }
                break;
            }
        }
    }
    if !terminated
        && (state.goal == LoopGoal::Question || !state.goal.satisfied_by(&state.last_outcome))
    {
        failed = true;
    }
    if failed && !terminated && state.last_outcome.is_none() {
        let message = if let Some(code) = state.last_failed_tool_error_code {
            honest_no_change_with_diagnostic(state.goal, code)
        } else {
            honest_no_change(state.goal)
        };
        state.last_outcome = Some(finalize_result_helper(
            agent_task_id,
            state.last_outcome.take(),
            &message,
        ));
    }
    let status = if needs_clarification {
        AgentLoopTerminalStatus::NeedsClarification
    } else if failed && result_has_artifact(&state.last_outcome) {
        AgentLoopTerminalStatus::PartiallyCompleted
    } else if failed {
        AgentLoopTerminalStatus::Failed
    } else {
        AgentLoopTerminalStatus::Completed
    };
    let _ = record_agent_timing_diagnostic(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        None,
        AgentTimingMetric::RunTotal,
        state.run_started_at.elapsed(),
    );
    Ok(AgentLoopResult {
        result: finalize_result(
            agent_task_id,
            state.last_outcome,
            &honest_no_change(state.goal),
        ),
        status,
        goal: state.goal,
    })
}

pub(super) fn first_model_step(executed_steps: &[ExecutedStepSummary]) -> usize {
    usize::from(!executed_steps.is_empty())
}

pub(crate) fn run_explicit_command(
    app: &AppHandle,
    connection: &Connection,
    agent_task_id: &str,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    task_brief: &str,
    tool: &str,
    timeline_version_id: Option<&str>,
    storyboard: Option<&StoryboardVersion>,
    timelines: &[TimelineVersion],
) -> Result<AgentEditResult, String> {
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
        tool_policy: RequestToolPolicy::default(),
        pending_clarification: None,
        run_started_at: Instant::now(),
        run_deadline: Instant::now() + AGENT_RUN_TIMEOUT,
        history: Vec::new(),
        storyboard: storyboard.cloned(),
        timelines: timelines.to_vec(),
        last_outcome: None,
        executed_steps: Vec::new(),
        last_failed_tool_error_code: None,
        project_fact_question: false,
        successful_observation: false,
    };
    let mut args = json!({});
    if let Some(timeline_id) = timeline_version_id {
        args["timelineVersionId"] = json!(timeline_id);
    }
    let step_id = begin_agent_run_step(
        connection,
        project_id,
        editing_task_id,
        agent_task_id,
        1,
        tool,
    )?;
    if let Err(error) = apply_skill(&mut state, tool, &args) {
        finish_agent_run_step(
            connection,
            project_id,
            editing_task_id,
            agent_task_id,
            &step_id,
            "failed",
            None,
            None,
            Some(safe_step_error_code(&error)),
        )?;
        return Err(error);
    }
    let artifact = persisted_artifact_for_tool(&state, tool);
    finish_agent_run_step(
        connection,
        project_id,
        editing_task_id,
        agent_task_id,
        &step_id,
        "completed",
        artifact.as_ref().map(|(kind, _)| *kind),
        artifact.as_ref().map(|(_, id)| id.as_str()),
        None,
    )?;
    Ok(state
        .last_outcome
        .take()
        .unwrap_or_else(|| AgentEditResult {
            agent_task_id: agent_task_id.to_owned(),
            message: "已处理该请求，但没有产生新的 storyboard、时间线、preview 或剪映草稿。"
                .to_owned(),
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        }))
}

fn reject_user_restricted_tool(
    state: &mut LoopState,
    transcript: &mut Vec<Value>,
    step_id: &str,
    step_number: usize,
    tool: &str,
) -> Result<bool, String> {
    if !state.tool_policy.forbids(tool) {
        return Ok(false);
    }
    finish_agent_run_step(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.agent_task_id,
        step_id,
        "failed",
        None,
        None,
        Some("user_restricted_tool"),
    )?;
    record_agent_diagnostic(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.conversation_id,
        state.agent_task_id,
        Some(step_number as i64),
        "tool_error",
        "user_restricted_tool",
    )?;
    state.last_failed_tool_error_code = Some("user_restricted_tool");
    state.executed_steps.push(ExecutedStepSummary {
        step_number,
        tool: tool.to_owned(),
        status: "blocked".to_owned(),
        produced_artifact: None,
    });
    transcript.push(json!({
        "role": "system",
        "content": "The selected side-effect tool is explicitly excluded by the current user request. Do not retry it. Choose an allowed tool that stays within the request, or finish truthfully."
    }));
    Ok(true)
}

fn execute_initial_skill(
    state: &mut LoopState,
    transcript: &mut Vec<Value>,
    initial: InitialAgentSkill,
) -> Result<(), String> {
    let step_number = 1;
    let step_id = begin_agent_run_step(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.agent_task_id,
        step_number,
        &initial.tool,
    )?;
    if reject_user_restricted_tool(
        state,
        transcript,
        &step_id,
        step_number as usize,
        &initial.tool,
    )? {
        return Ok(());
    }
    let started_at = Instant::now();
    match apply_skill(state, &initial.tool, &initial.args) {
        Ok(context) => {
            if OBSERVATION_TOOLS.contains(&initial.tool.as_str()) {
                state.successful_observation = true;
            }
            let artifact = persisted_artifact_for_tool(state, &initial.tool);
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
            state.executed_steps.push(ExecutedStepSummary {
                step_number: step_number as usize,
                tool: initial.tool.clone(),
                status: "succeeded".to_owned(),
                produced_artifact: produced_artifact_for_tool(&initial.tool).map(str::to_owned),
            });
            transcript.push(json!({
                "role": "tool",
                "tool": initial.tool,
                "content": context.to_string()
            }));
        }
        Err(error) => {
            let error_code = safe_step_error_code(&error);
            let diagnostic = safe_tool_failure_context(&initial.tool, &error);
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &step_id,
                "failed",
                None,
                None,
                Some(error_code),
            )?;
            state.last_failed_tool_error_code = Some(error_code);
            state.executed_steps.push(ExecutedStepSummary {
                step_number: step_number as usize,
                tool: initial.tool.clone(),
                status: "failed".to_owned(),
                produced_artifact: None,
            });
            transcript.push(json!({
                "role": "tool",
                "content": diagnostic
            }));
        }
    }
    let _ = record_agent_timing_diagnostic(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.conversation_id,
        state.agent_task_id,
        Some(1),
        AgentTimingMetric::SkillExecution,
        started_at.elapsed(),
    );
    Ok(())
}

/// Runs a single skill decision: prompts the model, executes exactly one skill,
/// and records the result in the transcript. A terminal decision is only
/// honored when the loop goal is satisfied; otherwise a corrective message is
/// fed back and the loop continues.
fn run_step(
    state: &mut LoopState,
    access: &ModelAccess,
    transcript: &mut Vec<Value>,
    step_number: usize,
) -> Result<AgentLoopControl, String> {
    let agent_task_id = state.agent_task_id;
    let snapshot = build_agent_state_snapshot(state, MAX_STEPS.saturating_sub(step_number - 1))?;
    let prerequisite_hints = deterministic_prerequisite_hints(&snapshot);
    let prompt = build_step_prompt(state, transcript, &snapshot, &prerequisite_hints)?;
    let request_body = json!({
        "model": "gpt-5.4",
        "store": false,
        "stream": true,
        "input": [{ "role": "user", "content": [{ "type": "input_text", "text": prompt }] }],
        "text": { "format": { "type": "json_object" } }
    });
    let Some(timeout) = remaining_model_timeout(state.run_deadline, Instant::now()) else {
        return Ok(AgentLoopControl::DeadlineExceeded);
    };
    let model_started_at = Instant::now();
    let body = match post_model_payload(access, &request_body, Some(timeout)) {
        Ok(body) => body,
        Err(error) => {
            let _ = record_agent_timing_diagnostic(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                Some(step_number as i64),
                AgentTimingMetric::ModelRequest,
                model_started_at.elapsed(),
            );
            if Instant::now() >= state.run_deadline {
                return Ok(AgentLoopControl::DeadlineExceeded);
            }
            log::warn!("AI agent-loop step request failed: {error}");
            let _ = record_agent_diagnostic(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                Some(step_number as i64),
                "pipeline_error",
                "provider_request_failed",
            );
            return Err(error);
        }
    };
    let _ = record_agent_timing_diagnostic(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.conversation_id,
        state.agent_task_id,
        Some(step_number as i64),
        AgentTimingMetric::ModelRequest,
        model_started_at.elapsed(),
    );
    let text = match model_response_json_text(access, &body) {
        Some(text) => text,
        None => {
            log::warn!(
                "AI agent-loop step did not contain decision JSON (response length: {}).",
                body.len()
            );
            let _ = record_agent_diagnostic(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                Some(step_number as i64),
                "model_response",
                "decision_json_missing",
            );
            return Err("Step response had no JSON.".to_owned());
        }
    };
    record_agent_diagnostic(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.conversation_id,
        state.agent_task_id,
        Some(step_number as i64),
        "model_response",
        &format!("decision_json_received_bytes={}", text.len()),
    )?;
    let Ok(raw) = serde_json::from_str::<Value>(&text) else {
        log::warn!("Agent-loop step returned malformed JSON; ending the loop.");
        record_agent_diagnostic(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.conversation_id,
            state.agent_task_id,
            Some(step_number as i64),
            "model_response",
            "decision_json_malformed",
        )?;
        return Ok(AgentLoopControl::Failed);
    };
    let step: AgentStep = match serde_json::from_value(raw.clone()) {
        Ok(step) => step,
        Err(_) => {
            log::warn!("Agent-loop step schema did not match; ending the loop.");
            record_agent_diagnostic(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                Some(step_number as i64),
                "model_response",
                "decision_schema_invalid",
            )?;
            return Ok(AgentLoopControl::Failed);
        }
    };
    if !state.goal_locked {
        let Some(goal) = parse_declared_goal(step.goal.as_deref(), step.is_question) else {
            let _ = record_agent_diagnostic(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                Some(step_number as i64),
                "model_response",
                "goal_declaration_invalid",
            );
            return Ok(AgentLoopControl::Failed);
        };
        if state.tool_policy.forbids_goal(goal) {
            record_agent_diagnostic(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                Some(step_number as i64),
                "model_response",
                "goal_declaration_restricted",
            )?;
            transcript.push(json!({
                "role": "system",
                "content": "The declared goal requires a side effect that the current user request explicitly excludes. Choose an allowed goal and tool, or finish truthfully without claiming the excluded deliverable."
            }));
            return Ok(AgentLoopControl::Continue);
        }
        state.project_fact_question = goal == LoopGoal::Question && state.project_fact_question;
        state.goal = goal;
        state.goal_locked = true;
    }
    let tool = step.tool.clone().unwrap_or_default();
    if Instant::now() >= state.run_deadline {
        return Ok(AgentLoopControl::DeadlineExceeded);
    }
    log::info!(
        "Agent loop step. Task {} chose tool `{}`.",
        agent_task_id,
        tool
    );
    transcript.push(json!({ "role": "assistant", "content": tool }));

    let recorded_tool = if tool.is_empty() {
        "no_action"
    } else if EDIT_TOOLS.contains(&tool.as_str())
        || OBSERVATION_TOOLS.contains(&tool.as_str())
        || matches!(tool.as_str(), "ask_user" | "finish" | "done" | "no_action")
    {
        tool.as_str()
    } else {
        "unknown_tool"
    };
    let persisted_step_id = begin_agent_run_step(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.agent_task_id,
        step_number as i64,
        recorded_tool,
    )?;

    if reject_user_restricted_tool(state, transcript, &persisted_step_id, step_number, &tool)? {
        return Ok(AgentLoopControl::Continue);
    }

    if !state.tool_policy.read_only && recorded_tool != "unknown_tool" {
        if let Some(brief) = step
            .task_brief
            .as_deref()
            .map(str::trim)
            .filter(|brief| !brief.is_empty())
        {
            state.task_brief = brief.to_owned();
            let _ = state.connection.execute(
                "UPDATE editing_tasks SET brief = ?1, title = CASE WHEN title IN ('新的剪辑任务', '新的剪辑会话') THEN substr(?1, 1, 28) ELSE title END, updated_at = ?2 WHERE id = ?3",
                params![brief, now_millis(), state.editing_task_id],
            );
        }
    }

    if tool.is_empty() || tool == "no_action" || tool == "finish" || tool == "done" {
        if state.project_fact_question && !state.successful_observation {
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &persisted_step_id,
                "failed",
                None,
                None,
                Some("project_observation_required"),
            )?;
            transcript.push(json!({ "role": "system", "content": "This is a project-fact question. You must successfully call a read-only observation skill before answering." }));
            return Ok(AgentLoopControl::Continue);
        }
        if state.goal.satisfied_by(&state.last_outcome) {
            let reply = step.answer.clone().unwrap_or_default();
            let previous = state.last_outcome.take();
            state.last_outcome = Some(finalize_terminal(
                agent_task_id,
                state.goal,
                previous,
                &reply,
            ));
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &persisted_step_id,
                "completed",
                None,
                None,
                None,
            )?;
            return Ok(AgentLoopControl::Done);
        }
        if state.last_outcome.is_none() && state.last_failed_tool_error_code.is_some() {
            let explanation = step.answer.clone().unwrap_or_default();
            if safe_failure_explanation(&explanation) {
                state.last_outcome = Some(AgentEditResult {
                    agent_task_id: agent_task_id.to_owned(),
                    message: format!("任务未完成。{}", explanation.trim()),
                    storyboard: None,
                    timeline: None,
                    preview: None,
                    jianying_draft: None,
                });
                finish_agent_run_step(
                    state.connection,
                    state.project_id,
                    state.editing_task_id,
                    state.agent_task_id,
                    &persisted_step_id,
                    "completed",
                    None,
                    None,
                    None,
                )?;
                return Ok(AgentLoopControl::ExplainedFailure);
            }
        }
        if let Some(partial) = state.last_outcome.take() {
            // Keep the verified intermediate artifact, but do not stop the loop
            // until the requested deliverable is actually satisfied.
            state.last_outcome = Some(partial);
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &persisted_step_id,
                "completed",
                None,
                None,
                None,
            )?;
            transcript.push(json!({
                "role": "system",
                "content": "当前已经有可审阅的中间产物，但请求的最终交付还没完成；继续下一步，不要把中间产物当作终点。"
            }));
            return Ok(AgentLoopControl::Continue);
        }
        log::info!(
            "Loop goal {:?} not satisfied; asking the model to continue.",
            state.goal
        );
        finish_agent_run_step(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.agent_task_id,
            &persisted_step_id,
            "failed",
            None,
            None,
            Some("goal_not_satisfied"),
        )?;
        transcript.push(json!({ "role": "system", "content": corrective_message(state.goal) }));
        return Ok(AgentLoopControl::Continue);
    }
    if tool == "ask_user" {
        if should_redirect_storyboard_after_failed_generation(
            state.goal,
            state.last_failed_tool_error_code,
        ) {
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &persisted_step_id,
                "failed",
                None,
                None,
                Some("storyboard_generation_failed"),
            )?;
            transcript.push(json!({
                "role": "system",
                "content": "The user already supplied a complete editing brief. A prior storyboard attempt failed local validation. Do not ask again for theme, style, duration, audience, or the brief. Use the existing brief: either retry generate_storyboard with a valid, evidence-bound plan, or give a helpful natural-language explanation without claiming that an artifact was created."
            }));
            return Ok(AgentLoopControl::Continue);
        }
        let question = step
            .question
            .clone()
            .or_else(|| step.answer.clone())
            .unwrap_or_else(|| "请补充素材或希望保留的具体片段。".to_owned());
        state.last_outcome = Some(match state.last_outcome.take() {
            Some(mut existing) => {
                existing.message = question;
                existing
            }
            None => AgentEditResult {
                agent_task_id: agent_task_id.to_owned(),
                message: question,
                storyboard: None,
                timeline: None,
                preview: None,
                jianying_draft: None,
            },
        });
        finish_agent_run_step(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.agent_task_id,
            &persisted_step_id,
            "completed",
            None,
            None,
            None,
        )?;
        return Ok(AgentLoopControl::NeedsClarification);
    }

    let args = super::schema::step_args(&raw);
    if state.project_fact_question && EDIT_TOOLS.contains(&tool.as_str()) {
        finish_agent_run_step(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.agent_task_id,
            &persisted_step_id,
            "failed",
            None,
            None,
            Some("project_question_read_only"),
        )?;
        transcript.push(json!({ "role": "system", "content": "Project-fact questions are read-only. Choose an observation skill, or finish only after a successful observation." }));
        return Ok(AgentLoopControl::Continue);
    }
    if !EDIT_TOOLS.contains(&tool.as_str()) && !OBSERVATION_TOOLS.contains(&tool.as_str()) {
        log::warn!("Agent loop tried an unknown skill `{tool}`; asking the model to retry.");
        finish_agent_run_step(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.agent_task_id,
            &persisted_step_id,
            "failed",
            None,
            None,
            Some("tool_not_allowed"),
        )?;
        transcript.push(json!({
            "role": "system",
            "content": format!(
                "技能\u{201c}{tool}\u{201d}不可用。可用的观察技能：{}；编辑/交付技能：{}。请重新选择。",
                OBSERVATION_TOOLS.join(", "),
                EDIT_TOOLS.join(", ")
            )
        }));
        return Ok(AgentLoopControl::Continue);
    }

    if Instant::now() >= state.run_deadline {
        finish_agent_run_step(
            state.connection,
            state.project_id,
            state.editing_task_id,
            state.agent_task_id,
            &persisted_step_id,
            "failed",
            None,
            None,
            Some("run_deadline_exceeded"),
        )?;
        return Ok(AgentLoopControl::DeadlineExceeded);
    }
    let skill_started_at = Instant::now();
    let skill_result = apply_skill(state, &tool, &args);
    let _ = record_agent_timing_diagnostic(
        state.connection,
        state.project_id,
        state.editing_task_id,
        state.conversation_id,
        state.agent_task_id,
        Some(step_number as i64),
        AgentTimingMetric::SkillExecution,
        skill_started_at.elapsed(),
    );
    match skill_result {
        Ok(context) => {
            if OBSERVATION_TOOLS.contains(&tool.as_str()) {
                state.successful_observation = true;
            }
            // 检测 storyboard 确认需求
            let needs_confirmation =
                context.get("status").and_then(Value::as_str) == Some("needs_confirmation");
            if needs_confirmation && tool == "generate_storyboard" {
                let artifact = persisted_artifact_for_tool(state, &tool);
                finish_agent_run_step(
                    state.connection,
                    state.project_id,
                    state.editing_task_id,
                    state.agent_task_id,
                    &persisted_step_id,
                    "completed",
                    artifact.as_ref().map(|(kind, _)| *kind),
                    artifact.as_ref().map(|(_, id)| id.as_str()),
                    None,
                )?;
                state.executed_steps.push(ExecutedStepSummary {
                    step_number,
                    produced_artifact: produced_artifact_for_tool(&tool).map(str::to_owned),
                    tool: tool.clone(),
                    status: "succeeded".to_owned(),
                });
                return Ok(AgentLoopControl::NeedsClarification);
            }
            let artifact = persisted_artifact_for_tool(state, &tool);
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &persisted_step_id,
                "completed",
                artifact.as_ref().map(|(kind, _)| *kind),
                artifact.as_ref().map(|(_, id)| id.as_str()),
                None,
            )?;
            state.executed_steps.push(ExecutedStepSummary {
                step_number,
                produced_artifact: produced_artifact_for_tool(&tool).map(str::to_owned),
                tool: tool.clone(),
                status: "succeeded".to_owned(),
            });
            transcript.push(json!({
                "role": "tool",
                "tool": tool,
                "content": context.to_string()
            }));
            if transcript.len() > 8 {
                let split = transcript.len().saturating_sub(6);
                transcript.drain(1..split);
            }
            Ok(AgentLoopControl::Continue)
        }
        Err(error) => {
            let diagnostic = safe_tool_failure_context(&tool, &error);
            log::warn!(
                "Agent skill `{tool}` failed with safe code: {}",
                safe_step_error_code(&error)
            );
            record_agent_diagnostic(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                Some(step_number as i64),
                "tool_error",
                safe_step_error_code(&error),
            )?;
            let error_code = safe_step_error_code(&error);
            finish_agent_run_step(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.agent_task_id,
                &persisted_step_id,
                "failed",
                None,
                None,
                Some(error_code),
            )?;
            state.executed_steps.push(ExecutedStepSummary {
                step_number,
                tool: tool.clone(),
                status: "failed".to_owned(),
                produced_artifact: None,
            });
            state.last_failed_tool_error_code = Some(error_code);
            transcript.push(json!({
                "role": "tool",
                "content": diagnostic
            }));
            Ok(AgentLoopControl::Continue)
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// 结果组装
// ──────────────────────────────────────────────────────────────────────────────

pub(super) fn finalize_result(
    agent_task_id: &str,
    last: Option<AgentEditResult>,
    fallback: &str,
) -> AgentEditResult {
    match last {
        Some(result) => result,
        None => AgentEditResult {
            agent_task_id: agent_task_id.to_owned(),
            message: fallback.to_owned(),
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        },
    }
}

fn result_has_artifact(last: &Option<AgentEditResult>) -> bool {
    last.as_ref().is_some_and(|result| {
        result.storyboard.is_some()
            || result.timeline.is_some()
            || result.preview.is_some()
            || result.jianying_draft.is_some()
    })
}

fn finalize_result_helper(
    agent_task_id: &str,
    last: Option<AgentEditResult>,
    fallback: &str,
) -> AgentEditResult {
    match last {
        Some(mut result) => {
            result.message = fallback.to_owned();
            result
        }
        None => finalize_result(agent_task_id, None, fallback),
    }
}

/// Assembles the terminal result message. When a real artifact exists the
/// already-grounded outcome from the skill is kept; when nothing was produced
/// the reply is either the conversational answer (Question goal) or a fixed
/// honest fallback (a deliverable goal that was never satisfied).
pub(super) fn finalize_terminal(
    agent_task_id: &str,
    goal: LoopGoal,
    previous: Option<AgentEditResult>,
    model_reply: &str,
) -> AgentEditResult {
    match previous {
        Some(result) => result,
        None => {
            let reply = model_reply.trim();
            let message = match goal {
                LoopGoal::Question if !reply.is_empty() => reply.to_owned(),
                _ => honest_no_change(goal),
            };
            AgentEditResult {
                agent_task_id: agent_task_id.to_owned(),
                message,
                storyboard: None,
                timeline: None,
                preview: None,
                jianying_draft: None,
            }
        }
    }
}

fn remaining_model_timeout(deadline: Instant, now: Instant) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(now)?;
    (!remaining.is_zero()).then_some(AGENT_STEP_TIMEOUT.min(remaining))
}
