//! Agent 请求的路由、状态快照、有界模型循环与技能执行器。
//!
//! 纯请求策略和真实产物完成门位于 `agentloop/policy.rs`；本文件负责把策略应用到
//! 一次运行，并且只有具名技能执行路径可以触发数据库、文件或外部程序副作用。

use crate::audit::{begin_agent_run_step, finish_agent_run_step};
use crate::audit::{record_agent_diagnostic, record_agent_timing_diagnostic, AgentTimingMetric};
use crate::db::now_millis;
use crate::jianying::create_jianying_draft;
use crate::models::{
    AgentEditResult, ClipAdjustmentParams, ClipReplacementParams, MusicCue, MusicTrack,
    PendingClarificationSnapshot, StoryboardVersion, TextTrack, TimelineVersion,
};
use crate::music_provider::{attribution_for, download_track, eligible_track, search_tracks};
use crate::preview::render_preview;
use crate::provider::{model_response_json_text, post_model_payload, ModelAccess};
use crate::storyboard::generate_storyboard_for_agent;
use crate::timeline::{
    change_clip_duration, create_timeline_draft, reorder_clips, replace_clips,
    replace_music_tracks, replace_text_tracks, select_timeline_candidate, text_recipe_capabilities,
    text_track_quality_warnings, ClipAdjustment, ClipReplacement,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

mod policy;

use policy::{
    corrective_message, fast_goal, honest_no_change, model_unavailable_message,
    parse_declared_goal, pinned_goal_allows_response, request_requires_project_observation,
    run_deadline_message, LoopGoal, RequestToolPolicy, EDIT_TOOLS, OBSERVATION_TOOLS,
};

/// Maximum number of skill steps the loop will run before stopping. The loop is
/// goal-driven and may compose several edits, but it is always bounded so a
/// failing skill or a model that cannot satisfy the requested deliverable can
/// never loop forever.
const MAX_STEPS: usize = 10;

/// Timeout for a single agent-loop step model decision. A slow or hung provider
/// must never leave a request running forever; when it fires, the step fails and
/// the loop surfaces a fixed honest degradation reply.
const AGENT_STEP_TIMEOUT: Duration = Duration::from_secs(120);
/// Cooperative budget for model decisions in one interactive Agent run. Domain
/// tools keep their own bounded timeouts and are never killed mid-side-effect.
const AGENT_RUN_TIMEOUT: Duration = Duration::from_secs(300);

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

pub(crate) struct InitialAgentSkill {
    pub(crate) goal: LoopGoal,
    pub(crate) tool: String,
    pub(crate) args: Value,
    pub(crate) project_fact_question: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationRouteResponse {
    route: String,
    goal: Option<String>,
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
         Return one JSON object. route must be respond, clarify, or run.\n\
         For goal=question, include informationScope=general or project. Use general only when the answer does not depend on this project's current assets, tasks, artifacts, counts, state, or failure causes. A project-scoped question must use route=run and observe real state before answering.\n\
         - respond: only for general conversational answers that need no tool or side effect. Include goal=question, isQuestion=true, informationScope=general, answer.\n\
         - clarify: only when a genuinely required input is missing. Include question.\n\
         - run: for observation requiring project details, media analysis, storyboard/timeline edits, preview, or Jianying delivery. Include goal, isQuestion=false unless this is an observation question, and choose the FIRST tool now. Tool arguments stay at the JSON top level.\n\
         When pendingClarification is not null, respond and run must include clarificationAction=keep or resolve. Resolve only when this turn answers or explicitly abandons that question; keep it for unrelated turns. A new clarify route replaces the old question.\n\
         A long narration/script supplied after the Agent requested a creative goal is normally a creative input, even when its heading is a rhetorical question. Exact completion facts come only from latestRun/artifacts. The backend-pinned goal, when not null, is authoritative.\n\n\
         Available first tools: {tools}. Return JSON only.",
        latest_run = latest_run.unwrap_or(Value::Null),
        artifacts = artifacts,
        pending_clarification = serde_json::to_value(&pending_clarification).unwrap_or(Value::Null),
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
            let prev = serde_json::to_string(&raw).unwrap_or_else(|_| "{}".to_owned());
            let cp = format!("{prompt}\n\nYour previous response: {prev}\n\nIssue: {hint} Return corrected JSON only.");
            let cb = json!({"model":"gpt-5.4","store":false,"stream":true,"input":[{"role":"user","content":[{"type":"input_text","text":cp}]}],"text":{"format":{"type":"json_object"}}});
            let rb = post_model_payload(access, &cb, Some(timeout))?;
            let rt = model_response_json_text(access, &rb)
                .ok_or_else(|| "Route correction had no JSON.".to_owned())?;
            raw = serde_json::from_str(&rt)
                .map_err(|_| "Route correction was malformed.".to_owned())?;
            response = serde_json::from_value(raw.clone())
                .map_err(|_| "Route correction schema invalid.".to_owned())?;
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

fn clarification_resolution(
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
                .ok_or_else(|| "route=run: goal must be question/storyboard/timeline/preview/jianying.".to_owned())?;
            if tool_policy.forbids_goal(goal) {
                return Err(format!("goal='{}' is user-denied.", goal.code()));
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
            Ok(ConversationRouteDecision::Run { goal, tool, args: step_args(raw), project_fact_question: pfq, resolved_clarification_id: resolved })
        }
        r => Err(format!("route='{r}' unknown. Must be respond, clarify, or run.")),
    }
}

fn question_scope_allows_route(scope: Option<&str>, route: &str) -> bool {
    matches!(
        (scope, route),
        (Some("general"), "respond" | "run") | (Some("project"), "run")
    )
}

/// A single decision the model returns between steps. Every argument the loop
/// needs is placed at the top level of the JSON object, mirroring the explicit
/// schema so the model is not forced into a nested argument wrapper.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentStep {
    goal: Option<String>,
    is_question: Option<bool>,
    tool: Option<String>,
    #[allow(dead_code)]
    reason: Option<String>,
    #[allow(dead_code)]
    answer: Option<String>,
    #[allow(dead_code)]
    question: Option<String>,
    #[allow(dead_code)]
    task_brief: Option<String>,
}

/// A compact, authoritative view of the current Agent scope. This structure is
/// the only state snapshot embedded in a model step prompt. It deliberately
/// excludes local paths, message/model payloads, and media-evidence text.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentStateSnapshot {
    scope: AgentScopeSnapshot,
    assets: AssetAvailabilitySnapshot,
    artifacts: ArtifactPresenceSnapshot,
    executed_steps: Vec<ExecutedStepSummary>,
    remaining_steps: usize,
    goal: String,
    pending_clarification: Option<PendingClarificationSnapshot>,
    unmet_conditions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct AgentScopeSnapshot {
    project_id: String,
    editing_task_id: String,
    conversation_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct AssetAvailabilitySnapshot {
    total_count: usize,
    usable_count: usize,
    pending_analysis_count: usize,
    failed_analysis_count: usize,
    unavailable_source_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ArtifactPresenceSnapshot {
    storyboard: VersionArtifactSnapshot,
    timeline: VersionArtifactSnapshot,
    preview: TimelineArtifactSnapshot,
    jianying_draft: JianyingArtifactSnapshot,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct VersionArtifactSnapshot {
    exists: bool,
    version_id: Option<String>,
    version_number: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TimelineArtifactSnapshot {
    exists: bool,
    timeline_version_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct JianyingArtifactSnapshot {
    exists: bool,
    timeline_version_id: Option<String>,
    registration_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ExecutedStepSummary {
    step_number: usize,
    tool: String,
    status: String,
    produced_artifact: Option<String>,
}

struct LoopState<'a> {
    app: &'a AppHandle,
    connection: &'a Connection,
    agent_task_id: &'a str,
    project_id: &'a str,
    editing_task_id: &'a str,
    conversation_id: &'a str,
    task_brief: String,
    goal: LoopGoal,
    goal_locked: bool,
    tool_policy: RequestToolPolicy,
    pending_clarification: Option<PendingClarificationSnapshot>,
    run_started_at: Instant,
    run_deadline: Instant,
    history: Vec<(String, String)>,
    storyboard: Option<StoryboardVersion>,
    timelines: Vec<TimelineVersion>,
    last_outcome: Option<AgentEditResult>,
    executed_steps: Vec<ExecutedStepSummary>,
    last_failed_tool_error_code: Option<&'static str>,
    project_fact_question: bool,
    successful_observation: bool,
}

impl LoopState<'_> {
    fn agent_task_id(&self) -> &str {
        self.agent_task_id
    }
}

/// Runs a bounded, goal-driven editing loop. The loop derives a deliverable
/// goal from the request, lets the model pick exactly one skill per step, and
/// only allows a terminal `finish`/`no_action` when that goal's artifact
/// actually exists. Final messaging is assembled from the real executed
/// artifacts; a premature finish gets a corrective message instead of being
/// trusted verbatim.
pub(crate) struct AgentLoopResult {
    pub(crate) result: AgentEditResult,
    pub(crate) status: AgentLoopTerminalStatus,
    pub(crate) goal: LoopGoal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentLoopTerminalStatus {
    Completed,
    PartiallyCompleted,
    Failed,
    NeedsClarification,
}

impl AgentLoopTerminalStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::PartiallyCompleted => "partially_completed",
            Self::Failed => "failed",
            Self::NeedsClarification => "needs_clarification",
        }
    }
}

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
                    state.last_outcome = Some(finalize_result_helper(
                        agent_task_id,
                        state.last_outcome.take(),
                        &honest_no_change(state.goal),
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
                // A step only surfaces Err when the provider/model call itself
                // failed (for example it timed out). There is no point asking
                // the model again, so degrade to an honest, goal-appropriate
                // reply instead of bubbling a raw error to the client.
                log::warn!("AI agent-loop step aborted by a model error: {error}");
                terminated = true;
                failed = state.goal == LoopGoal::Question
                    || !state.goal.satisfied_by(&state.last_outcome);
                if failed {
                    state.last_outcome = Some(finalize_result_helper(
                        agent_task_id,
                        state.last_outcome.take(),
                        &model_unavailable_message(state.goal),
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
        state.last_outcome = Some(finalize_result_helper(
            agent_task_id,
            state.last_outcome.take(),
            &honest_no_change(state.goal),
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

fn first_model_step(executed_steps: &[ExecutedStepSummary]) -> usize {
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

enum AgentLoopControl {
    Continue,
    Done,
    PartiallyDone,
    Failed,
    ExplainedFailure,
    NeedsClarification,
    DeadlineExceeded,
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
    let prompt = build_step_prompt(state, transcript, &snapshot, &prerequisite_hints);
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

    let args = step_args(&raw);
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
                "技能“{tool}”不可用。可用的观察技能：{}；编辑/交付技能：{}。请重新选择。",
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

fn finalize_result(
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
fn finalize_terminal(
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

fn step_args(raw: &Value) -> Value {
    let mut args = raw.clone();
    if let Some(object) = args.as_object_mut() {
        for key in [
            "goal",
            "isQuestion",
            "tool",
            "reason",
            "answer",
            "question",
            "taskBrief",
            "clarificationAction",
            "informationScope",
        ] {
            object.remove(key);
        }
    }
    args
}

fn remaining_model_timeout(deadline: Instant, now: Instant) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(now)?;
    (!remaining.is_zero()).then_some(AGENT_STEP_TIMEOUT.min(remaining))
}

/// Maximum number of recent messages loaded for conversation memory.
const MAX_HISTORY_MESSAGES: usize = 12;
/// Character budget for the conversation history fed to the model.
const MAX_HISTORY_CHARS: usize = 8000;

/// Loads the most recent conversation messages for the given conversation,
/// excluding the message that is exactly the current request (the loop
/// transcript already carries that turn). Returns chronological (role, content)
/// pairs, capped by message count and total character budget.
fn load_message_history(
    connection: &Connection,
    conversation_id: &str,
    exclude_request: &str,
) -> Vec<(String, String)> {
    let mut statement = match connection.prepare(
        "SELECT role, content FROM messages WHERE conversation_id = ?1 ORDER BY created_at DESC LIMIT ?2",
    ) {
        Ok(statement) => statement,
        Err(_) => return Vec::new(),
    };
    let rows = match statement.query_map(
        params![conversation_id, MAX_HISTORY_MESSAGES as i64 + 1],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut newest_first: Vec<(String, String)> = rows.filter_map(Result::ok).collect();
    if let Some((role, content)) = newest_first.first() {
        if role == "user" && content.trim() == exclude_request.trim() {
            newest_first.remove(0);
        }
    }
    let mut kept: Vec<(String, String)> = Vec::new();
    let mut total_chars = 0;
    for (role, content) in newest_first {
        let chars = content.chars().count();
        if total_chars + chars > MAX_HISTORY_CHARS {
            continue;
        }
        total_chars += chars;
        kept.push((role, content));
    }
    kept.reverse();
    kept
}

/// Renders conversation history as a compact labelled text block for the model.
fn render_history(history: &[(String, String)]) -> String {
    if history.is_empty() {
        return "（无）".to_owned();
    }
    let lines: Vec<String> = history
        .iter()
        .map(|(role, content)| {
            let speaker = match role.as_str() {
                "user" => "用户",
                "agent" => "Agent",
                _ => "系统",
            };
            format!("{speaker}: {content}")
        })
        .collect();
    lines.join("\n")
}

pub(crate) fn load_pending_clarification(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
) -> Result<Option<PendingClarificationSnapshot>, String> {
    connection
        .query_row(
            "SELECT id, source_kind, source_agent_task_id, goal, question, created_at FROM pending_clarifications WHERE project_id = ?1 AND editing_task_id = ?2 AND conversation_id = ?3 AND status = 'pending' ORDER BY updated_at DESC LIMIT 1",
            params![project_id, editing_task_id, conversation_id],
            |row| Ok(PendingClarificationSnapshot {
                id: row.get(0)?,
                source_kind: row.get(1)?,
                source_agent_task_id: row.get(2)?,
                goal: row.get(3)?,
                question: row.get(4)?,
                created_at: row.get(5)?,
            }),
        )
        .optional()
        .map_err(|_| "Pending clarification could not be read.".to_owned())
}

fn load_asset_availability(
    connection: &Connection,
    project_id: &str,
) -> Result<AssetAvailabilitySnapshot, String> {
    let mut statement = connection
        .prepare("SELECT analysis_status, source_reference FROM assets WHERE project_id = ?1")
        .map_err(|_| "Asset availability unreadable.".to_owned())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| "Asset availability unreadable.".to_owned())?;
    let mut snapshot = AssetAvailabilitySnapshot {
        total_count: 0,
        usable_count: 0,
        pending_analysis_count: 0,
        failed_analysis_count: 0,
        unavailable_source_count: 0,
    };
    for row in rows {
        let (analysis_status, source_reference) =
            row.map_err(|_| "Asset availability unreadable.".to_owned())?;
        let source_available = Path::new(&source_reference).is_file();
        snapshot.total_count += 1;
        if !source_available {
            snapshot.unavailable_source_count += 1;
        }
        match analysis_status.as_str() {
            "ready" if source_available => snapshot.usable_count += 1,
            "queued" | "analyzing" => snapshot.pending_analysis_count += 1,
            "ready" => {}
            _ => snapshot.failed_analysis_count += 1,
        }
    }
    Ok(snapshot)
}

fn current_artifact_presence(state: &LoopState) -> Result<ArtifactPresenceSnapshot, String> {
    let storyboard = if let Some(storyboard) = &state.storyboard {
        let exists = state
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM storyboard_versions WHERE id = ?1 AND project_id = ?2 AND editing_task_id = ?3)",
                params![storyboard.id, state.project_id, state.editing_task_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|_| "Storyboard state unreadable.".to_owned())?;
        VersionArtifactSnapshot {
            exists,
            version_id: exists.then(|| storyboard.id.clone()),
            version_number: exists.then_some(storyboard.version_number),
        }
    } else {
        VersionArtifactSnapshot {
            exists: false,
            version_id: None,
            version_number: None,
        }
    };

    let candidate = state
        .timelines
        .iter()
        .max_by_key(|timeline| timeline.version_number);
    let current_timeline = if let Some(timeline) = candidate {
        let exists = state
            .connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM timeline_versions timeline
                    JOIN storyboard_versions storyboard ON storyboard.id = timeline.storyboard_version_id
                    WHERE timeline.id = ?1 AND timeline.project_id = ?2 AND storyboard.editing_task_id = ?3
                )",
                params![timeline.id, state.project_id, state.editing_task_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|_| "Timeline state unreadable.".to_owned())?;
        exists.then_some(timeline)
    } else {
        None
    };
    let timeline = VersionArtifactSnapshot {
        exists: current_timeline.is_some(),
        version_id: current_timeline.map(|value| value.id.clone()),
        version_number: current_timeline.map(|value| value.version_number),
    };
    let preview = current_timeline
        .map(|value| preview_presence(state, &value.id))
        .transpose()?
        .unwrap_or(TimelineArtifactSnapshot {
            exists: false,
            timeline_version_id: None,
        });
    let jianying_draft = current_timeline
        .map(|value| jianying_presence(state.connection, state.project_id, &value.id))
        .transpose()?
        .unwrap_or(JianyingArtifactSnapshot {
            exists: false,
            timeline_version_id: None,
            registration_status: None,
        });
    Ok(ArtifactPresenceSnapshot {
        storyboard,
        timeline,
        preview,
        jianying_draft,
    })
}

fn preview_presence(
    state: &LoopState,
    timeline_version_id: &str,
) -> Result<TimelineArtifactSnapshot, String> {
    let marked_ready = state
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM timeline_versions WHERE id = ?1 AND project_id = ?2 AND status = 'preview_ready')",
            params![timeline_version_id, state.project_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| "Agent preview state could not be read.".to_owned())?;
    let preview_exists = marked_ready
        && state
            .app
            .path()
            .app_data_dir()
            .ok()
            .map(|directory| {
                directory
                    .join("previews")
                    .join(timeline_version_id)
                    .join("preview.mp4")
                    .is_file()
            })
            .unwrap_or(false);
    Ok(TimelineArtifactSnapshot {
        exists: preview_exists,
        timeline_version_id: preview_exists.then(|| timeline_version_id.to_owned()),
    })
}

fn jianying_presence(
    connection: &Connection,
    project_id: &str,
    timeline_version_id: &str,
) -> Result<JianyingArtifactSnapshot, String> {
    let mut statement = connection
        .prepare(
            "SELECT status, input_json, result_json FROM agent_tasks
             WHERE project_id = ?1 AND tool_name = 'register_jianying_draft'
             ORDER BY created_at DESC",
        )
        .map_err(|_| "Jianying draft state unreadable.".to_owned())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|_| "Jianying draft state unreadable.".to_owned())?;
    for row in rows {
        let (task_status, input_json, result_json) =
            row.map_err(|_| "Jianying draft state unreadable.".to_owned())?;
        let input: Value = match serde_json::from_str(&input_json) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if input.get("timelineVersionId").and_then(Value::as_str) != Some(timeline_version_id) {
            continue;
        }
        let result = result_json.as_deref().and_then(|value| {
            serde_json::from_str::<crate::models::JianyingDraftResult>(value).ok()
        });
        let exists = result.as_ref().is_some_and(|draft| {
            Path::new(&draft.draft_directory).is_dir()
                && Path::new(&draft.draft_content_path).is_file()
        });
        let registration_status = result.map(|draft| draft.registration_status).or_else(|| {
            Some(
                match task_status.as_str() {
                    "completed" => "registered",
                    "failed" | "cancelled" => "failed",
                    _ => "pending",
                }
                .to_owned(),
            )
        });
        return Ok(JianyingArtifactSnapshot {
            exists,
            timeline_version_id: exists.then(|| timeline_version_id.to_owned()),
            registration_status,
        });
    }
    Ok(JianyingArtifactSnapshot {
        exists: false,
        timeline_version_id: None,
        registration_status: None,
    })
}

fn unmet_conditions(
    goal: LoopGoal,
    assets: &AssetAvailabilitySnapshot,
    artifacts: &ArtifactPresenceSnapshot,
    task_brief_is_empty: bool,
    goal_satisfied: bool,
) -> Vec<String> {
    if goal == LoopGoal::Question {
        return Vec::new();
    }
    let mut unmet = Vec::new();
    if !goal_satisfied {
        unmet.push(format!("requested_{}_not_produced", goal.code()));
    }
    let needs_storyboard_creation = matches!(goal, LoopGoal::Storyboard)
        || (!artifacts.timeline.exists
            && matches!(
                goal,
                LoopGoal::Timeline | LoopGoal::Preview | LoopGoal::JianyingDraft
            )
            && !artifacts.storyboard.exists);
    if needs_storyboard_creation {
        if task_brief_is_empty {
            unmet.push("task_brief_missing".to_owned());
        }
        if assets.total_count == 0 {
            unmet.push("no_assets".to_owned());
        } else if assets.usable_count == 0 {
            if assets.pending_analysis_count > 0 {
                unmet.push("asset_analysis_incomplete".to_owned());
            } else {
                unmet.push("no_usable_analyzed_assets".to_owned());
            }
        }
    }
    if matches!(
        goal,
        LoopGoal::Timeline | LoopGoal::Preview | LoopGoal::JianyingDraft
    ) && !artifacts.timeline.exists
        && !artifacts.storyboard.exists
    {
        unmet.push("storyboard_missing_for_timeline_creation".to_owned());
    }
    if goal == LoopGoal::Preview && !artifacts.timeline.exists {
        unmet.push("timeline_missing_for_preview".to_owned());
    }
    if goal == LoopGoal::JianyingDraft && !artifacts.timeline.exists {
        unmet.push("timeline_missing_for_jianying_draft".to_owned());
    }
    unmet
}

/// Builds the compact per-step state snapshot. The snapshot is rebuilt before
/// every model decision, so it reflects artifacts created by earlier skills in
/// the same run and never relies on a stale conversation claim.
fn build_agent_state_snapshot(
    state: &LoopState,
    remaining_steps: usize,
) -> Result<AgentStateSnapshot, String> {
    let assets = load_asset_availability(state.connection, state.project_id)?;
    let artifacts = current_artifact_presence(state)?;
    let unmet_conditions = if state.goal_locked {
        unmet_conditions(
            state.goal,
            &assets,
            &artifacts,
            state.task_brief.trim().is_empty(),
            state.goal.satisfied_by(&state.last_outcome),
        )
    } else {
        Vec::new()
    };
    Ok(AgentStateSnapshot {
        scope: AgentScopeSnapshot {
            project_id: state.project_id.to_owned(),
            editing_task_id: state.editing_task_id.to_owned(),
            conversation_id: state.conversation_id.to_owned(),
        },
        assets,
        artifacts,
        executed_steps: state.executed_steps.clone(),
        remaining_steps,
        goal: if state.goal_locked {
            state.goal.code().to_owned()
        } else {
            "pending".to_owned()
        },
        pending_clarification: state.pending_clarification.clone(),
        unmet_conditions,
    })
}

/// Produces deterministic guidance from the snapshot without executing a skill.
/// It identifies the shortest currently valid dependency, while explicitly
/// preserving direct edits/renders when a timeline already exists.
fn deterministic_prerequisite_hints(snapshot: &AgentStateSnapshot) -> Vec<String> {
    let mut hints = Vec::new();
    if snapshot
        .unmet_conditions
        .iter()
        .any(|value| value == "no_assets")
    {
        hints.push("当前没有素材；需要创作产物时应使用 ask_user 请用户先导入素材。".to_owned());
        return hints;
    }
    if snapshot
        .unmet_conditions
        .iter()
        .any(|value| value == "asset_analysis_incomplete")
    {
        hints.push(
            "素材分析尚未完成；不要从文件名猜测内容。可观察素材状态、请求可用素材的本地分析，或在确实缺少输入时澄清。"
                .to_owned(),
        );
        return hints;
    }
    if snapshot
        .unmet_conditions
        .iter()
        .any(|value| value == "no_usable_analyzed_assets")
    {
        hints.push(
            "没有可用且已分析的素材；可观察当前素材状态，并根据用户意图选择请求分析或澄清。"
                .to_owned(),
        );
        return hints;
    }
    match snapshot.goal.as_str() {
        "pending" => hints.push(
            "本轮目标尚未锁定；首次响应必须同时声明 goal/isQuestion 并选择一个实际技能或 finish。"
                .to_owned(),
        ),
        "storyboard" => hints.push("素材已可用；可用技能包括 generate_storyboard 与观察技能。根据用户意图选择下一步。".to_owned()),
        "timeline" if snapshot.artifacts.timeline.exists => hints.push(
            "当前内部时间线已存在；可用时间线编辑技能和观察技能。根据用户意图选择，不要无理由重建 storyboard。"
                .to_owned(),
        ),
        "timeline" if snapshot.artifacts.storyboard.exists => {
            hints.push("当前已有 storyboard，但没有内部时间线；可用技能包括 create_timeline_draft 与观察技能。".to_owned())
        }
        "timeline" => hints.push(
            "当前没有可编辑时间线；现有素材、storyboard 与时间线状态如快照所示。选择满足用户意图的合法下一步。"
                .to_owned(),
        ),
        "preview" if snapshot.artifacts.timeline.exists => hints.push(
            "当前内部时间线已存在；可用技能包括 render_preview、时间线编辑和观察技能。"
                .to_owned(),
        ),
        "preview" if snapshot.artifacts.storyboard.exists => hints.push(
            "当前缺少时间线但已有 storyboard；render_preview 需要时间线，可用技能包括 create_timeline_draft 与观察技能。".to_owned(),
        ),
        "preview" => hints.push(
            "当前缺少 storyboard 和时间线；快照中的素材状态决定哪些创作工具当前可用。"
                .to_owned(),
        ),
        "jianying_draft" if snapshot.artifacts.timeline.exists => hints.push(
            "当前内部时间线已存在；可用技能包括 create_jianying_draft、时间线编辑和观察技能。"
                .to_owned(),
        ),
        "jianying_draft" if snapshot.artifacts.storyboard.exists => hints.push(
            "当前缺少时间线但已有 storyboard；create_jianying_draft 需要时间线，可用技能包括 create_timeline_draft 与观察技能。"
                .to_owned(),
        ),
        "jianying_draft" => hints.push(
            "当前缺少 storyboard 和时间线；快照中的素材状态决定哪些创作和交付工具当前可用。"
                .to_owned(),
        ),
        _ => hints.push("问答目标可先使用观察技能获取当前事实，再如实回答。".to_owned()),
    }
    hints
}

fn produced_artifact_for_tool(tool: &str) -> Option<&'static str> {
    match tool {
        "generate_storyboard" => Some("storyboard"),
        "create_timeline_draft"
        | "replace_clips"
        | "change_clip_duration"
        | "reorder_clips"
        | "replace_text_tracks"
        | "replace_music_tracks" => Some("timeline"),
        "render_preview" => Some("preview"),
        "create_jianying_draft" => Some("jianying_draft"),
        _ => None,
    }
}

fn persisted_artifact_for_tool(state: &LoopState, tool: &str) -> Option<(&'static str, String)> {
    let result = state.last_outcome.as_ref()?;
    match tool {
        "generate_storyboard" => result
            .storyboard
            .as_ref()
            .map(|artifact| ("storyboard_version", artifact.id.clone())),
        "create_timeline_draft"
        | "replace_clips"
        | "change_clip_duration"
        | "reorder_clips"
        | "replace_text_tracks"
        | "replace_music_tracks" => result
            .timeline
            .as_ref()
            .map(|artifact| ("timeline_version", artifact.id.clone())),
        "render_preview" => result
            .preview
            .as_ref()
            .map(|artifact| ("preview", artifact.timeline_version_id.clone())),
        "create_jianying_draft" => result
            .timeline
            .as_ref()
            .map(|artifact| ("jianying_draft", artifact.id.clone())),
        _ => None,
    }
}

fn safe_step_error_code(error: &str) -> &'static str {
    if error.starts_with("storyboard_source_inventory_unavailable:")
        || error.starts_with("storyboard_visual_evidence_unavailable:")
    {
        "unavailable_media"
    } else if error.contains("time range") || error.contains("source range") {
        "invalid_source_time_range"
    } else if error.contains("storyboard") || error.contains("timeline") {
        "missing_or_invalid_prerequisite"
    } else if error.contains("asset") || error.contains("media") {
        "unavailable_media"
    } else {
        "skill_execution_failed"
    }
}

fn diagnostic_count(error: &str, key: &str) -> Option<usize> {
    error
        .split([':', ';'])
        .map(str::trim)
        .find_map(|field| field.strip_prefix(&format!("{key}=")))
        .and_then(|value| value.parse().ok())
}

fn safe_tool_failure_context(tool: &str, error: &str) -> Value {
    let code = safe_step_error_code(error);
    if error.starts_with("storyboard_source_inventory_unavailable:") {
        let visual_ready = diagnostic_count(error, "visual_ready_candidates").unwrap_or(0);
        let accessible = diagnostic_count(error, "accessible_source_files").unwrap_or(0);
        return json!({
            "status": "failed",
            "operation": tool,
            "stage": "storyboard_source_validation",
            "code": code,
            "facts": [
                format!("{visual_ready} imported assets have completed visual evidence"),
                format!("{accessible} of those source files are currently accessible")
            ],
            "retryable": true,
            "recovery": "Reconnect the source storage or relink the imported media, then retry.",
            "responseInstruction": "Explain the failure naturally in the user's language. State that the requested artifact was not created. Do not invent a more specific filesystem cause and do not claim completion."
        });
    }
    if error.starts_with("storyboard_visual_evidence_unavailable:") {
        return json!({
            "status": "failed",
            "operation": tool,
            "stage": "storyboard_evidence_validation",
            "code": code,
            "facts": ["No imported asset currently has completed visual evidence usable for storyboard generation."],
            "retryable": true,
            "recovery": "Complete or retry visual analysis for relevant imported media, then retry storyboard generation.",
            "responseInstruction": "Explain the failure naturally in the user's language. State that the storyboard was not created. Do not claim source files are missing unless supplied facts say so, and do not claim completion."
        });
    }
    json!({
        "status": "failed",
        "operation": tool,
        "stage": "tool_execution",
        "code": code,
        "facts": ["The local tool rejected the operation before confirming the requested artifact."],
        "retryable": code != "invalid_source_time_range",
        "recovery": "Use the safe code and current task state to explain the failure or choose another allowed tool.",
        "responseInstruction": "Explain only the supplied facts in the user's language. Do not infer hidden details, expose local paths, or claim completion."
    })
}

fn safe_failure_explanation(explanation: &str) -> bool {
    let explanation = explanation.trim().to_lowercase();
    !explanation.is_empty()
        && ![
            "已生成",
            "已创建",
            "已完成",
            "生成成功",
            "创建成功",
            "successfully generated",
            "successfully created",
            "completed successfully",
        ]
        .iter()
        .any(|claim| explanation.contains(claim))
}

fn should_redirect_storyboard_after_failed_generation(
    goal: LoopGoal,
    last_failed_tool_error_code: Option<&str>,
) -> bool {
    goal == LoopGoal::Storyboard
        && matches!(last_failed_tool_error_code, Some("skill_execution_failed"))
}

fn project_fact_completion_instruction(
    project_fact_question: bool,
    successful_observation: bool,
) -> &'static str {
    if project_fact_question && successful_observation {
        "This is a project-fact question and at least one read-only observation has already succeeded. If the latest tool result contains the count, status, or fact the user asked for, choose finish now and answer from that result. Do not call a semantically overlapping observation tool merely to confirm the same fact. Call another observation only when a specifically requested fact is absent from the latest result."
    } else {
        "For a project-fact question, obtain one successful read-only observation before answering."
    }
}

fn build_step_prompt(
    state: &LoopState,
    transcript: &[Value],
    snapshot: &AgentStateSnapshot,
    prerequisite_hints: &[String],
) -> String {
    let snapshot_json = serde_json::to_string(snapshot).unwrap_or_else(|_| "{}".to_owned());
    let prerequisite_json =
        serde_json::to_string(prerequisite_hints).unwrap_or_else(|_| "[]".to_owned());
    let transcript_json = serde_json::to_string(transcript).unwrap_or_else(|_| "[]".to_owned());
    let history_text = render_history(&state.history);
    let goal_label = if state.goal_locked {
        state.goal.label()
    } else {
        "待模型结合本轮请求与历史声明"
    };
    let clarification_hint = state.pending_clarification.as_ref().map_or_else(
        || "There is no pending clarification marker.".to_owned(),
        |pending| format!(
            "There is an unanswered scoped clarification: {} Treat the current user message as a possible answer, use the full history, and do not repeat it blindly.",
            pending.question
        ),
    );
    let project_fact_instruction = project_fact_completion_instruction(
        state.project_fact_question,
        state.successful_observation,
    );
    let denied_tools = state.tool_policy.prompt_label();
    format!(
        "You are Assembly Agent, the local video-editing loop for a project. The requested deliverable \
         for THIS request is: {goal}. You must only call finish after you have REALLY produced that \
         deliverable; finishing without producing it will be rejected and the loop will continue. \
         never claim in an answer that you performed edits that you did not actually execute. If the \
         state snapshot says remainingSteps is 0, choose finish now: summarize only real artifacts and \
         any incomplete work. A truthful partial completion is preferable to another tool call.\n\n\
         Recent conversation history (before this request):\n{history_text}\n\n\
         {clarification_hint}\n\n\
         {project_fact_instruction}\n\n\
         User-denied side-effect tools for this request: {denied_tools}. These tools are unavailable even if they would normally be a useful follow-up. Do not declare a goal whose deliverable requires a denied tool.\n\n\
         If the requested deliverable above is pending, this first response must BOTH declare goal \
         (question|storyboard|timeline|preview|jianying_draft) and isQuestion, and choose the first \
         skill or finish in the same JSON object. A long narration/script supplied after the Agent asked \
         for a creative goal is normally an answer to that clarification, even when its title is phrased \
         as a rhetorical question. Once declared, the goal is fixed. If the backend already pins a goal, \
         that goal is authoritative and model output cannot replace it. For a question that does not need \
         project facts, answer with finish in this same step instead of calling an observation skill.\n\n\
         Pick exactly ONE skill for this step from the list below. Put every argument field at the TOP \
         LEVEL of the JSON object (no nested parameter wrapper) using the exact camelCase names; stray \
         keys are tolerated. Only refer to clips and durations that exist in the provided state, and \
         honour the user's intent. You may optionally include taskBrief only when the user gives or \
         materially changes a video-creation goal.\n\n\
          Skills:\n\
          - get_edit_status. no args. Read the latest previous scoped Agent task and report only grounded completion status.\n\
          - get_asset_health_summary. no args. Use for questions about this project's current source-file health, counts, scan state, or unreadable/missing causes. It returns persisted counts and safe reason codes, never paths or raw operating-system errors. Do not infer a specific cause when reasonEvidenceAvailable is false.\n\
           - list_assets. no args. Use only for a compact persisted status inventory or before requesting analysis. This Agent observation never starts or reprioritizes analysis.\n\
          - search_assets. args: query (optional), kind (video|image|audio|other optional), minDurationMs/maxDurationMs (optional), minRating 0..5 (optional), favoriteOnly (optional), tag (optional), collectionId (optional), offset (optional), limit 1..20 (optional). Use for targeted candidate discovery. It excludes user-blocked assets and returns bounded safe summaries, match reason codes and nextOffset; it never returns paths, notes, OCR text or media content.\n\
         - search_asset_segments. args: query (required), assetId (optional), offset (optional), limit 1..20 (optional). Use after candidate discovery when an edit needs exact source windows. It returns evidence-bound sourceStartMs/sourceEndMs, safe visual labels and reason codes; OCR text and paths remain private. Missing, changed, unreadable and user-blocked sources are excluded.\n\
         - search_music. args: query. Search the configured Jamendo catalog. It only returns tracks whose download is allowed and whose license is CC0 or CC-BY; CC-BY attribution is retained on the music cue. Never invent a track or URL.\n\
         - download_music. args: trackId. Download exactly one eligible Jamendo track to the current local project and queue its normal media analysis. Call search_music first.\n\
         - use_online_music. args: trackId, timelineVersionId (optional). Call search_music first. It downloads exactly one eligible track, waits for its local analysis, then creates a new timeline version with that track looped across the full timeline at safe background volume. Use this when the user asks you to choose and apply music, not merely recommend it. After it succeeds, use render_preview or create_jianying_draft if requested.\n\
          - request_asset_analysis. args: assetIds [string]; use only after list_assets identifies imported queued or failed assets. This queues local analysis and does not expose paths or run filesystem commands in the model.\n\
         - For a user request to analyze or reanalyze media, observe with list_assets first, then choose request_asset_analysis for eligible assets. Do not choose generate_storyboard unless the user asks to create a storyboard.\n\
         - get_storyboard. no args.\n\
         - get_timeline. args: timelineVersionId (optional).\n\
         - get_text_capabilities. no args. Call this before authoring or revising text when the user cares about fonts, effects, or Jianying delivery. It returns the verified Jianying matrix and local-preview-only options.\n\
         - replace_music_tracks. args: timelineVersionId (optional), musicTracks: [{{id, enabled, cues}}]. Each cue needs id, assetId, sourceStartMs, sourceEndMs, timelineStartMs, timelineEndMs, volume (0..2); loopEnabled, fadeInMs, fadeOutMs are optional. Call get_timeline and list_assets first. Music must use a ready audio asset and stay in the timeline. Set loopEnabled only when a shorter source range must repeat. create_jianying_draft can create a new experimental Jianying music draft from these local assets; never claim playback has been visually reviewed in Jianying.\n\
         - generate_storyboard. args: brief. It consumes only analysis evidence already ready in this project; it never starts or reprioritizes analysis. Request analysis explicitly first only when the user permits it.\n\
         - create_timeline_draft. no args; it uses the active storyboard in this editing task.\n\
         - replace_clips. args: timelineVersionId (optional), shots: [{{shotIndex int, assetId string, sourceStartMs int, sourceEndMs int}}]. A video source range must equal the replaced shot's current duration; images use 0 and 0.\n\
         - change_clip_duration. args: timelineVersionId (optional), adjustments: [{{shotIndex int, newDurationMs int optional, newSourceStartMs int optional}}]. The new source window must stay inside the shot's verified source.\n\
         - reorder_clips. args: timelineVersionId (optional), order: [ ints ] (a complete valid permutation of the shot indexes).\n\
         - replace_text_tracks. args: timelineVersionId (optional), textTracks: [{{id, role subtitle|headline|callout|cta|label, layer, enabled, cues}}]. Each cue needs id, startMs, endMs, text. style and layout are optional: omitted fields resolve to the safe default subtitle design. Before the first text-authoring call, call get_timeline and get_text_capabilities; call get_storyboard too when the intended on-screen meaning is not clear from the timeline. Prefer the capability selectionHint: use subtitle_safe for dialogue/narration, headline_rise for progression or an opening reveal, headline_pop for a surprise/key result/contrast, headline_drop for a conclusion/rule/warning, and callout_card/cta_card only when the user explicitly accepts a local-preview-only result. Use at most one headline per visual beat; do not overlap a headline with another headline, or use a headline as ordinary subtitles. The backend resolves a template to an auditable complete recipe and ignores conflicting style/animation values. Without a templateId, use only fade, slide_up, slide_down, pop, or wipe animations. Never send jianyingCompatibility: the backend assigns it. Jianying delivery requires fontKey jianying_default, no stroke/shadow/background/loop, an optional fade exit, and only static/fade/slide_up/slide_down/pop entrance. Unicode text is written through the verified escaped nested-text path.\n\
         - render_preview. args: timelineVersionId (optional).\n\
         - create_jianying_draft. args: timelineVersionId (optional).\n\
         - ask_user. args: question (only if a needed value is genuinely missing).\n\
         - finish. args: answer (concise Chinese summary of what was done).\n\n\
         The authoritative state snapshot contains only scoped availability and artifact facts. If \
         you need storyboard shots or timeline clips, call get_storyboard or get_timeline first. \
         Deterministic prerequisite hints identify the shortest currently valid path; they do not \
         require rebuilding a storyboard when an existing timeline can be edited or rendered.\n\n\
         Return JSON with ONLY the keys: goal and isQuestion when required above; tool (exactly one of \
         the names above, or no_action if nothing should be done); reason; answer/question/taskBrief when \
         relevant; and the argument fields that tool needs.\n\n\
         Agent state snapshot: {snapshot_json}\n\
         Deterministic prerequisite hints: {prerequisite_json}\n\
         Conversation so far: {transcript_json}",
        goal = goal_label,
        history_text = history_text,
        clarification_hint = clarification_hint,
        project_fact_instruction = project_fact_instruction,
        denied_tools = denied_tools,
    )
}

/// 在真实且已校验的领域函数上执行一个技能。返回值只作为下一步观察；只有这里落地的
/// 可审计产物才会更新 `last_outcome`，模型文字本身永远不代表副作用成功。
fn apply_skill(state: &mut LoopState, tool: &str, args: &Value) -> Result<Value, String> {
    let agent_task_id = state.agent_task_id().to_owned();
    match tool {
        "get_edit_status" => {
            let message = read_scoped_edit_status(
                state.app,
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                Some(state.agent_task_id),
            )?;
            state.last_outcome = Some(AgentEditResult {
                agent_task_id: agent_task_id.clone(),
                message: message.clone(),
                storyboard: None,
                timeline: None,
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({"tool":"get_edit_status","status":"ok","message":message}))
        }
        "search_music" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "search_music needs a query.".to_owned())?;
            let tracks = search_tracks(query)?;
            Ok(json!({"tool":"search_music","status":"ok","tracks":tracks}))
        }
        "download_music" => {
            let track_id = args
                .get("trackId")
                .and_then(Value::as_str)
                .ok_or_else(|| "download_music needs a trackId.".to_owned())?;
            let asset = download_track(&state.app, state.project_id, track_id)?;
            Ok(
                json!({"tool":"download_music","status":"ok","assetId":asset.id,"analysisStatus":asset.analysis_status}),
            )
        }
        "use_online_music" => {
            let track_id = args
                .get("trackId")
                .and_then(Value::as_str)
                .ok_or_else(|| "use_online_music needs a trackId.".to_owned())?;
            let timeline = select_timeline_for_tool(state, args)?;
            let track = eligible_track(track_id)?;
            let attribution = attribution_for(&track);
            let asset = download_track(&state.app, state.project_id, track_id)?;
            let asset =
                crate::assets::wait_for_asset_ready(&state.app, state.project_id, &asset.id)?;
            let timeline_duration = timeline
                .clips
                .iter()
                .map(|clip| clip.timeline_end_ms)
                .max()
                .ok_or_else(|| "Timeline has no clips for music.".to_owned())?;
            let source_duration = asset
                .duration_ms
                .ok_or_else(|| "Music has no verified duration.".to_owned())?;
            let source_end = source_duration.min(timeline_duration);
            let asset_id = asset.id.clone();
            let result = replace_music_tracks(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                &agent_task_id,
                &timeline,
                vec![MusicTrack {
                    id: format!("jamendo-{track_id}"),
                    enabled: true,
                    cues: vec![MusicCue {
                        id: format!("jamendo-{track_id}-cue"),
                        asset_id: asset_id.clone(),
                        source_start_ms: 0,
                        source_end_ms: source_end,
                        timeline_start_ms: 0,
                        timeline_end_ms: timeline_duration,
                        loop_enabled: source_end < timeline_duration,
                        volume: 0.35,
                        fade_in_ms: 250,
                        fade_out_ms: 350,
                        jianying_compatibility: "not_deliverable".to_owned(),
                        provider: Some("Jamendo".to_owned()),
                        license_url: Some(track.license_ccurl),
                        attribution: Some(attribution),
                    }],
                }],
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message:
                    "已选择并下载一首符合许可条件的背景音乐，完成本地分析后写入新的内部时间线版本。"
                        .to_owned(),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(
                json!({"tool":"use_online_music","status":"ok","timelineVersionId":timeline_version_id,"versionNumber":version_number,"assetId":asset_id}),
            )
        }
        "list_assets" => {
            let assets = crate::assets::list_assets_for_agent(
                state.app.clone(),
                state.project_id.to_owned(),
            )?;
            let summary: Vec<Value> = assets
                .iter()
                .map(|asset| {
                    json!({
                        "id": asset.id,
                        "name": asset.display_name,
                        "kind": asset.kind,
                        "durationMs": asset.duration_ms,
                        "analysisStatus": asset.analysis_status,
                        "sceneCount": asset.scene_count,
                    })
                })
                .collect();
            Ok(json!({ "tool": "list_assets", "status": "ok", "assets": summary }))
        }
        "get_asset_health_summary" => {
            let summary = crate::assets::get_asset_health_summary_for_agent(
                &state.connection,
                state.project_id,
            )?;
            Ok(json!({ "tool": "get_asset_health_summary", "status": "ok", "summary": summary }))
        }
        "search_assets" => {
            let results = crate::assets::search_assets_for_agent(
                &state.connection,
                state.project_id,
                args.get("query").and_then(Value::as_str),
                args.get("kind").and_then(Value::as_str),
                args.get("minDurationMs").and_then(Value::as_i64),
                args.get("maxDurationMs").and_then(Value::as_i64),
                args.get("minRating").and_then(Value::as_i64),
                args.get("favoriteOnly")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                args.get("tag").and_then(Value::as_str),
                args.get("collectionId").and_then(Value::as_str),
                args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize,
                args.get("limit").and_then(Value::as_u64).unwrap_or(12) as usize,
            )?;
            Ok(json!({ "tool": "search_assets", "status": "ok", "results": results }))
        }
        "search_asset_segments" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "Segments query required.".to_owned())?;
            let results = crate::assets::search_asset_segments_for_agent(
                &state.connection,
                state.project_id,
                query,
                args.get("assetId").and_then(Value::as_str),
                args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize,
                args.get("limit").and_then(Value::as_u64).unwrap_or(12) as usize,
            )?;
            Ok(json!({"tool":"search_asset_segments","status":"ok","results":results}))
        }
        "request_asset_analysis" => {
            let asset_ids: Vec<String> = serde_json::from_value(
                args.get("assetIds")
                    .cloned()
                    .ok_or_else(|| "request_asset_analysis needs an assetIds array.".to_owned())?,
            )
            .map_err(|error| error.to_string())?;
            let queued =
                crate::assets::request_asset_analysis(state.app, state.project_id, &asset_ids)?;
            Ok(
                json!({ "tool": "request_asset_analysis", "status": "queued", "queuedCount": queued }),
            )
        }
        "get_storyboard" => Ok(json!({
            "tool": "get_storyboard",
            "status": "ok",
            "storyboard": state
                .storyboard
                .as_ref()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .unwrap_or(Value::Null)
        })),
        "get_timeline" => {
            let timeline_id = args.get("timelineVersionId").and_then(Value::as_str);
            Ok(json!({
                "tool": "get_timeline",
                "status": "ok",
                "timeline": build_timeline_snapshot(state, timeline_id)
            }))
        }
        "get_text_capabilities" => Ok(json!({
            "tool": "get_text_capabilities",
            "status": "ok",
            "fonts": [
                {"fontKey": "jianying_default", "preview": "supported", "jianying": "verified", "note": "Jianying default font; Unicode text is written through the verified escaped nested-text path."},
                {"fontKey": "sans_bold", "preview": "supported", "jianying": "local_preview_only"},
                {"fontKey": "sans_clean", "preview": "supported", "jianying": "local_preview_only"},
                {"fontKey": "serif_editorial", "preview": "supported", "jianying": "local_preview_only"},
                {"fontKey": "mono_tech", "preview": "supported", "jianying": "local_preview_only"},
                {"fontKey": "jianying_sans_bold", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying Source Han Sans bold resource; visual delivery validation is pending."},
                {"fontKey": "jianying_sans_regular", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying Source Han Sans regular resource; visual delivery validation is pending."},
                {"fontKey": "jianying_serif_bold", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying Source Han Serif bold resource; visual delivery validation is pending."},
                {"fontKey": "jianying_handwritten", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying WenKai bold resource; visual delivery validation is pending."},
                {"fontKey": "jianying_harmony_bold", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying HarmonyOS Sans bold resource; visual delivery validation is pending."}
            ],
            "templates": [
                {"templateId": "static", "preview": "supported", "jianying": "verified"},
                {"templateId": "fade", "phase": "entrance_or_exit", "preview": "supported", "jianying": "verified"},
                {"templateId": "slide_up", "phase": "entrance", "preview": "supported", "jianying": "verified"},
                {"templateId": "slide_down", "phase": "entrance", "preview": "supported", "jianying": "verified"},
                {"templateId": "pop", "phase": "entrance", "preview": "supported", "jianying": "verified"},
                {"templateId": "wipe", "preview": "supported", "jianying": "local_preview_only"}
            ],
            "textRecipes": text_recipe_capabilities(),
            "jianyingRestrictions": "Verified delivery requires jianying_default, no stroke, shadow, background, or loop animation; only fade may be an exit, and only fade/slide_up/slide_down/pop may be an entrance. Text content is serialized through the verified escaped nested-text path."
        })),
        "generate_storyboard" => {
            let brief = args
                .get("brief")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|brief| !brief.is_empty())
                .unwrap_or(state.task_brief.trim());
            if brief.is_empty() {
                return Err("The user has no video goal to base a storyboard on.".to_owned());
            }
            let generated = generate_storyboard_for_agent(
                state.app.clone(),
                state.project_id.to_owned(),
                state.editing_task_id.to_owned(),
                brief.to_owned(),
            )?;
            let storyboard_version_id = generated.id.clone();
            let version_number = generated.version_number;
            let summary = generated.summary.clone();
            state.storyboard = Some(generated.clone());
            state.timelines = Vec::new();
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!(
                    "已按你的目标生成 storyboard（版本 {version}）。{summary}",
                    version = version_number
                ),
                storyboard: Some(generated),
                timeline: None,
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "generate_storyboard",
                "status": "ok",
                "storyboardVersionId": storyboard_version_id,
                "versionNumber": version_number
            }))
        }
        "create_timeline_draft" => {
            let storyboard = state
                .storyboard
                .as_ref()
                .ok_or_else(|| "Create a storyboard before creating a timeline.".to_owned())?;
            let created = create_timeline_draft(
                state.app.clone(),
                state.project_id.to_owned(),
                storyboard.id.clone(),
            )?;
            let timeline_version_id = created.id.clone();
            let version_number = created.version_number;
            upsert(&mut state.timelines, created.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已创建内容时间线 v{}。", version_number),
                storyboard: None,
                timeline: Some(created),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "create_timeline_draft",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "versionNumber": version_number
            }))
        }
        "replace_clips" => {
            let existing = select_timeline_for_tool(state, args)?;
            let shots_json = args
                .get("shots")
                .or_else(|| args.get("replacements"))
                .ok_or_else(|| "replace_clips needs a shots array.".to_owned())?;
            let params: Vec<ClipReplacementParams> =
                serde_json::from_value(shots_json.clone()).map_err(|error| error.to_string())?;
            if params.is_empty() {
                return Err("Agent did not identify any replacement media.".to_owned());
            }
            let replacements: Vec<ClipReplacement> = params
                .into_iter()
                .map(|replacement| ClipReplacement {
                    shot_index: replacement.shot_index,
                    asset_id: replacement.asset_id,
                    source_start_ms: replacement.source_start_ms,
                    source_end_ms: replacement.source_end_ms,
                })
                .collect();
            let result = replace_clips(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                &existing,
                &replacements,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            let quality_warnings = text_track_quality_warnings(&result.text_tracks);
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已批量替换镜头并创建新的内部时间线 v{}。", version_number),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "replace_clips",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "versionNumber": version_number,
                "qualityWarnings": quality_warnings
            }))
        }
        "change_clip_duration" => {
            let existing = select_timeline_for_tool(state, args)?;
            let adjustments_json = args
                .get("adjustments")
                .ok_or_else(|| "change_clip_duration needs an adjustments array.".to_owned())?;
            let adjustments: Vec<ClipAdjustmentParams> =
                serde_json::from_value(adjustments_json.clone())
                    .map_err(|error| error.to_string())?;
            if adjustments.is_empty() {
                return Err("Agent did not identify any clips to retime.".to_owned());
            }
            let clip_adjustments: Vec<ClipAdjustment> = adjustments
                .into_iter()
                .map(|adjustment| ClipAdjustment {
                    shot_index: adjustment.shot_index,
                    new_duration_ms: adjustment.new_duration_ms,
                    new_source_start_ms: adjustment.new_source_start_ms,
                })
                .collect();
            let result = change_clip_duration(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                &existing,
                &clip_adjustments,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!(
                    "已按新的时长与起止点校准镜头并创建本地时间线 v{}。",
                    version_number
                ),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "change_clip_duration",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "versionNumber": version_number
            }))
        }
        "reorder_clips" => {
            let existing = select_timeline_for_tool(state, args)?;
            let order_json = args
                .get("order")
                .ok_or_else(|| "reorder_clips needs an order array.".to_owned())?;
            let order: Vec<i64> =
                serde_json::from_value(order_json.clone()).map_err(|error| error.to_string())?;
            let result = reorder_clips(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                &existing,
                &order,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已按新的顺序排列镜头并创建本地时间线 v{}。", version_number),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "reorder_clips",
                "status": "ok",
                "timelineVersionId": timeline_version_id
            }))
        }
        "replace_text_tracks" => {
            let existing = select_timeline_for_tool(state, args)?;
            let tracks_json = args
                .get("textTracks")
                .ok_or_else(|| "replace_text_tracks needs a textTracks array.".to_owned())?;
            let text_tracks: Vec<TextTrack> =
                serde_json::from_value(tracks_json.clone()).map_err(|error| error.to_string())?;
            let result = replace_text_tracks(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                &existing,
                text_tracks,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已更新文本轨并创建内部时间线 v{}。", version_number),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "replace_text_tracks",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "versionNumber": version_number
            }))
        }
        "replace_music_tracks" => {
            let existing = select_timeline_for_tool(state, args)?;
            let tracks_json = args
                .get("musicTracks")
                .ok_or_else(|| "replace_music_tracks needs a musicTracks array.".to_owned())?;
            let music_tracks: Vec<MusicTrack> =
                serde_json::from_value(tracks_json.clone()).map_err(|error| error.to_string())?;
            let result = replace_music_tracks(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                &agent_task_id,
                &existing,
                music_tracks,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: "已创建本地音乐轨时间线版本；可创建新的实验性 Jianying music draft，仍需在 Jianying 中复核播放效果。".to_owned(),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(
                json!({"tool":"replace_music_tracks","status":"ok","timelineVersionId":timeline_version_id,"versionNumber":version_number,"jianying":"experimental_review_required"}),
            )
        }
        "render_preview" => {
            let timeline = select_timeline_for_tool(state, args)?;
            let timeline_version_id = timeline.id.clone();
            let version_number = timeline.version_number;
            let timeline_for_render = timeline.clone();
            let preview = render_preview(state.app.clone(), timeline_for_render.id.clone())?;
            upsert(&mut state.timelines, timeline_for_render.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已按请求生成本地低清预览（时间线 v{}）。", version_number),
                storyboard: None,
                timeline: Some(timeline_for_render),
                preview: Some(preview),
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "render_preview",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "versionNumber": version_number
            }))
        }
        "create_jianying_draft" => {
            let timeline = select_timeline_for_tool(state, args)?;
            let timeline_version_id = timeline.id.clone();
            let draft_timeline = timeline.clone();
            let draft = create_jianying_draft(state.app.clone(), draft_timeline.id.clone())?;
            let draft_name = Path::new(&draft.draft_directory)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Assembly Video Agent");
            let message = if draft.registration_status == "pending" {
                format!("已生成剪映草稿“{draft_name}”，剪映正在运行，退出剪映后会自动完成注册。")
            } else {
                format!("已创建并注册剪映草稿“{draft_name}”，可在剪映本地草稿中查看。")
            };
            let registration_status = draft.registration_status.clone();
            upsert_timeline(&mut state.timelines, draft_timeline.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message,
                storyboard: None,
                timeline: Some(draft_timeline),
                preview: None,
                jianying_draft: Some(draft),
            });
            Ok(json!({
                "tool": "create_jianying_draft",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "registrationStatus": registration_status
            }))
        }
        other => Err(format!("Unknown skill: {other}")),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct EditArtifactState {
    has_storyboard: bool,
    has_timeline: bool,
    has_preview: bool,
}

