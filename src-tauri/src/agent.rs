//! 对话提交、异步 Agent task 生命周期与终态事务边界。
//! 这里负责任务落库和恢复，不决定模型应该调用哪个具体领域技能。

use crate::agentloop::{
    decide_conversation_route, native_tool_loop_enabled, read_scoped_edit_status,
    run_configured_loop, run_explicit_command, ConversationRouteDecision, InitialAgentSkill,
};
use crate::audit::{record_agent_operation, update_agent_task};
use crate::db::{now_millis, open_connection};
use crate::models::{AgentEditResult, ConversationTurnResult, StoryboardVersion, TimelineVersion};
use crate::provider::ModelAccess;
use crate::storyboard::load_storyboard_version;
use crate::timeline::{timeline_candidates_for_editing_task, timeline_candidates_for_storyboard};
use rusqlite::{params, Connection};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

/// 精确单命令绕过模型并直接执行具名技能；其他请求统一进入有界 Agent loop。
pub(crate) fn explicit_command_tool(request: &str) -> Option<&'static str> {
    let normalized = request.trim().trim_matches(|character: char| {
        matches!(
            character,
            '。' | '！' | '？' | '.' | '!' | '?' | '，' | ',' | ' '
        )
    });
    match normalized {
        "剪好了吗" | "剪好没有" | "做好了吗" | "完成了吗" | "现在剪好了吗" => {
            Some("get_edit_status")
        }
        "生成剪映草稿" | "创建剪映草稿" | "生成 Jianying draft" | "创建 Jianying draft" => {
            Some("create_jianying_draft")
        }
        "生成内部时间线"
        | "创建内部时间线"
        | "生成时间线"
        | "创建时间线"
        | "生成时间线草稿"
        | "创建时间线草稿" => Some("create_timeline_draft"),
        "生成预览" | "渲染预览" | "生成预览视频" | "渲染预览视频" | "生成低清预览" => {
            Some("render_preview")
        }
        _ => None,
    }
}

fn explicit_command_needs_model_recovery(
    tool: Option<&str>,
    has_storyboard: bool,
    has_requested_timeline: bool,
) -> bool {
    !has_storyboard
        && has_requested_timeline
        && matches!(tool, Some("render_preview" | "create_jianying_draft"))
}

fn should_use_native_loop(
    native_enabled: bool,
    has_initial_skill: bool,
    explicit_tool: Option<&str>,
) -> bool {
    native_enabled && !has_initial_skill && matches!(explicit_tool, None | Some("render_preview"))
}

fn load_agent_context(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    storyboard_version_id: Option<&str>,
    timeline_version_id: Option<&str>,
) -> Result<(String, Option<StoryboardVersion>, Vec<TimelineVersion>), String> {
    let conversation_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1 AND project_id = ?2 AND editing_task_id = ?3)",
            params![conversation_id, project_id, editing_task_id],
            |row| row.get(0),
        )
        .map_err(|_| "Conversation does not belong to the current editing task.".to_owned())?;
    if !conversation_exists {
        return Err("Conversation does not belong to the current editing task.".to_owned());
    }
    let storyboard: Option<StoryboardVersion> = storyboard_version_id
        .map(|id| load_storyboard_version(connection, id))
        .transpose()
        .map_err(|error| error.to_string())?;
    if let Some(value) = &storyboard {
        if value.project_id != project_id || value.editing_task_id != editing_task_id {
            return Err("Storyboard does not belong to the current editing task.".to_owned());
        }
    }
    let task_brief: String = connection
        .query_row(
            "SELECT brief FROM editing_tasks WHERE id = ?1 AND project_id = ?2",
            params![editing_task_id, project_id],
            |row| row.get(0),
        )
        .map_err(|_| "Editing task is unavailable.".to_owned())?;
    let mut timelines = match &storyboard {
        Some(value) => timeline_candidates_for_storyboard(connection, project_id, &value.id)?,
        None => timeline_candidates_for_editing_task(connection, project_id, editing_task_id)?,
    };
    if let Some(timeline_id) = timeline_version_id {
        let timeline = timelines
            .iter()
            .find(|timeline| timeline.id == timeline_id)
            .cloned()
            .ok_or_else(|| {
                "Selected timeline does not belong to the current editing task.".to_owned()
            })?;
        timelines = vec![timeline];
    }
    Ok((task_brief, storyboard, timelines))
}

fn safe_tool_failure(tool_name: &str, error: &str) -> serde_json::Value {
    let code = match error {
        "Storyboard referenced an invalid video time range."
        | "Storyboard shot duration exceeds its verified video source range." => {
            "invalid_source_time_range"
        }
        "Storyboard referenced an unavailable asset."
        | "Storyboard referenced video without a verified duration." => {
            "unavailable_media_evidence"
        }
        _ => "tool_execution_failed",
    };
    json!({ "tool": tool_name, "status": "failed", "code": code })
}

pub(crate) fn failed_agent_edit_result(agent_task_id: String, message: &str) -> AgentEditResult {
    AgentEditResult {
        agent_task_id,
        message: message.to_owned(),
        storyboard: None,
        timeline: None,
        preview: None,
        jianying_draft: None,
    }
}

