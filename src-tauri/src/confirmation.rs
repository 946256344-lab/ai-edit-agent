//! 用户确认 storyboard 后的自动化流程：create_timeline_draft + render_preview。
//! 解决 pending clarification，创建后台任务，依次执行两步并发出完成事件。

use crate::agent::{
    failed_agent_edit_result, persist_agent_completion_message, persisted_task_status,
    resolve_pending_clarification,
};
use crate::audit::{record_agent_operation, update_agent_task};
use crate::db::{now_millis, open_connection};
use crate::models::AgentEditResult;
use crate::storyboard::load_storyboard_version;
use rusqlite::params;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

/// 用户确认 storyboard 后，自动依次执行 create_timeline_draft + render_preview。
#[tauri::command]
pub fn confirm_storyboard_and_preview(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    conversation_id: String,
    storyboard_version_id: String,
) -> Result<String, String> {
    let connection = open_connection(&app)?;
    // 验证 storyboard 归属当前剪辑任务
    let storyboard = load_storyboard_version(&connection, &storyboard_version_id)
        .map_err(|error| error.to_string())?;
    if storyboard.project_id != project_id || storyboard.editing_task_id != editing_task_id {
        return Err("Storyboard does not belong to the current editing task.".to_owned());
    }

    // 解决 pending clarification（如果存在）
    let pending_clarification = connection
        .query_row(
            "SELECT id FROM pending_clarifications WHERE project_id = ?1 AND editing_task_id = ?2 AND conversation_id = ?3 AND status = 'pending' AND goal = 'storyboard'",
            params![&project_id, &editing_task_id, &conversation_id],
            |row| row.get::<_, String>(0),
        )
        .ok();

    // 生成人工确认消息并解决 pending clarification
    let user_message_id = format!("confirm-storyboard-{}", Uuid::new_v4());
    let confirmation_text = format!("确认 storyboard v{}", storyboard.version_number);
    let timestamp = now_millis();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO messages (id, conversation_id, role, content, created_at)
             VALUES (?1, ?2, 'user', ?3, ?4)",
            params![
                user_message_id,
                conversation_id,
                confirmation_text,
                timestamp
            ],
        )
        .map_err(|error| error.to_string())?;

    if let Some(clarification_id) = pending_clarification {
        resolve_pending_clarification(
            &transaction,
            &project_id,
            &editing_task_id,
            &conversation_id,
            &clarification_id,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;

    // 创建后台 Agent 任务执行 timeline + preview
    let agent_task_id = Uuid::new_v4().to_string();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO agent_tasks (id, project_id, editing_task_id, conversation_id, tool_name, status, input_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'confirm_storyboard_and_preview', 'queued', ?5, ?6, ?6)",
            params![
                agent_task_id.clone(),
                project_id,
                editing_task_id,
                conversation_id,
                json!({
                    "storyboardVersionId": storyboard_version_id,
                    "autoSequence": ["create_timeline_draft", "render_preview"]
                })
                .to_string(),
                timestamp
            ],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;

    let worker_app = app.clone();
    let worker_task_id = agent_task_id.clone();
    let worker_project_id = project_id.clone();
    let worker_editing_task_id = editing_task_id.clone();
    let worker_conversation_id = conversation_id.clone();
    let worker_storyboard_id = storyboard_version_id.clone();

    std::thread::spawn(move || {
        run_storyboard_confirmation_sequence(
            worker_app,
            &worker_task_id,
            worker_project_id,
            worker_editing_task_id,
            worker_conversation_id,
            worker_storyboard_id,
        );
    });

    Ok(agent_task_id)
}

fn run_storyboard_confirmation_sequence(
    app: AppHandle,
    agent_task_id: &str,
    project_id: String,
    editing_task_id: String,
    conversation_id: String,
    storyboard_version_id: String,
) {
    let emit = |status: &str, result: AgentEditResult| {
        let event = crate::models::AgentEditEvent {
            agent_task_id: result.agent_task_id.clone(),
            status: status.to_owned(),
            result,
        };
        let _ = app.emit("agent-edit-completed", &event);
    };

    let outcome = run_confirmation_sequence_pipeline(
        app.clone(),
        agent_task_id,
        project_id.clone(),
        editing_task_id.clone(),
        conversation_id.clone(),
        storyboard_version_id,
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
            log::warn!("Storyboard confirmation sequence failed: {error}");
            let connection = open_connection(&app).ok();
            let result = failed_agent_edit_result(
                agent_task_id.to_owned(),
                "自动生成时间线和预览失败；请手动重试或补充说明。",
            );
            if let Some(connection) = &connection {
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

fn run_confirmation_sequence_pipeline(
    app: AppHandle,
    agent_task_id: &str,
    project_id: String,
    editing_task_id: String,
    conversation_id: String,
    storyboard_version_id: String,
) -> Result<AgentEditResult, String> {
    let connection = open_connection(&app)?;
    update_agent_task(&connection, agent_task_id, None, "running", None, None)?;

    let storyboard = load_storyboard_version(&connection, &storyboard_version_id)
        .map_err(|error| error.to_string())?;

    // 步骤 1: create_timeline_draft
    let timeline = crate::timeline::create_timeline_draft(
        app.clone(),
        project_id.clone(),
        storyboard.id.clone(),
    )?;
    let timeline_version_id = timeline.id.clone();
    let timeline_version_number = timeline.version_number;

    // 步骤 2: render_preview
    let preview = crate::preview::render_preview(app.clone(), timeline.id.clone())?;

    let message = format!(
        "已确认 storyboard v{}，自动创建了时间线 v{} 并生成了预览。",
        storyboard.version_number, timeline_version_number
    );

    let result = AgentEditResult {
        agent_task_id: agent_task_id.to_owned(),
        message: message.clone(),
        storyboard: Some(storyboard),
        timeline: Some(timeline),
        preview: Some(preview),
        jianying_draft: None,
    };

    let summary = json!({
        "tool": "confirm_storyboard_and_preview",
        "status": "completed",
        "storyboardVersionId": storyboard_version_id,
        "timelineVersionId": timeline_version_id,
        "previewTimelineVersionId": timeline_version_id,
    });

    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;

    update_agent_task(
        &transaction,
        agent_task_id,
        None,
        "completed",
        Some(&summary),
        None,
    )?;

    record_agent_operation(
        &transaction,
        &project_id,
        &editing_task_id,
        &conversation_id,
        agent_task_id,
        "confirm_storyboard_and_preview",
        "timeline_version",
        &timeline_version_id,
        &summary,
    )?;

    persist_agent_completion_message(
        &transaction,
        agent_task_id,
        &project_id,
        &editing_task_id,
        &conversation_id,
        &message,
    )?;

    transaction.commit().map_err(|error| error.to_string())?;

    Ok(result)
}
