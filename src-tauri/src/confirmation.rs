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

const CONFIRMATION_MAX_AGE_MILLIS: i64 = 24 * 60 * 60 * 1_000;

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

    let agent_task_id = queue_storyboard_confirmation(
        &connection,
        &project_id,
        &editing_task_id,
        &conversation_id,
        &storyboard_version_id,
        storyboard.version_number,
        now_millis(),
    )?;

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

fn confirmation_is_fresh(updated_at: i64, now: i64) -> bool {
    updated_at > 0 && now >= updated_at && now - updated_at <= CONFIRMATION_MAX_AGE_MILLIS
}

#[allow(clippy::too_many_arguments)]
fn queue_storyboard_confirmation(
    connection: &rusqlite::Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    storyboard_version_id: &str,
    storyboard_version_number: i64,
    now: i64,
) -> Result<String, String> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|_| "Storyboard confirmation could not be queued.".to_owned())?;
    let (clarification_id, updated_at, source_status, source_result_json) = transaction
        .query_row(
            "SELECT pending.id, pending.updated_at, source.status, source.result_json
             FROM pending_clarifications AS pending
             JOIN agent_tasks AS source
               ON source.id = pending.source_agent_task_id
              AND source.project_id = pending.project_id
              AND source.editing_task_id = pending.editing_task_id
              AND source.conversation_id = pending.conversation_id
             WHERE pending.project_id = ?1
               AND pending.editing_task_id = ?2
               AND pending.conversation_id = ?3
               AND pending.status = 'pending'
               AND pending.goal = 'storyboard'
               AND pending.source_kind = 'agent_run'
               AND pending.source_agent_task_id IS NOT NULL
             ORDER BY pending.updated_at DESC
             LIMIT 1",
            params![project_id, editing_task_id, conversation_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .map_err(|_| "No active storyboard confirmation exists for this scope.".to_owned())?;
    if !confirmation_is_fresh(updated_at, now) {
        return Err("Storyboard confirmation has expired; request a new confirmation.".to_owned());
    }
    let source_receipt = source_result_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
    let receipt_matches = source_status == "needs_clarification"
        && source_receipt.as_ref().is_some_and(|receipt| {
            receipt["status"] == "needs_clarification"
                && receipt["storyboardVersionId"].as_str() == Some(storyboard_version_id)
        });
    if !receipt_matches {
        return Err("The requested storyboard is not the pending confirmed operation.".to_owned());
    }

    resolve_pending_clarification(
        &transaction,
        project_id,
        editing_task_id,
        conversation_id,
        &clarification_id,
    )?;
    transaction
        .execute(
            "INSERT INTO messages (id, conversation_id, role, content, created_at)
             VALUES (?1, ?2, 'user', ?3, ?4)",
            params![
                format!("confirm-storyboard-{}", Uuid::new_v4()),
                conversation_id,
                format!("确认 storyboard v{storyboard_version_number}"),
                now
            ],
        )
        .map_err(|_| "Storyboard confirmation message could not be saved.".to_owned())?;
    let agent_task_id = Uuid::new_v4().to_string();
    transaction
        .execute(
            "INSERT INTO agent_tasks (id, project_id, editing_task_id, conversation_id, tool_name, status, input_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'confirm_storyboard_and_preview', 'queued', ?5, ?6, ?6)",
            params![
                &agent_task_id,
                project_id,
                editing_task_id,
                conversation_id,
                json!({
                    "storyboardVersionId": storyboard_version_id,
                    "autoSequence": ["create_timeline_draft", "render_preview"]
                })
                .to_string(),
                now
            ],
        )
        .map_err(|_| "Storyboard confirmation task could not be queued.".to_owned())?;
    transaction
        .commit()
        .map_err(|_| "Storyboard confirmation could not be committed.".to_owned())?;
    Ok(agent_task_id)
}

#[cfg(test)]
mod tests {
    use super::{
        confirmation_is_fresh, queue_storyboard_confirmation, CONFIRMATION_MAX_AGE_MILLIS,
    };
    use rusqlite::{params, Connection};

    #[test]
    fn confirmation_freshness_rejects_expired_or_future_records() {
        let now = 10_000_000;
        assert!(confirmation_is_fresh(now - 1_000, now));
        assert!(!confirmation_is_fresh(now - 24 * 60 * 60 * 1_000 - 1, now));
        assert!(!confirmation_is_fresh(now + 1, now));
    }

    fn confirmation_database(now: i64) -> Connection {
        let connection = Connection::open_in_memory().expect("open confirmation database");
        connection
            .execute_batch(
                "CREATE TABLE pending_clarifications (
                   id TEXT PRIMARY KEY, project_id TEXT, editing_task_id TEXT, conversation_id TEXT,
                   source_kind TEXT, source_agent_task_id TEXT, goal TEXT, question TEXT,
                   status TEXT, created_at INTEGER, updated_at INTEGER, resolved_at INTEGER
                 );
                 CREATE TABLE messages (
                   id TEXT PRIMARY KEY, conversation_id TEXT, role TEXT, content TEXT, created_at INTEGER
                 );
                 CREATE TABLE agent_tasks (
                   id TEXT PRIMARY KEY, project_id TEXT, editing_task_id TEXT, conversation_id TEXT,
                   tool_name TEXT, status TEXT, input_json TEXT, result_json TEXT,
                   created_at INTEGER, updated_at INTEGER
                 );",
            )
            .expect("create confirmation schema");
        connection
            .execute(
                "INSERT INTO agent_tasks
                 (id, project_id, editing_task_id, conversation_id, tool_name, status, input_json, result_json, created_at, updated_at)
                 VALUES ('source-task', 'project-1', 'task-1', 'conversation-1', 'agent_loop', 'needs_clarification', '{}', ?1, ?2, ?2)",
                params![
                    serde_json::json!({
                        "status": "needs_clarification",
                        "storyboardVersionId": "storyboard-1"
                    })
                    .to_string(),
                    now
                ],
            )
            .expect("seed source task");
        connection
            .execute(
                "INSERT INTO pending_clarifications
                 (id, project_id, editing_task_id, conversation_id, source_kind, source_agent_task_id,
                  goal, question, status, created_at, updated_at)
                 VALUES ('clarification-1', 'project-1', 'task-1', 'conversation-1', 'agent_run',
                         'source-task', 'storyboard', 'Confirm storyboard', 'pending', ?1, ?1)",
                params![now],
            )
            .expect("seed pending confirmation");
        connection
    }

    #[test]
    fn confirmation_queue_is_scope_bound_atomic_and_single_use() {
        let now = 10_000_000;
        let connection = confirmation_database(now);
        for (project_id, editing_task_id, conversation_id, storyboard_id) in [
            ("wrong-project", "task-1", "conversation-1", "storyboard-1"),
            ("project-1", "wrong-task", "conversation-1", "storyboard-1"),
            ("project-1", "task-1", "wrong-conversation", "storyboard-1"),
            ("project-1", "task-1", "conversation-1", "wrong-storyboard"),
        ] {
            assert!(queue_storyboard_confirmation(
                &connection,
                project_id,
                editing_task_id,
                conversation_id,
                storyboard_id,
                1,
                now,
            )
            .is_err());
        }

        let confirmation_task_id = queue_storyboard_confirmation(
            &connection,
            "project-1",
            "task-1",
            "conversation-1",
            "storyboard-1",
            1,
            now,
        )
        .expect("matching confirmation queues the bound operation");
        let queued_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM agent_tasks WHERE id = ?1 AND tool_name = 'confirm_storyboard_and_preview' AND status = 'queued'",
                params![confirmation_task_id],
                |row| row.get(0),
            )
            .expect("count queued confirmation task");
        let message_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = 'conversation-1' AND role = 'user'",
                [],
                |row| row.get(0),
            )
            .expect("count confirmation messages");
        assert_eq!(queued_count, 1);
        assert_eq!(message_count, 1);

        assert!(queue_storyboard_confirmation(
            &connection,
            "project-1",
            "task-1",
            "conversation-1",
            "storyboard-1",
            1,
            now,
        )
        .is_err());
        let total_confirmation_tasks: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM agent_tasks WHERE tool_name = 'confirm_storyboard_and_preview'",
                [],
                |row| row.get(0),
            )
            .expect("count all confirmation tasks");
        assert_eq!(total_confirmation_tasks, 1);
    }

    #[test]
    fn expired_or_unbound_confirmation_never_queues_a_write() {
        let now = 100_000_000;
        let expired = confirmation_database(now - CONFIRMATION_MAX_AGE_MILLIS - 1);
        assert!(queue_storyboard_confirmation(
            &expired,
            "project-1",
            "task-1",
            "conversation-1",
            "storyboard-1",
            1,
            now,
        )
        .is_err());

        let unbound = confirmation_database(now);
        unbound
            .execute(
                "UPDATE pending_clarifications SET source_agent_task_id = NULL",
                [],
            )
            .expect("remove source binding");
        assert!(queue_storyboard_confirmation(
            &unbound,
            "project-1",
            "task-1",
            "conversation-1",
            "storyboard-1",
            1,
            now,
        )
        .is_err());

        for connection in [&expired, &unbound] {
            let queued: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM agent_tasks WHERE tool_name = 'confirm_storyboard_and_preview'",
                    [],
                    |row| row.get(0),
                )
                .expect("count forbidden confirmation tasks");
            assert_eq!(queued, 0);
        }
    }
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
    let mut timeline = crate::timeline::create_timeline_draft(
        app.clone(),
        project_id.clone(),
        storyboard.id.clone(),
    )?;
    let mut voiceover_failed = None;
    if let Some(narration) = crate::voice_provider::storyboard_narration_text(Some(&storyboard)) {
        match crate::voice_provider::synthesize_voiceover_for_timeline(
            &app,
            &connection,
            &project_id,
            &editing_task_id,
            &conversation_id,
            agent_task_id,
            &timeline,
            &narration,
            None,
        ) {
            Ok((updated, _)) => timeline = updated,
            Err(error) => {
                log::warn!("Storyboard confirmation voiceover failed: {error}");
                voiceover_failed = Some(error);
            }
        }
    }
    let timeline_version_id = timeline.id.clone();
    let timeline_version_number = timeline.version_number;

    // 步骤 2: render_preview
    let preview = crate::preview::render_preview(app.clone(), timeline.id.clone())?;

    let message = if voiceover_failed.is_some() {
        format!(
            "已确认 storyboard v{}，已创建时间线 v{} 并生成预览。配音未写入：ElevenLabs 拒绝了本次语音请求。可再说一次「配音」重试。",
            storyboard.version_number, timeline_version_number
        )
    } else {
        format!(
            "已确认 storyboard v{}，自动创建了时间线 v{} 并生成了预览。",
            storyboard.version_number, timeline_version_number
        )
    };

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