pub(crate) fn resolve_pending_clarification(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    clarification_id: &str,
) -> Result<(), String> {
    let now = now_millis();
    let updated = connection
        .execute(
            "UPDATE pending_clarifications SET status = 'resolved', updated_at = ?5, resolved_at = ?5 WHERE project_id = ?1 AND editing_task_id = ?2 AND conversation_id = ?3 AND id = ?4 AND status = 'pending'",
            params![project_id, editing_task_id, conversation_id, clarification_id, now],
        )
        .map_err(|_| "Pending clarification could not be resolved.".to_owned())?;
    if updated != 1 {
        return Err("Pending clarification is not active in this scope.".to_owned());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn replace_pending_clarification(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    source_kind: &str,
    source_agent_task_id: Option<&str>,
    goal: Option<&str>,
    question: &str,
) -> Result<(), String> {
    let question = question.trim();
    if question.is_empty()
        || !matches!(source_kind, "router" | "agent_run")
        || (source_kind == "router") != source_agent_task_id.is_none()
    {
        return Err("Pending clarification was invalid.".to_owned());
    }
    let now = now_millis();
    connection
        .execute(
            "UPDATE pending_clarifications SET status = 'superseded', updated_at = ?4 WHERE project_id = ?1 AND editing_task_id = ?2 AND conversation_id = ?3 AND status = 'pending'",
            params![project_id, editing_task_id, conversation_id, now],
        )
        .map_err(|_| "Pending clarification could not be replaced.".to_owned())?;
    connection
        .execute(
            "INSERT INTO pending_clarifications (id, project_id, editing_task_id, conversation_id, source_kind, source_agent_task_id, goal, question, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9, ?9)",
            params![Uuid::new_v4().to_string(), project_id, editing_task_id, conversation_id, source_kind, source_agent_task_id, goal, question, now],
        )
        .map_err(|_| "Pending clarification could not be saved.".to_owned())?;
    Ok(())
}

/// 原子提交 task 终态、真实产物审计、确定性回复和 conversation 终态；失败只保存安全码。
fn finalize_agent_task(
    connection: &Connection,
    agent_task_id: &str,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    tool_name: &str,
    outcome: Result<AgentEditResult, String>,
    terminal_status: &str,
    clarification_goal: Option<&str>,
    completion_role: &str,
) -> Result<AgentEditResult, String> {
    match outcome {
        Ok(result) => {
            let mut summary = json!({
                "tool": tool_name,
                "storyboardVersionId": result.storyboard.as_ref().map(|value| &value.id),
                "timelineVersionId": result.timeline.as_ref().map(|value| &value.id),
                "previewTimelineVersionId": result.preview.as_ref().map(|value| &value.timeline_version_id),
                "jianyingRegistrationStatus": result.jianying_draft.as_ref().map(|value| &value.registration_status),
            });
            let status = match terminal_status {
                "completed" | "partially_completed" | "failed" | "needs_clarification" => {
                    terminal_status
                }
                _ => "failed",
            };
            summary["status"] = json!(status);
            if matches!(status, "partially_completed" | "failed") {
                summary["code"] = json!("agent_goal_not_reached");
            }
            let transaction = connection
                .unchecked_transaction()
                .map_err(|error| error.to_string())?;
            update_agent_task(
                &transaction,
                agent_task_id,
                None,
                status,
                Some(&summary),
                (status == "failed").then_some("Agent loop did not reach the requested goal."),
            )?;
            if status == "needs_clarification" {
                replace_pending_clarification(
                    &transaction,
                    project_id,
                    editing_task_id,
                    conversation_id,
                    "agent_run",
                    Some(agent_task_id),
                    clarification_goal,
                    &result.message,
                )?;
            }
            let has_artifact = result.storyboard.is_some()
                || result.timeline.is_some()
                || result.preview.is_some()
                || result.jianying_draft.is_some();
            if has_artifact {
                let (entity_type, entity_id) = if let Some(storyboard) = &result.storyboard {
                    ("storyboard_version", storyboard.id.as_str())
                } else if let Some(timeline) = &result.timeline {
                    ("timeline_version", timeline.id.as_str())
                } else {
                    ("agent_operation", agent_task_id)
                };
                record_agent_operation(
                    &transaction,
                    project_id,
                    editing_task_id,
                    conversation_id,
                    agent_task_id,
                    tool_name,
                    entity_type,
                    entity_id,
                    &summary,
                )?;
            }
            persist_agent_completion_message_with_role(
                &transaction,
                agent_task_id,
                project_id,
                editing_task_id,
                conversation_id,
                &result.message,
                completion_role,
            )?;
            transaction.commit().map_err(|error| error.to_string())?;
            Ok(result)
        }
        Err(error) => {
            let tool_result = safe_tool_failure(tool_name, &error);
            let transaction = connection
                .unchecked_transaction()
                .map_err(|error| error.to_string())?;
            update_agent_task(
                &transaction,
                agent_task_id,
                None,
                "failed",
                Some(&tool_result),
                Some(&error),
            )?;
            let result = failed_agent_edit_result(
                agent_task_id.to_owned(),
                "这次受限操作没有完成，我没有修改现有 storyboard、时间线或 preview。请重试，或补充你希望保留的素材和片段。",
            );
            persist_agent_completion_message_with_role(
                &transaction,
                agent_task_id,
                project_id,
                editing_task_id,
                conversation_id,
                &result.message,
                completion_role,
            )?;
            transaction.commit().map_err(|error| error.to_string())?;
            Ok(result)
        }
    }
}

#[tauri::command]
pub fn execute_agent_edit(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    conversation_id: String,
    storyboard_version_id: Option<String>,
    timeline_version_id: Option<String>,
    request: String,
    route_receipt: String,
) -> Result<String, String> {
    if request.trim().is_empty() {
        return Err("Agent edit request cannot be empty.".to_owned());
    }
    let connection = open_connection(&app)?;
    crate::taskrouter::consume_route_receipt(
        &connection,
        &project_id,
        &editing_task_id,
        &conversation_id,
        &request,
        &route_receipt,
        false,
    )?;
    spawn_agent_run(
        app,
        project_id,
        editing_task_id,
        conversation_id,
        storyboard_version_id,
        timeline_version_id,
        request,
        None,
        None,
    )
}

#[tauri::command]
/// 统一对话入口：先消费绑定完整请求与作用域的一次性 receipt，随后才允许写消息或运行技能。
pub fn submit_conversation_turn(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    conversation_id: String,
    storyboard_version_id: Option<String>,
    timeline_version_id: Option<String>,
    request: String,
    route_receipt: String,
) -> Result<ConversationTurnResult, String> {
    if request.trim().is_empty() {
        return Err("Conversation request cannot be empty.".to_owned());
    }
    let connection = open_connection(&app)?;
    let (task_brief, storyboard, timelines) = load_agent_context(
        &connection,
        &project_id,
        &editing_task_id,
        &conversation_id,
        storyboard_version_id.as_deref(),
        timeline_version_id.as_deref(),
    )?;
    crate::taskrouter::consume_route_receipt(
        &connection,
        &project_id,
        &editing_task_id,
        &conversation_id,
        &request,
        &route_receipt,
        true,
    )?;
    let explicit_tool = explicit_command_tool(&request);
    if explicit_tool != Some("get_edit_status") {
        crate::taskrouter::note_task_request(&connection, &project_id, &editing_task_id, &request)?;
    }
    if explicit_tool == Some("get_edit_status") {
        let message = read_scoped_edit_status(
            &app,
            &connection,
            &project_id,
            &editing_task_id,
            &conversation_id,
            None,
        )?;
        return Ok(ConversationTurnResult::Immediate {
            status: "response".to_owned(),
            message,
        });
    }
    if explicit_tool.is_some() {
        let agent_task_id = spawn_agent_run(
            app,
            project_id,
            editing_task_id,
            conversation_id,
            storyboard_version_id,
            timeline_version_id,
            request,
            None,
            None,
        )?;
        return Ok(ConversationTurnResult::Run { agent_task_id });
    }
    if native_tool_loop_enabled() {
        let agent_task_id = spawn_agent_run(
            app,
            project_id,
            editing_task_id,
            conversation_id,
            storyboard_version_id,
            timeline_version_id,
            request,
            None,
            None,
        )?;
        return Ok(ConversationTurnResult::Run { agent_task_id });
    }
    let access = ModelAccess::resolve().map_err(|_| "Agent model is unavailable.".to_owned())?;
    let route_decision = match decide_conversation_route(
        &connection,
        &project_id,
        &editing_task_id,
        &conversation_id,
        &request,
        &task_brief,
        &access,
        storyboard.as_ref(),
        &timelines,
    ) {
        Ok(decision) => decision,
        Err(error) => {
            log::warn!("Conversation route failed closed: {error}");
            let agent_task_id = spawn_agent_run(
                app,
                project_id,
                editing_task_id,
                conversation_id,
                storyboard_version_id,
                timeline_version_id,
                request,
                None,
                None,
            )?;
            return Ok(ConversationTurnResult::Run { agent_task_id });
        }
    };
    match route_decision {
        ConversationRouteDecision::Respond {
            message,
            resolved_clarification_id,
        } => {
            if let Some(clarification_id) = resolved_clarification_id {
                let transaction = connection
                    .unchecked_transaction()
                    .map_err(|error| error.to_string())?;
                resolve_pending_clarification(
                    &transaction,
                    &project_id,
                    &editing_task_id,
                    &conversation_id,
                    &clarification_id,
                )?;
                transaction.commit().map_err(|error| error.to_string())?;
            }
            Ok(ConversationTurnResult::Immediate {
                status: "response".to_owned(),
                message,
            })
        }
        ConversationRouteDecision::Clarify(message) => {
            let transaction = connection
                .unchecked_transaction()
                .map_err(|error| error.to_string())?;
            replace_pending_clarification(
                &transaction,
                &project_id,
                &editing_task_id,
                &conversation_id,
                "router",
                None,
                None,
                &message,
            )?;
            transaction.commit().map_err(|error| error.to_string())?;
            Ok(ConversationTurnResult::Immediate {
                status: "clarification".to_owned(),
                message,
            })
        }
        ConversationRouteDecision::Run {
            goal,
            tool,
            args,
            project_fact_question,
            resolved_clarification_id,
        } => {
            drop(connection);
            let agent_task_id = spawn_agent_run(
                app,
                project_id,
                editing_task_id,
                conversation_id,
                storyboard_version_id,
                timeline_version_id,
                request,
                Some(InitialAgentSkill {
                    project_fact_question,
                    goal,
                    tool,
                    args,
                }),
                resolved_clarification_id,
            )?;
            Ok(ConversationTurnResult::Run { agent_task_id })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_agent_run(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    conversation_id: String,
    storyboard_version_id: Option<String>,
    timeline_version_id: Option<String>,
    request: String,
    initial_skill: Option<InitialAgentSkill>,
    resolved_clarification_id: Option<String>,
) -> Result<String, String> {
    if request.trim().is_empty() {
        return Err("Agent request cannot be empty.".to_owned());
    }
    log::info!("Starting AI edit decision.");
    let connection = open_connection(&app)?;
    let agent_task_id = Uuid::new_v4().to_string();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO agent_tasks (id, project_id, editing_task_id, conversation_id, tool_name, status, input_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'agent_edit', 'queued', ?5, ?6, ?6)",
            params![
                agent_task_id.clone(),
                project_id,
                editing_task_id,
                conversation_id,
                json!({
                    "requestLength": request.chars().count(),
                    "storyboardVersionId": storyboard_version_id,
                    "timelineVersionId": timeline_version_id
                })
                .to_string(),
                now_millis()
            ],
        )
        .map_err(|error| error.to_string())?;
    if let Some(clarification_id) = resolved_clarification_id {
        resolve_pending_clarification(
            &transaction,
            &project_id,
            &editing_task_id,
            &conversation_id,
            &clarification_id,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    let worker_app = app.clone();
    let worker_task_id = agent_task_id.clone();
    let worker_project_id = project_id.clone();
    let worker_editing_task_id = editing_task_id.clone();
    let worker_conversation_id = conversation_id.clone();
    std::thread::spawn(move || {
        run_agent_edit(
            worker_app,
            &worker_task_id,
            worker_project_id,
            worker_editing_task_id,
            worker_conversation_id,
            storyboard_version_id,
            timeline_version_id,
            request,
            initial_skill,
        );
    });
    Ok(agent_task_id)
}

fn run_agent_edit(
    app: AppHandle,
    agent_task_id: &str,
    project_id: String,
    editing_task_id: String,
    conversation_id: String,
    storyboard_version_id: Option<String>,
    timeline_version_id: Option<String>,
    request: String,
    initial_skill: Option<InitialAgentSkill>,
) {
    let emit = |status: &str, result: AgentEditResult| {
        let event = crate::models::AgentEditEvent {
            agent_task_id: result.agent_task_id.clone(),
            status: status.to_owned(),
            result,
        };
        let _ = app.emit("agent-edit-completed", &event);
    };
    let outcome = run_agent_edit_pipeline(
        app.clone(),
        agent_task_id,
        project_id.clone(),
        editing_task_id.clone(),
        conversation_id.clone(),
        storyboard_version_id,
        timeline_version_id,
        request,
        initial_skill,
    );
    match outcome {
        Ok(result) => {
            let status = persisted_task_status(&app, agent_task_id);
            if let Ok(connection) = open_connection(&app) {
                let _ = crate::taskrouter::refresh_task_state_snapshot(
                    &connection,
                    &project_id,
                    &editing_task_id,
                );
            }
            emit(&status, result);
        }
        Err(error) => {
            log::warn!("Agent task pipeline failed: {error}");
            let connection = open_connection(&app).ok();
            let result = failed_agent_edit_result(
                agent_task_id.to_owned(),
                "这次受限操作没有完成，我没有修改现有 storyboard、时间线或 preview。请重试，或补充你希望保留的素材和片段。",
            );
            if let Some(connection) = &connection {
                let _ = crate::audit::record_agent_diagnostic(
                    connection,
                    &project_id,
                    &editing_task_id,
                    &conversation_id,
                    agent_task_id,
                    None,
                    "pipeline_error",
                    "pipeline_execution_failed",
                );
                // 失败终态和确定性回复必须一起提交；不能留下“task 已失败但 conversation 永远 working”。
                let transaction = connection.unchecked_transaction();
                if let Ok(transaction) = transaction {
                    let _ =
                        update_agent_task(&transaction, agent_task_id, None, "failed", None, None);
                    let _ = persist_agent_completion_message(
                        &transaction,
                        agent_task_id,
                        &project_id,
                        &editing_task_id,
                        &conversation_id,
                        &result.message,
                    );
                    let _ = transaction.commit();
                } else {
                    log::warn!("Failed Agent completion could not be persisted: DB transaction unavailable.");
                }
                let _ = crate::taskrouter::refresh_task_state_snapshot(
                    connection,
                    &project_id,
                    &editing_task_id,
                );
            }
            emit("failed", result);
        }
    }
}

pub(crate) fn persist_agent_completion_message(
    connection: &Connection,
    agent_task_id: &str,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    message: &str,
) -> Result<(), String> {
    persist_agent_completion_message_with_role(
        connection,
        agent_task_id,
        project_id,
        editing_task_id,
        conversation_id,
        message,
        "agent",
    )
}

fn persist_agent_completion_message_with_role(
    connection: &Connection,
    agent_task_id: &str,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    message: &str,
    role: &str,
) -> Result<(), String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Agent completion message cannot be empty.".to_owned());
    }
    if !matches!(role, "agent" | "assistant") {
        return Err("Agent completion message role is invalid.".to_owned());
    }
    // 由 task ID 派生稳定主键，使事件、轮询和重启恢复重复对账时仍只会插入一次回复。
    let message_id = format!("agent-task-result-{agent_task_id}");
    let timestamp = now_millis();
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO messages (id, conversation_id, role, content, created_at)
             SELECT ?1, ?2, ?3, ?4, ?5
             WHERE EXISTS (
               SELECT 1 FROM conversations
               WHERE id = ?2 AND project_id = ?6 AND editing_task_id = ?7
             )",
            params![
                message_id,
                conversation_id,
                role,
                message,
                timestamp,
                project_id,
                editing_task_id
            ],
        )
        .map_err(|error| error.to_string())?;
    if inserted == 0 {
        let already_persisted: bool = connection
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM messages
                   WHERE id = ?1 AND conversation_id = ?2 AND role = ?3
                 )",
                params![message_id, conversation_id, role],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !already_persisted {
            return Err("Agent completion conversation scope is invalid.".to_owned());
        }
    } else {
        connection
            .execute(
                "UPDATE conversations
                 SET updated_at = ?1, summary = ?2
                 WHERE id = ?3 AND project_id = ?4 AND editing_task_id = ?5",
                params![
                    timestamp,
                    message,
                    conversation_id,
                    project_id,
                    editing_task_id
                ],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "UPDATE editing_tasks SET updated_at = ?1 WHERE id = ?2 AND project_id = ?3",
                params![timestamp, editing_task_id, project_id],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                params![timestamp, project_id],
            )
            .map_err(|error| error.to_string())?;
    }
    connection
        .execute(
            "UPDATE conversations
             SET status = 'ready'
             WHERE id = ?1 AND project_id = ?2 AND editing_task_id = ?3
               AND NOT EXISTS (
                 SELECT 1 FROM agent_tasks AS newer_task
                 WHERE newer_task.conversation_id = ?1
                   AND newer_task.id <> ?4
                   AND newer_task.created_at >= (
                     SELECT current_task.created_at FROM agent_tasks AS current_task
                     WHERE current_task.id = ?4
                   )
                   AND newer_task.status IN ('queued', 'running')
               )
               AND NOT EXISTS (
                 SELECT 1 FROM messages AS newer_request
                 WHERE newer_request.conversation_id = ?1
                   AND newer_request.role = 'user'
                   AND newer_request.created_at > (
                     SELECT current_task.created_at FROM agent_tasks AS current_task
                     WHERE current_task.id = ?4
                   )
               )",
            params![conversation_id, project_id, editing_task_id, agent_task_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[rustfmt::skip]
pub(crate) fn persisted_task_status(app: &AppHandle, agent_task_id: &str) -> String {
    match open_connection(app) {
        Ok(connection) => connection
            .query_row("SELECT status FROM agent_tasks WHERE id = ?1", params![agent_task_id], |row| row.get::<_, String>(0))
            .unwrap_or_else(|e| { log::warn!("Agent task status DB read failed: {e}"); "failed".to_owned() }),
        Err(e) => { log::warn!("Agent task status: DB unavailable: {e}"); "failed".to_owned() }
    }
}

fn run_agent_edit_pipeline(
    app: AppHandle,
    agent_task_id: &str,
    project_id: String,
    editing_task_id: String,
    conversation_id: String,
    storyboard_version_id: Option<String>,
    timeline_version_id: Option<String>,
    request: String,
    initial_skill: Option<InitialAgentSkill>,
) -> Result<AgentEditResult, String> {
    let connection = open_connection(&app)?;
    let conversation_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1 AND project_id = ?2 AND editing_task_id = ?3)",
            params![conversation_id, project_id, editing_task_id],
            |row| row.get(0),
        )
        .map_err(|_| "Conversation does not belong to the current editing task.".to_owned())?;
    if !conversation_exists {
        return Err("Conversation does not belong to the current editing task.".to_owned());
    }
    let storyboard: Option<StoryboardVersion> = storyboard_version_id
        .as_ref()
        .map(|id| load_storyboard_version(&connection, id))
        .transpose()
        .map_err(|error| error.to_string())?;
    if let Some(storyboard) = &storyboard {
        if storyboard.project_id != project_id || storyboard.editing_task_id != editing_task_id {
            return Err("Storyboard does not belong to the current editing task.".to_owned());
        }
    }
    let task_brief: String = connection
        .query_row(
            "SELECT brief FROM editing_tasks WHERE id = ?1 AND project_id = ?2",
            params![editing_task_id, project_id],
            |row| row.get(0),
        )
        .map_err(|_| "Editing task is unavailable.".to_owned())?;
    let mut timelines = match &storyboard {
        Some(value) => timeline_candidates_for_storyboard(&connection, &project_id, &value.id)?,
        None => timeline_candidates_for_editing_task(&connection, &project_id, &editing_task_id)?,
    };
    if let Some(timeline_id) = timeline_version_id.as_deref() {
        let timeline = timelines
            .iter()
            .find(|timeline| timeline.id == timeline_id)
            .cloned()
            .ok_or_else(|| {
                "Selected timeline does not belong to the current editing task.".to_owned()
            })?;
        timelines = vec![timeline];
    }

    let explicit_tool = explicit_command_tool(&request);
    let tool_name = explicit_tool.unwrap_or("agent_loop").to_owned();
    update_agent_task(
        &connection,
        agent_task_id,
        Some(&tool_name),
        "running",
        None,
        None,
    )?;

    let needs_model_recovery = explicit_command_needs_model_recovery(
        explicit_tool,
        storyboard.is_some(),
        timeline_version_id.is_some(),
    );
    let native_completion = should_use_native_loop(
        native_tool_loop_enabled(),
        initial_skill.is_some(),
        explicit_tool,
    );
    let (outcome, terminal_status, clarification_goal) =
        if let Some(tool) = explicit_tool.filter(|_| !needs_model_recovery && !native_completion) {
            (
                run_explicit_command(
                    &app,
                    &connection,
                    agent_task_id,
                    &project_id,
                    &editing_task_id,
                    &conversation_id,
                    &task_brief,
                    tool,
                    timeline_version_id.as_deref(),
                    storyboard.as_ref(),
                    &timelines,
                ),
                "completed",
                None,
            )
        } else {
            let access = match ModelAccess::resolve() {
                Ok(access) => access,
                Err(error) => {
                    log::warn!("AI edit could not access the configured provider: {error}.");
                    let tool_result = json!({
                        "tool": "agent_loop",
                        "status": "failed",
                        "code": "provider_unavailable"
                    });
                    update_agent_task(
                        &connection,
                        agent_task_id,
                        None,
                        "failed",
                        Some(&tool_result),
                        Some(&error),
                    )?;
                    return Ok(failed_agent_edit_result(
                        agent_task_id.to_owned(),
                        "当前无法连接 Agent 模型，因此没有执行剪辑操作。请检查模型连接后重试。",
                    ));
                }
            };
            let loop_result = run_configured_loop(
                &app,
                &connection,
                agent_task_id,
                &project_id,
                &editing_task_id,
                &conversation_id,
                &request,
                &task_brief,
                &access,
                storyboard.as_ref(),
                &timelines,
                initial_skill,
            )?;
            (
                Ok(loop_result.result),
                loop_result.status.as_str(),
                Some(loop_result.goal.code()),
            )
        };
    finalize_agent_task(
        &connection,
        agent_task_id,
        &project_id,
        &editing_task_id,
        &conversation_id,
        &tool_name,
        outcome,
        terminal_status,
        clarification_goal,
        if native_completion {
            "assistant"
        } else {
            "agent"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_tool_failure_omits_the_original_error() {
        let failure = safe_tool_failure(
            "generate_storyboard",
            "Storyboard referenced an invalid video time range.",
        );
        assert_eq!(failure["code"], "invalid_source_time_range");
        assert!(failure.get("error").is_none());
    }

    #[test]
    fn unqualified_draft_command_stays_in_the_model_loop() {
        assert_eq!(explicit_command_tool("生成草稿"), None);
        assert_eq!(
            explicit_command_tool("创建时间线草稿"),
            Some("create_timeline_draft")
        );
        assert_eq!(
            explicit_command_tool("生成低清预览"),
            Some("render_preview")
        );
        assert_eq!(explicit_command_tool("草稿为什么没出现"), None);
        assert_eq!(explicit_command_tool("请把镜头 2 换成另一个素材"), None);
        assert_eq!(explicit_command_tool("剪好了吗？"), Some("get_edit_status"));
    }

    #[test]
    fn incomplete_preview_context_is_deferred_to_the_model() {
        assert!(explicit_command_needs_model_recovery(
            Some("render_preview"),
            false,
            true,
        ));
        assert!(explicit_command_needs_model_recovery(
            Some("create_jianying_draft"),
            false,
            true,
        ));
        assert!(!explicit_command_needs_model_recovery(
            Some("render_preview"),
            true,
            true,
        ));
        assert!(!explicit_command_needs_model_recovery(
            Some("create_timeline_draft"),
            false,
            true,
        ));
    }

    #[test]
    fn native_switch_routes_only_migrated_preview_explicit_command_to_native_loop() {
        assert!(should_use_native_loop(true, false, Some("render_preview")));
        assert!(should_use_native_loop(true, false, None));
        assert!(!should_use_native_loop(
            true,
            false,
            Some("create_timeline_draft")
        ));
        assert!(!should_use_native_loop(
            false,
            false,
            Some("render_preview")
        ));
        assert!(!should_use_native_loop(true, true, Some("render_preview")));
    }

    #[test]
    fn unsatisfied_loop_is_persisted_as_failed() {
        let connection = Connection::open_in_memory().expect("open agent task test database");
        crate::db::migrate(&connection).expect("migrate agent task test database");
        connection
            .execute_batch(
                "
                INSERT INTO projects (id, name, created_at, updated_at)
                VALUES ('project-1', 'Project', 1, 1);
                INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at)
                VALUES ('editing-task-1', 'project-1', 'Task', '', 1, 1);
                INSERT INTO conversations (id, project_id, editing_task_id, title, status, created_at, updated_at)
                VALUES ('conversation-1', 'project-1', 'editing-task-1', 'Conversation', 'working', 1, 1);
                INSERT INTO agent_tasks (
                  id, project_id, editing_task_id, conversation_id, tool_name, status,
                  input_json, created_at, updated_at
                ) VALUES (
                  'agent-task-1', 'project-1', 'editing-task-1', 'conversation-1',
                  'agent_loop', 'running', '{}', 2, 2
                );
                ",
            )
            .expect("seed agent task test scope");
        let result =
            failed_agent_edit_result("agent-task-1".to_owned(), "本轮没有生成新的 preview。");

        finalize_agent_task(
            &connection,
            "agent-task-1",
            "project-1",
            "editing-task-1",
            "conversation-1",
            "agent_loop",
            Ok(result),
            "failed",
            None,
            "agent",
        )
        .expect("persist failed loop result");

        let (status, result_json, error_message): (String, String, String) = connection
            .query_row(
                "SELECT status, result_json, error_message FROM agent_tasks WHERE id = 'agent-task-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read failed loop result");
        assert_eq!(status, "failed");
        assert!(result_json.contains("agent_goal_not_reached"));
        assert_eq!(
            error_message,
            "Agent operation failed. Reissue the request or review the local desktop message."
        );
    }

    #[test]
    fn agent_completion_message_is_idempotent_and_marks_conversation_ready() {
        let connection = Connection::open_in_memory().expect("open completion message database");
        crate::db::migrate(&connection).expect("migrate completion message database");
        connection
            .execute_batch(
                "INSERT INTO projects (id, name, created_at, updated_at)
                 VALUES ('project-1', 'Project', 1, 1);
                 INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at)
                 VALUES ('task-1', 'project-1', 'Task', '', 1, 1);
                 INSERT INTO conversations (id, project_id, editing_task_id, title, status, created_at, updated_at)
                 VALUES ('conversation-1', 'project-1', 'task-1', 'Conversation', 'working', 1, 1);
                 INSERT INTO agent_tasks (
                   id, project_id, editing_task_id, conversation_id, tool_name, status,
                   input_json, created_at, updated_at
                 ) VALUES (
                   'agent-task-1', 'project-1', 'task-1', 'conversation-1', 'agent_loop',
                   'completed', '{}', 2, 2
                 );",
            )
            .expect("seed completion message scope");

        for _ in 0..2 {
            persist_agent_completion_message(
                &connection,
                "agent-task-1",
                "project-1",
                "task-1",
                "conversation-1",
                "当前有 10 个可用视频。",
            )
            .expect("persist completion message");
        }

        let message_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE id = 'agent-task-result-agent-task-1'",
                [],
                |row| row.get(0),
            )
            .expect("count completion messages");
        let (status, summary): (String, String) = connection
            .query_row(
                "SELECT status, summary FROM conversations WHERE id = 'conversation-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read completion conversation");
        assert_eq!(message_count, 1);
        assert_eq!(status, "ready");
        assert_eq!(summary, "当前有 10 个可用视频。");

        connection
            .execute_batch(
                "INSERT INTO messages (id, conversation_id, role, content, created_at)
                 VALUES ('newer-user-message', 'conversation-1', 'user', '继续处理下一条请求', 3);
                 UPDATE conversations SET status = 'working' WHERE id = 'conversation-1';",
            )
            .expect("seed a newer request");
        persist_agent_completion_message(
            &connection,
            "agent-task-1",
            "project-1",
            "task-1",
            "conversation-1",
            "当前有 10 个可用视频。",
        )
        .expect("reconcile duplicate completion after newer request");
        let status_after_newer_request: String = connection
            .query_row(
                "SELECT status FROM conversations WHERE id = 'conversation-1'",
                [],
                |row| row.get(0),
            )
            .expect("read conversation status after newer request");
        assert_eq!(status_after_newer_request, "working");
    }

    #[test]
    fn finalizing_a_successful_agent_task_persists_its_reply_atomically() {
        let connection = Connection::open_in_memory().expect("open finalized task database");
        crate::db::migrate(&connection).expect("migrate finalized task database");
        connection
            .execute_batch(
                "INSERT INTO projects (id, name, created_at, updated_at)
                 VALUES ('project-1', 'Project', 1, 1);
                 INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at)
                 VALUES ('task-1', 'project-1', 'Task', '', 1, 1);
                 INSERT INTO conversations (id, project_id, editing_task_id, title, status, created_at, updated_at)
                 VALUES ('conversation-1', 'project-1', 'task-1', 'Conversation', 'working', 1, 1);
                 INSERT INTO agent_tasks (
                   id, project_id, editing_task_id, conversation_id, tool_name, status,
                   input_json, created_at, updated_at
                 ) VALUES (
                   'agent-task-1', 'project-1', 'task-1', 'conversation-1', 'agent_loop',
                   'running', '{}', 2, 2
                 );",
            )
            .expect("seed finalized task scope");

        let result = AgentEditResult {
            agent_task_id: "agent-task-1".to_owned(),
            message: "当前任务事实已经核对完成。".to_owned(),
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        };
        finalize_agent_task(
            &connection,
            "agent-task-1",
            "project-1",
            "task-1",
            "conversation-1",
            "agent_loop",
            Ok(result),
            "completed",
            None,
            "agent",
        )
        .expect("finalize successful task");

        let (task_status, conversation_status, message_count): (String, String, i64) = connection
            .query_row(
                "SELECT agent_tasks.status, conversations.status,
                        (SELECT COUNT(*) FROM messages WHERE id = 'agent-task-result-agent-task-1')
                 FROM agent_tasks
                 JOIN conversations ON conversations.id = agent_tasks.conversation_id
                 WHERE agent_tasks.id = 'agent-task-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read finalized task state");
        assert_eq!(task_status, "completed");
        assert_eq!(conversation_status, "ready");
        assert_eq!(message_count, 1);
    }

    #[test]
    fn native_completion_message_is_saved_as_assistant() {
        let connection = Connection::open_in_memory().expect("open native completion database");
        crate::db::migrate(&connection).expect("migrate native completion database");
        connection
            .execute_batch(
                "INSERT INTO projects (id, name, created_at, updated_at) VALUES ('p', 'Project', 1, 1);
                 INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at) VALUES ('t', 'p', 'Task', '', 1, 1);
                 INSERT INTO conversations (id, project_id, editing_task_id, title, status, created_at, updated_at) VALUES ('c', 'p', 't', 'Conversation', 'working', 1, 1);
                 INSERT INTO agent_tasks (id, project_id, editing_task_id, conversation_id, tool_name, status, input_json, created_at, updated_at) VALUES ('run', 'p', 't', 'c', 'agent_loop', 'completed', '{}', 2, 2);",
            )
            .expect("seed native completion scope");

        persist_agent_completion_message_with_role(
            &connection,
            "run",
            "p",
            "t",
            "c",
            "项目中有 10 个素材。",
            "assistant",
        )
        .expect("persist native assistant message");

        let role: String = connection
            .query_row(
                "SELECT role FROM messages WHERE id = 'agent-task-result-run'",
                [],
                |row| row.get(0),
            )
            .expect("read native assistant role");
        assert_eq!(role, "assistant");
    }

    #[test]
    fn replacing_a_clarification_keeps_only_one_pending_record() {
        let connection = Connection::open_in_memory().expect("open clarification database");
        connection
            .execute_batch(
                "CREATE TABLE pending_clarifications (
                  id TEXT PRIMARY KEY, project_id TEXT, editing_task_id TEXT, conversation_id TEXT,
                  source_kind TEXT, source_agent_task_id TEXT, goal TEXT, question TEXT,
                  status TEXT, created_at INTEGER, updated_at INTEGER, resolved_at INTEGER
                );
                CREATE UNIQUE INDEX pending_clarifications_one_active_idx
                  ON pending_clarifications(conversation_id) WHERE status = 'pending';",
            )
            .expect("create clarification schema");

        replace_pending_clarification(
            &connection,
            "project-1",
            "task-1",
            "conversation-1",
            "router",
            None,
            Some("storyboard"),
            "第一个问题",
        )
        .expect("insert first clarification");
        replace_pending_clarification(
            &connection,
            "project-1",
            "task-1",
            "conversation-1",
            "agent_run",
            Some("agent-task-1"),
            Some("timeline"),
            "第二个问题",
        )
        .expect("replace clarification");

        let pending_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pending_clarifications WHERE conversation_id = 'conversation-1' AND status = 'pending'",
                [],
                |row| row.get(0),
            )
            .expect("count pending clarifications");
        let superseded_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pending_clarifications WHERE conversation_id = 'conversation-1' AND status = 'superseded'",
                [],
                |row| row.get(0),
            )
            .expect("count superseded clarifications");
        assert_eq!(pending_count, 1);
        assert_eq!(superseded_count, 1);
    }

    #[test]
    fn clarification_resolution_rejects_wrong_scope_and_replay() {
        let connection = Connection::open_in_memory().expect("open clarification database");
        connection
            .execute_batch(
                "CREATE TABLE pending_clarifications (
                  id TEXT PRIMARY KEY, project_id TEXT, editing_task_id TEXT, conversation_id TEXT,
                  source_kind TEXT, source_agent_task_id TEXT, goal TEXT, question TEXT,
                  status TEXT, created_at INTEGER, updated_at INTEGER, resolved_at INTEGER
                );",
            )
            .expect("create clarification schema");
        let now = now_millis();
        connection
            .execute(
                "INSERT INTO pending_clarifications
                 (id, project_id, editing_task_id, conversation_id, source_kind, goal, question, status, created_at, updated_at)
                 VALUES ('clarification-1', 'project-1', 'task-1', 'conversation-1', 'agent_run', 'storyboard', '请确认', 'pending', ?1, ?1)",
                params![now],
            )
            .expect("seed pending clarification");

        assert!(resolve_pending_clarification(
            &connection,
            "project-1",
            "task-1",
            "wrong-conversation",
            "clarification-1",
        )
        .is_err());
        resolve_pending_clarification(
            &connection,
            "project-1",
            "task-1",
            "conversation-1",
            "clarification-1",
        )
        .expect("matching clarification scope");
        assert!(resolve_pending_clarification(
            &connection,
            "project-1",
            "task-1",
            "conversation-1",
            "clarification-1",
        )
        .is_err());
    }
}