fn artifact_status_message(artifacts: EditArtifactState) -> Option<&'static str> {
    if artifacts.has_preview {
        Some("已经生成可审阅的 local preview。")
    } else if artifacts.has_timeline {
        Some("已经生成内部时间线，但还没有生成 local preview。")
    } else if artifacts.has_storyboard {
        Some("已经生成 storyboard，但还没有生成内部时间线。")
    } else {
        None
    }
}

fn edit_status_message(previous_status: Option<&str>, artifacts: EditArtifactState) -> String {
    let artifact_message = artifact_status_message(artifacts);
    match previous_status {
        Some("queued" | "running") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务仍在处理中；{message}"),
            None => "还没剪好，上一条 Agent 任务仍在处理中。".to_owned(),
        },
        Some("needs_clarification") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务正在等待你补充信息；{message}"),
            None => "还没剪好，上一条 Agent 任务正在等待你补充信息。".to_owned(),
        },
        Some("needs_review") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务需要审阅后再继续；{message}"),
            None => "还没确认完成，上一条 Agent 任务需要审阅后再继续。".to_owned(),
        },
        Some("failed") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务没有完成；{message}"),
            None => "还没剪好，上一条 Agent 任务没有完成，也没有把失败当成成功。".to_owned(),
        },
        Some("partially_completed") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务只完成了一部分；{message}"),
            None => "还没完全剪好，上一条 Agent 任务只完成了一部分。".to_owned(),
        },
        Some("completed") | None => artifact_message
            .unwrap_or_else(|| {
                if previous_status.is_some() {
                    "上一条 Agent 请求已完成，但当前没有可检查的剪辑产物。"
                } else {
                    "当前会话还没有可检查的剪辑任务或产物。"
                }
            })
            .to_owned(),
        Some(_) => "当前剪辑状态暂时无法确认。".to_owned(),
    }
}

pub(crate) fn read_scoped_edit_status(
    app: &AppHandle,
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    excluded_agent_task_id: Option<&str>,
) -> Result<String, String> {
    let previous_status = connection
        .query_row(
            "SELECT status FROM agent_tasks WHERE project_id = ?1 AND editing_task_id = ?2 AND conversation_id = ?3 AND (?4 IS NULL OR id != ?4) AND tool_name NOT IN ('analyze_asset', 'analyze_asset_visual_batch', 'get_edit_status') ORDER BY created_at DESC LIMIT 1",
            params![project_id, editing_task_id, conversation_id, excluded_agent_task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| "Agent edit status could not be read.".to_owned())?;
    let storyboard_id = connection
        .query_row(
            "SELECT id FROM storyboard_versions WHERE project_id = ?1 AND editing_task_id = ?2 ORDER BY version_number DESC, created_at DESC LIMIT 1",
            params![project_id, editing_task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| "Agent storyboard status could not be read.".to_owned())?;
    let latest_timeline = storyboard_id
        .as_deref()
        .map(|storyboard_id| {
            connection
                .query_row(
                    "SELECT id, status FROM timeline_versions WHERE project_id = ?1 AND storyboard_version_id = ?2 ORDER BY version_number DESC, created_at DESC LIMIT 1",
                    params![project_id, storyboard_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
        })
        .transpose()
        .map_err(|_| "Agent timeline status could not be read.".to_owned())?
        .flatten();
    let has_preview = latest_timeline
        .as_ref()
        .is_some_and(|(timeline_id, status)| {
            status == "preview_ready"
                && app
                    .path()
                    .app_data_dir()
                    .ok()
                    .map(|directory| {
                        directory
                            .join("previews")
                            .join(timeline_id)
                            .join("preview.mp4")
                            .is_file()
                    })
                    .unwrap_or(false)
        });
    Ok(edit_status_message(
        previous_status.as_deref(),
        EditArtifactState {
            has_storyboard: storyboard_id.is_some(),
            has_timeline: latest_timeline.is_some(),
            has_preview,
        },
    ))
}

fn select_timeline_for_tool(state: &LoopState, args: &Value) -> Result<TimelineVersion, String> {
    let timeline_id = args.get("timelineVersionId").and_then(Value::as_str);
    select_timeline_candidate(&state.timelines, timeline_id, None).ok_or_else(|| {
        "Agent must select a timeline that belongs to the current storyboard.".to_owned()
    })
}

fn build_timeline_snapshot(state: &LoopState, requested: Option<&str>) -> Value {
    select_timeline_candidate(&state.timelines, requested, None)
        .map(|timeline| serde_json::to_value(timeline).unwrap_or(Value::Null))
        .unwrap_or(Value::Null)
}

fn upsert(timelines: &mut Vec<TimelineVersion>, updated: TimelineVersion) {
    if let Some(slot) = timelines
        .iter_mut()
        .find(|timeline| timeline.id == updated.id)
    {
        *slot = updated;
    } else {
        timelines.push(updated);
    }
}

fn upsert_timeline(timelines: &mut Vec<TimelineVersion>, updated: TimelineVersion) {
    upsert(timelines, updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentEditResult;
    use serde_json::json;

    #[test]
    fn storyboard_generation_failure_redirects_without_reasking_for_a_brief() {
        assert!(should_redirect_storyboard_after_failed_generation(
            LoopGoal::Storyboard,
            Some("skill_execution_failed")
        ));
        assert!(!should_redirect_storyboard_after_failed_generation(
            LoopGoal::Storyboard,
            Some("missing_or_invalid_prerequisite")
        ));
    }

    fn test_snapshot(goal: &str) -> AgentStateSnapshot {
        AgentStateSnapshot {
            scope: AgentScopeSnapshot {
                project_id: "project-1".to_owned(),
                editing_task_id: "task-1".to_owned(),
                conversation_id: "conversation-1".to_owned(),
            },
            assets: AssetAvailabilitySnapshot {
                total_count: 2,
                usable_count: 2,
                pending_analysis_count: 0,
                failed_analysis_count: 0,
                unavailable_source_count: 0,
            },
            artifacts: ArtifactPresenceSnapshot {
                storyboard: VersionArtifactSnapshot {
                    exists: false,
                    version_id: None,
                    version_number: None,
                },
                timeline: VersionArtifactSnapshot {
                    exists: false,
                    version_id: None,
                    version_number: None,
                },
                preview: TimelineArtifactSnapshot {
                    exists: false,
                    timeline_version_id: None,
                },
                jianying_draft: JianyingArtifactSnapshot {
                    exists: false,
                    timeline_version_id: None,
                    registration_status: None,
                },
            },
            executed_steps: vec![ExecutedStepSummary {
                step_number: 1,
                tool: "list_assets".to_owned(),
                status: "succeeded".to_owned(),
                produced_artifact: None,
            }],
            remaining_steps: 5,
            goal: goal.to_owned(),
            pending_clarification: None,
            unmet_conditions: Vec::new(),
        }
    }

    #[test]
    fn step_args_removes_meta_keys() {
        let raw = json!({
            "goal": "timeline",
            "isQuestion": false,
            "tool": "replace_clips",
            "reason": "swap",
            "taskBrief": "new goal",
            "clarificationAction": "resolve",
            "informationScope": "project",
            "shots": [{"shotIndex": 1, "assetId": "a", "sourceStartMs": 0, "sourceEndMs": 2000}]
        });
        let args = step_args(&raw);
        assert!(args.get("tool").is_none());
        assert!(args.get("goal").is_none());
        assert!(args.get("isQuestion").is_none());
        assert!(args.get("reason").is_none());
        assert!(args.get("taskBrief").is_none());
        assert!(args.get("clarificationAction").is_none());
        assert!(args.get("informationScope").is_none());
        assert!(args.get("shots").is_some());
    }

    #[test]
    fn step_args_survives_non_object_decisions() {
        let args = step_args(&json!(["not", "an", "object"]));
        assert!(args.is_array());
    }

    #[test]
    fn finalize_result_keeps_the_last_concrete_outcome() {
        let result = AgentEditResult {
            agent_task_id: "task-1".to_owned(),
            message: "done".to_owned(),
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        };
        let finalized = finalize_result("task-1", Some(result), "fallback");
        assert_eq!(finalized.message, "done");
        let empty = finalize_result("task-1", None, "fallback");
        assert_eq!(empty.message, "fallback");
        assert!(empty.storyboard.is_none());
    }

    #[test]
    fn fast_goal_pins_unambiguous_requests() {
        assert_eq!(fast_goal("请生成一个新的预览"), Some(LoopGoal::Preview));
        assert_eq!(fast_goal("把镜头1换成视频素材"), Some(LoopGoal::Timeline));
        assert_eq!(fast_goal("不要这么多警报的镜头"), Some(LoopGoal::Timeline));
        assert_eq!(fast_goal("创建剪映草稿"), Some(LoopGoal::JianyingDraft));
        assert_eq!(fast_goal("生成一个分镜脚本"), Some(LoopGoal::Storyboard));
        assert_eq!(
            fast_goal("你好，介绍一下这个项目"),
            Some(LoopGoal::Question)
        );
    }

    #[test]
    fn explicit_negative_side_effects_narrow_the_tool_set_and_goal() {
        let request = "仅调整当前内部时间线：缩短第 2 个镜头，不生成 preview，不创建 Jianying draft，不分析素材。";
        let policy = RequestToolPolicy::from_request(request);

        assert!(policy.forbids("render_preview"));
        assert!(policy.forbids("create_jianying_draft"));
        assert!(policy.forbids("request_asset_analysis"));
        assert!(policy.forbids("download_music"));
        assert!(policy.forbids("use_online_music"));
        assert!(!policy.forbids("change_clip_duration"));
        assert!(policy.forbids_goal(LoopGoal::Preview));
        assert!(!policy.forbids_goal(LoopGoal::Timeline));
        assert_eq!(fast_goal(request), Some(LoopGoal::Timeline));
    }

    #[test]
    fn positive_preview_requests_remain_available() {
        let policy = RequestToolPolicy::from_request("请为当前时间线生成 preview");
        assert!(!policy.forbids("render_preview"));
        assert_eq!(
            fast_goal("请为当前时间线生成 preview"),
            Some(LoopGoal::Preview)
        );
        assert!(
            !RequestToolPolicy::from_request("No preview exists; please generate one")
                .forbids("render_preview")
        );
        assert!(
            RequestToolPolicy::from_request("Do not generate a preview").forbids("render_preview")
        );
        assert!(
            RequestToolPolicy::from_request("Without creating a preview, adjust the timeline")
                .forbids("render_preview")
        );
        assert!(RequestToolPolicy::from_request("Don't render a preview").forbids("render_preview"));
        let no_analysis = RequestToolPolicy::from_request("Do not analyze media or assets");
        assert!(no_analysis.forbids("request_asset_analysis"));
        assert!(no_analysis.forbids("download_music"));
        assert!(no_analysis.forbids("use_online_music"));
        assert!(RequestToolPolicy::from_request("Do not reanalyze assets")
            .forbids("request_asset_analysis"));
    }

    #[test]
    fn explicit_read_only_requests_block_every_edit_tool() {
        let request = "只读检查当前 timeline 版本，不生成 preview，也不要修改任何产物。";
        let policy = RequestToolPolicy::from_request(request);

        assert!(policy.read_only);
        assert!(EDIT_TOOLS.iter().all(|tool| policy.forbids(tool)));
        assert!(!policy.forbids("get_timeline"));
        assert!(policy.forbids_goal(LoopGoal::Storyboard));
        assert!(policy.forbids_goal(LoopGoal::Timeline));
        assert!(policy.forbids_goal(LoopGoal::Preview));
        assert!(policy.forbids_goal(LoopGoal::JianyingDraft));
        assert!(!policy.forbids_goal(LoopGoal::Question));
        assert_eq!(fast_goal(request), Some(LoopGoal::Question));
        assert!(!RequestToolPolicy::from_request("不是只读，请调整 timeline").read_only);
        assert_eq!(
            fast_goal("不是只读，请调整 timeline"),
            Some(LoopGoal::Timeline)
        );
        let english_edit = "This isn't readonly; adjust the timeline";
        assert!(!RequestToolPolicy::from_request(english_edit).read_only);
        assert_eq!(fast_goal(english_edit), Some(LoopGoal::Timeline));
        let chinese_mode_edit = "不要用只读模式，请调整 timeline";
        assert!(!RequestToolPolicy::from_request(chinese_mode_edit).read_only);
        assert_eq!(fast_goal(chinese_mode_edit), Some(LoopGoal::Timeline));
        let english_mode_edit = "Don't use readonly mode; adjust the timeline";
        assert!(!RequestToolPolicy::from_request(english_mode_edit).read_only);
        assert_eq!(fast_goal(english_mode_edit), Some(LoopGoal::Timeline));
        for request in [
            "This is not in readonly mode; adjust the timeline",
            "Don't keep it readonly; adjust the timeline",
            "不要保持只读模式，请调整 timeline",
        ] {
            assert!(!RequestToolPolicy::from_request(request).read_only);
            assert_eq!(fast_goal(request), Some(LoopGoal::Timeline));
        }
        for request in [
            "Keep the current timeline readonly",
            "保持当前时间线只读",
            "Don't keep the intro; keep the current timeline readonly",
            "不要保持片头；保持当前时间线只读",
            "Don't keep the intro: keep the current timeline readonly",
            "不要保持片头：保持当前时间线只读",
            "Don't keep the intro — keep the current timeline readonly",
        ] {
            let policy = RequestToolPolicy::from_request(request);
            assert!(policy.read_only);
            assert!(EDIT_TOOLS.iter().all(|tool| policy.forbids(tool)));
            assert_eq!(fast_goal(request), Some(LoopGoal::Question));
        }
    }

    #[test]
    fn current_project_questions_require_observation_when_routing_falls_back() {
        assert!(request_requires_project_observation(
            "只读检查当前 timeline 是 v几、包含多少片段？"
        ));
        assert!(request_requires_project_observation(
            "How many clips are in the current timeline?"
        ));
        assert_eq!(fast_goal("当前 preview 状态"), None);
        assert_eq!(fast_goal("当前 timeline 版本"), None);
        assert_eq!(
            fast_goal("Adjust the current timeline"),
            Some(LoopGoal::Timeline)
        );
        assert_eq!(
            fast_goal("Shorten the current clip"),
            Some(LoopGoal::Timeline)
        );
        assert_eq!(
            fast_goal("Render the current preview"),
            Some(LoopGoal::Preview)
        );
        assert_eq!(fast_goal("Update the current timeline"), None);
        assert_eq!(fast_goal("Modify the current clip"), None);
        assert_eq!(fast_goal("Extend the current clip"), None);
        assert!(!request_requires_project_observation(
            "请解释 timeline 是什么？"
        ));
    }

    #[test]
    fn pinned_goal_allows_response_is_always_true() {
        // fast_goal 已降级为提示；纠偏逻辑在 try_build_route_decision 处理。
        assert!(pinned_goal_allows_response(None));
        assert!(pinned_goal_allows_response(Some(LoopGoal::Question)));
        assert!(pinned_goal_allows_response(Some(LoopGoal::Preview)));
        assert!(pinned_goal_allows_response(Some(LoopGoal::Timeline)));
    }

    #[test]
    fn project_questions_cannot_bypass_observation_with_respond() {
        assert!(question_scope_allows_route(Some("general"), "respond"));
        assert!(question_scope_allows_route(Some("general"), "run"));
        assert!(question_scope_allows_route(Some("project"), "run"));
        assert!(!question_scope_allows_route(Some("project"), "respond"));
        assert!(!question_scope_allows_route(None, "respond"));
    }

    #[test]
    fn grounded_project_question_finishes_without_redundant_confirmation() {
        let instruction = project_fact_completion_instruction(true, true);
        assert!(instruction.contains("choose finish now"));
        assert!(instruction.contains("Do not call a semantically overlapping observation tool"));
        assert!(!project_fact_completion_instruction(true, false).contains("choose finish now"));
    }

    #[test]
    fn clarification_resolution_targets_the_observed_record() {
        let pending = PendingClarificationSnapshot {
            id: "clarification-1".to_owned(),
            source_kind: "router".to_owned(),
            source_agent_task_id: None,
            goal: Some("storyboard".to_owned()),
            question: "请补充目标。".to_owned(),
            created_at: 1,
        };
        assert_eq!(
            clarification_resolution(Some(&pending), Some("resolve"))
                .expect("resolve clarification"),
            Some("clarification-1".to_owned())
        );
        assert_eq!(
            clarification_resolution(Some(&pending), Some("keep")).expect("keep clarification"),
            None
        );
        assert!(clarification_resolution(Some(&pending), None).is_err());
    }

    #[test]
    fn an_attempted_initial_skill_advances_the_next_model_step() {
        assert_eq!(first_model_step(&[]), 0);
        let attempted = vec![ExecutedStepSummary {
            step_number: 1,
            tool: "generate_storyboard".to_owned(),
            status: "failed".to_owned(),
            produced_artifact: None,
        }];
        assert_eq!(first_model_step(&attempted), 1);
    }

    #[test]
    fn fast_goal_answers_questions_instead_of_forcing_edits() {
        assert_eq!(
            fast_goal("请告诉我选择每个镜头的逻辑"),
            Some(LoopGoal::Question)
        );
        assert_eq!(fast_goal("草稿为什么没出现"), Some(LoopGoal::Question));
        assert_eq!(fast_goal("为什么预览是黑的"), Some(LoopGoal::Question));
    }

    #[test]
    fn fast_goal_leaves_ambiguous_requests_for_the_model() {
        assert_eq!(fast_goal("你好"), None);
        assert_eq!(fast_goal("怎么把镜头1换成另一个素材"), None);
    }

    #[test]
    fn declared_goal_prefers_a_truthful_question_flag() {
        assert_eq!(
            parse_declared_goal(Some("timeline"), Some(true)),
            Some(LoopGoal::Question)
        );
        assert_eq!(
            parse_declared_goal(Some("timeline"), Some(false)),
            Some(LoopGoal::Timeline)
        );
        assert_eq!(
            parse_declared_goal(Some("preview"), None),
            Some(LoopGoal::Preview)
        );
        assert_eq!(
            parse_declared_goal(Some("storyboard"), Some(false)),
            Some(LoopGoal::Storyboard)
        );
        assert_eq!(
            parse_declared_goal(Some("jianying_draft"), Some(false)),
            Some(LoopGoal::JianyingDraft)
        );
        assert_eq!(parse_declared_goal(Some("unknown"), None), None);
        assert_eq!(parse_declared_goal(None, Some(false)), None);
    }

    #[test]
    fn edit_status_prefers_current_scoped_artifacts() {
        let preview = EditArtifactState {
            has_storyboard: true,
            has_timeline: true,
            has_preview: true,
        };
        assert_eq!(
            edit_status_message(Some("completed"), preview),
            "已经生成可审阅的 local preview。"
        );
        assert!(edit_status_message(Some("running"), preview).contains("仍在处理中"));
        assert!(edit_status_message(Some("running"), preview).contains("local preview"));
        assert!(edit_status_message(Some("failed"), preview).contains("没有完成"));
        assert_eq!(
            edit_status_message(
                Some("completed"),
                EditArtifactState {
                    has_storyboard: true,
                    has_timeline: true,
                    has_preview: false,
                },
            ),
            "已经生成内部时间线，但还没有生成 local preview。"
        );
        assert!(edit_status_message(None, EditArtifactState::default()).contains("没有可检查"));
    }

    #[test]
    fn message_history_excludes_the_current_request_and_is_chronological() {
        let connection = Connection::open_in_memory().expect("open history test database");
        connection
            .execute_batch(
                "CREATE TABLE messages (
                    id TEXT,
                    conversation_id TEXT,
                    role TEXT,
                    content TEXT,
                    created_at INTEGER
                );",
            )
            .expect("create messages table");
        let insert = |id: &str, role: &str, content: &str, created_at: i64| {
            connection
                .execute(
                    "INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, "conv-1", role, content, created_at],
                )
                .expect("insert message");
        };
        insert("m1", "user", "你好，介绍一下这个项目", 1000);
        insert("m2", "agent", "这是一个本地视频剪辑 Agent。", 2000);
        insert("m3", "user", "请告诉我选择每个镜头的逻辑", 3000);
        let history = load_message_history(&connection, "conv-1", "请告诉我选择每个镜头的逻辑");
        assert_eq!(
            history,
            vec![
                ("user".to_owned(), "你好，介绍一下这个项目".to_owned()),
                (
                    "agent".to_owned(),
                    "这是一个本地视频剪辑 Agent。".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn message_history_drops_other_conversations() {
        let connection = Connection::open_in_memory().expect("open history test database");
        connection
            .execute_batch(
                "CREATE TABLE messages (
                    id TEXT,
                    conversation_id TEXT,
                    role TEXT,
                    content TEXT,
                    created_at INTEGER
                );",
            )
            .expect("create messages table");
        connection
            .execute(
                "INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params!["m1", "conv-1", "user", "hi there", 1000],
            )
            .expect("insert message");
        connection
            .execute(
                "INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params!["m2", "conv-2", "user", "other conversation", 2000],
            )
            .expect("insert message");
        let history = load_message_history(&connection, "conv-1", "hello");
        assert_eq!(history, vec![("user".to_owned(), "hi there".to_owned())]);
    }

    #[test]
    fn render_history_labels_speakers_in_order() {
        let history = vec![
            ("user".to_owned(), "你好".to_owned()),
            ("agent".to_owned(), "嗨".to_owned()),
            ("tool".to_owned(), "skill output".to_owned()),
        ];
        let rendered = render_history(&history);
        assert!(rendered.contains("用户: 你好"));
        assert!(rendered.contains("Agent: 嗨"));
        assert!(rendered.contains("系统: skill output"));
        assert!(render_history(&[]).contains("无"));
    }

    #[test]
    fn model_timeout_is_clamped_to_the_remaining_run_budget() {
        let now = Instant::now();
        assert_eq!(
            remaining_model_timeout(now + Duration::from_secs(20), now),
            Some(Duration::from_secs(20))
        );
        assert_eq!(
            remaining_model_timeout(now + Duration::from_secs(200), now),
            Some(AGENT_STEP_TIMEOUT)
        );
        assert_eq!(remaining_model_timeout(now, now), None);
    }

    #[test]
    fn pending_goal_snapshot_uses_neutral_guidance() {
        let snapshot = test_snapshot("pending");
        let hints = deterministic_prerequisite_hints(&snapshot).join("\n");
        assert!(hints.contains("声明 goal/isQuestion"));
        assert!(!hints.contains("问答目标"));
    }

    #[test]
    fn pending_clarification_is_loaded_from_structured_scope() {
        let connection = Connection::open_in_memory().expect("open clarification test database");
        connection
            .execute_batch(
                "CREATE TABLE pending_clarifications (
                    id TEXT, project_id TEXT, editing_task_id TEXT, conversation_id TEXT,
                    source_kind TEXT, source_agent_task_id TEXT, goal TEXT, question TEXT,
                    status TEXT, created_at INTEGER, updated_at INTEGER
                );
                INSERT INTO pending_clarifications VALUES (
                    'clarification-1', 'project-1', 'task-1', 'conv-1',
                    'router', NULL, 'storyboard', '请补充创作目标。', 'pending', 1, 2
                );",
            )
            .expect("create clarification task table");
        let pending = load_pending_clarification(&connection, "project-1", "task-1", "conv-1")
            .expect("load pending clarification")
            .expect("pending clarification exists");
        assert_eq!(pending.question, "请补充创作目标。");
        assert!(
            load_pending_clarification(&connection, "project-1", "task-1", "other-conv",)
                .expect("load other scope")
                .is_none()
        );
    }

    #[test]
    fn goal_satisfied_only_with_a_real_artifact() {
        let none = None;
        assert!(LoopGoal::Question.satisfied_by(&none));
        assert!(!LoopGoal::Preview.satisfied_by(&none));
        assert!(!LoopGoal::Timeline.satisfied_by(&none));
        let timeline = || AgentEditResult {
            agent_task_id: "t".to_owned(),
            message: "x".to_owned(),
            storyboard: None,
            timeline: Some(TimelineVersion {
                id: "timeline-1".to_owned(),
                project_id: "p".to_owned(),
                storyboard_version_id: "s".to_owned(),
                version_number: 6,
                clips: Vec::new(),
                text_tracks: Vec::new(),
                music_tracks: Vec::new(),
                quality_report: None,
                created_at: 0,
            }),
            preview: None,
            jianying_draft: None,
        };
        assert!(LoopGoal::Timeline.satisfied_by(&Some(timeline())));
        assert!(!LoopGoal::Preview.satisfied_by(&Some(timeline())));
    }

    #[test]
    fn terminal_without_artifact_is_honest_for_deliverable_goals() {
        let result = finalize_terminal("t", LoopGoal::Preview, None, "已替换所有警报镜头");
        assert!(!result.message.contains("已替换所有警报镜头"));
        assert!(result.message.contains("preview"));
        assert!(result.preview.is_none());
        let question = finalize_terminal("t", LoopGoal::Question, None, "当前有 5 个素材。");
        assert_eq!(question.message, "当前有 5 个素材。");
    }

    #[test]
    fn terminal_with_artifact_keeps_the_verified_tool_summary() {
        let previous = AgentEditResult {
            agent_task_id: "t".to_owned(),
            message: "tool message".to_owned(),
            storyboard: None,
            timeline: Some(TimelineVersion {
                id: "timeline-1".to_owned(),
                project_id: "p".to_owned(),
                storyboard_version_id: "s".to_owned(),
                version_number: 1,
                clips: Vec::new(),
                text_tracks: Vec::new(),
                music_tracks: Vec::new(),
                quality_report: None,
                created_at: 0,
            }),
            preview: None,
            jianying_draft: None,
        };
        let result = finalize_terminal("t", LoopGoal::Timeline, Some(previous), "preview 已生成");
        assert_eq!(result.message, "tool message");
        assert!(result.timeline.is_some());
        assert!(result.preview.is_none());
    }

    #[test]
    fn state_snapshot_serializes_only_compact_safe_facts() {
        let mut snapshot = test_snapshot("preview");
        snapshot.pending_clarification = Some(PendingClarificationSnapshot {
            id: "clarification-1".to_owned(),
            source_kind: "router".to_owned(),
            source_agent_task_id: None,
            goal: Some("preview".to_owned()),
            question: "需要横屏还是竖屏？".to_owned(),
            created_at: 1,
        });
        let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(serialized.contains("project-1"));
        assert!(serialized.contains("usableCount"));
        assert!(serialized.contains("executedSteps"));
        assert!(serialized.contains("需要横屏还是竖屏"));
        assert!(!serialized.to_lowercase().contains("path"));
        assert!(!serialized.to_lowercase().contains("evidence"));
        assert!(!serialized.contains("sourceReference"));
    }

    #[test]
    fn preview_hint_uses_an_existing_timeline_without_rebuilding_storyboard() {
        let mut snapshot = test_snapshot("preview");
        snapshot.artifacts.timeline = VersionArtifactSnapshot {
            exists: true,
            version_id: Some("timeline-7".to_owned()),
            version_number: Some(7),
        };
        let hints = deterministic_prerequisite_hints(&snapshot).join("\n");
        assert!(hints.contains("render_preview"));
        assert!(!hints.contains("依次"));

        let unmet = unmet_conditions(
            LoopGoal::Preview,
            &snapshot.assets,
            &snapshot.artifacts,
            false,
            false,
        );
        assert_eq!(unmet, vec!["requested_preview_not_produced"]);
    }

    #[test]
    fn pending_media_analysis_blocks_evidence_based_creation_hint() {
        let mut snapshot = test_snapshot("storyboard");
        snapshot.assets.usable_count = 0;
        snapshot.assets.pending_analysis_count = 2;
        snapshot.unmet_conditions = unmet_conditions(
            LoopGoal::Storyboard,
            &snapshot.assets,
            &snapshot.artifacts,
            false,
            false,
        );
        assert!(snapshot
            .unmet_conditions
            .contains(&"asset_analysis_incomplete".to_owned()));
        let hints = deterministic_prerequisite_hints(&snapshot).join("\n");
        assert!(hints.contains("不要从文件名猜测内容"));
        assert!(hints.contains("请求可用素材的本地分析"));
    }

    #[test]
    fn delivery_tools_require_a_scoped_timeline_instead_of_creating_one() {
        let timeline = TimelineVersion {
            id: "timeline-current".to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number: 1,
            clips: Vec::new(),
            text_tracks: Vec::new(),
            music_tracks: Vec::new(),
            quality_report: None,
            created_at: 1,
        };

        assert!(
            select_timeline_candidate(&[timeline.clone()], Some("timeline-foreign"), None)
                .is_none()
        );
        assert_eq!(
            select_timeline_candidate(&[timeline.clone()], None, None).map(|value| value.id),
            Some(timeline.id)
        );
        assert!(select_timeline_candidate(&[], None, None).is_none());
    }

    #[test]
    fn storyboard_source_failure_becomes_path_free_model_context() {
        let diagnostic = safe_tool_failure_context(
            "generate_storyboard",
            "storyboard_source_inventory_unavailable: visual_ready_candidates=101; accessible_source_files=0; source=\\\\private-server\\secret.mov",
        );
        assert_eq!(diagnostic["stage"], "storyboard_source_validation");
        assert_eq!(diagnostic["code"], "unavailable_media");
        assert_eq!(
            diagnostic["facts"][0],
            "101 imported assets have completed visual evidence"
        );
        assert_eq!(
            diagnostic["facts"][1],
            "0 of those source files are currently accessible"
        );
        let serialized = diagnostic.to_string();
        assert!(!serialized.contains("private-server"));
        assert!(!serialized.contains("secret.mov"));
    }

    #[test]
    fn storyboard_without_visual_evidence_recommends_analysis_not_relinking() {
        let diagnostic = safe_tool_failure_context(
            "generate_storyboard",
            "storyboard_visual_evidence_unavailable: visual_ready_candidates=0",
        );
        assert_eq!(diagnostic["stage"], "storyboard_evidence_validation");
        assert!(diagnostic["recovery"]
            .as_str()
            .is_some_and(|recovery| recovery.contains("visual analysis")));
        assert!(!diagnostic["recovery"]
            .as_str()
            .is_some_and(|recovery| recovery.contains("relink")));
    }

    #[test]
    fn failure_explanation_rejects_completion_claims() {
        assert!(safe_failure_explanation(
            "源素材当前不可访问，请重新关联后重试。"
        ));
        assert!(!safe_failure_explanation(
            "已生成 storyboard，但素材不可访问。"
        ));
        assert!(!safe_failure_explanation("Preview successfully generated."));
    }
}
