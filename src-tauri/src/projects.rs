//! local project、剪辑任务、conversation、消息与启动恢复的命令边界。
//! 启动恢复只协调领域模块，不猜测丢失的模型回答或覆盖用户数据。

use crate::assets::resume_incomplete_analysis;
use crate::capcut::resume_pending_capcut_registrations;
use crate::db::{now_millis, open_connection};
use crate::jianying::resume_pending_jianying_registrations;
use crate::models::{Conversation, EditingSession, EditingTask, Message, Project, StoreStatus};
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const MISSING_AGENT_REPLY_MESSAGE: &str = "上一条 Agent 任务已结束，但应用未能恢复其最终回复。请审阅当前 storyboard、时间线和 preview，并重新提问或继续操作。";
pub(crate) const DEFAULT_CANDIDATE_SCORE_FIRST_SLOTS: usize = 5;

pub(crate) fn candidate_score_first_slots(
    connection: &Connection,
    project_id: &str,
) -> Result<usize, String> {
    let settings: String = connection
        .query_row(
            "SELECT settings_json FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let settings: serde_json::Value =
        serde_json::from_str(&settings).map_err(|error| error.to_string())?;
    Ok(settings
        .get("candidateScoreFirstSlots")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_CANDIDATE_SCORE_FIRST_SLOTS))
}

#[tauri::command(async)]
pub fn get_candidate_score_first_slots(
    app: AppHandle,
    project_id: String,
) -> Result<usize, String> {
    let connection = open_connection(&app)?;
    candidate_score_first_slots(&connection, &project_id)
}

#[tauri::command(async)]
pub fn set_candidate_score_first_slots(
    app: AppHandle,
    project_id: String,
    score_first_slots: usize,
) -> Result<usize, String> {
    let connection = open_connection(&app)?;
    let settings: String = connection
        .query_row(
            "SELECT settings_json FROM projects WHERE id = ?1",
            [&project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mut settings: serde_json::Value =
        serde_json::from_str(&settings).map_err(|error| error.to_string())?;
    settings["candidateScoreFirstSlots"] = serde_json::json!(score_first_slots);
    connection
        .execute(
            "UPDATE projects SET settings_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![settings.to_string(), now_millis(), project_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(score_first_slots)
}

fn recover_missing_agent_completion_messages(connection: &Connection) -> Result<usize, String> {
    let mut statement = connection
        .prepare(
            "WITH latest_tasks AS (
               SELECT id, conversation_id,
                      ROW_NUMBER() OVER (
                        PARTITION BY conversation_id
                        ORDER BY created_at DESC, updated_at DESC, id DESC
                      ) as row_num
               FROM agent_tasks
               WHERE editing_task_id IS NOT NULL
                 AND conversation_id IS NOT NULL
                 AND status IN ('completed', 'partially_completed', 'failed', 'needs_clarification', 'needs_review')
             )
             SELECT task.id, task.project_id, task.editing_task_id, task.conversation_id
             FROM agent_tasks AS task
             JOIN conversations AS conversation ON conversation.id = task.conversation_id
             JOIN latest_tasks ON latest_tasks.id = task.id AND latest_tasks.row_num = 1
             LEFT JOIN messages ON messages.id = 'agent-task-result-' || task.id
               AND messages.conversation_id = task.conversation_id
             WHERE conversation.status = 'working'
               AND messages.id IS NULL",
        )
        .map_err(|error| error.to_string())?;
    let missing = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    if missing.is_empty() {
        return Ok(0);
    }
    let timestamp = now_millis();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    for (agent_task_id, project_id, editing_task_id, conversation_id) in &missing {
        transaction
            .execute(
                "UPDATE agent_tasks
                 SET status = 'needs_review',
                     error_message = COALESCE(error_message, 'The Agent reply was unavailable after task completion.'),
                     updated_at = ?1
                 WHERE id = ?2 AND project_id = ?3 AND editing_task_id = ?4 AND conversation_id = ?5",
                params![timestamp, agent_task_id, project_id, editing_task_id, conversation_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO messages (id, conversation_id, role, content, created_at)
                 VALUES ('agent-task-result-' || ?1, ?2, 'agent', ?3, ?4)",
                params![
                    agent_task_id,
                    conversation_id,
                    MISSING_AGENT_REPLY_MESSAGE,
                    timestamp
                ],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE conversations SET status = 'review', summary = ?1, updated_at = ?2
                 WHERE id = ?3 AND project_id = ?4 AND editing_task_id = ?5",
                params![
                    MISSING_AGENT_REPLY_MESSAGE,
                    timestamp,
                    conversation_id,
                    project_id,
                    editing_task_id
                ],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE editing_tasks SET updated_at = ?1 WHERE id = ?2 AND project_id = ?3",
                params![timestamp, editing_task_id, project_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                params![timestamp, project_id],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(missing.len())
}

#[tauri::command(async)]
pub fn initialize_local_store(app: AppHandle) -> Result<StoreStatus, String> {
    let start = std::time::Instant::now();
    log::info!("[PERF] initialize_local_store: starting");

    let connection = open_connection(&app)?;
    log::info!(
        "[PERF] initialize_local_store: open_connection took {:?}",
        start.elapsed()
    );

    let step_start = std::time::Instant::now();
    connection.execute(
        "UPDATE agent_tasks SET status = 'needs_review', error_message = COALESCE(error_message, 'The application stopped before this Agent operation completed.'), updated_at = ?1 WHERE status IN ('queued', 'running') AND editing_task_id IS NOT NULL",
        params![now_millis()],
    ).map_err(|error| error.to_string())?;
    log::info!(
        "[PERF] initialize_local_store: UPDATE agent_tasks took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    connection.execute(
        "UPDATE agent_run_steps SET status = 'failed', error_code = COALESCE(error_code, 'interrupted_requires_review'), completed_at = ?1, updated_at = ?1 WHERE status IN ('queued', 'running') AND agent_task_id IN (SELECT id FROM agent_tasks WHERE status = 'needs_review')",
        params![now_millis()],
    ).map_err(|error| error.to_string())?;
    log::info!(
        "[PERF] initialize_local_store: UPDATE agent_run_steps took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    recover_missing_agent_completion_messages(&connection)?;
    log::info!(
        "[PERF] initialize_local_store: recover_missing_agent_completion_messages took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    connection.execute(
        "UPDATE conversations SET status = 'review' WHERE status = 'working' AND id IN (SELECT conversation_id FROM agent_tasks WHERE status = 'needs_review' AND conversation_id IS NOT NULL)",
        [],
    ).map_err(|error| error.to_string())?;
    log::info!(
        "[PERF] initialize_local_store: UPDATE conversations (review) took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    connection
        .execute(
            "UPDATE conversations SET status = 'ready' WHERE status = 'working' AND id NOT IN (SELECT conversation_id FROM agent_tasks WHERE status = 'needs_review' AND conversation_id IS NOT NULL)",
            [],
        )
        .map_err(|error| error.to_string())?;
    log::info!(
        "[PERF] initialize_local_store: UPDATE conversations (ready) took {:?}",
        step_start.elapsed()
    );

    drop(connection);

    let step_start = std::time::Instant::now();
    let known_projects = {
        let connection = open_connection(&app)?;
        let mut statement = connection
            .prepare("SELECT id FROM projects")
            .map_err(|error| error.to_string())?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        ids
    };
    if let Ok(freed) = crate::preview_cache::remove_orphan_project_caches(&app, &known_projects) {
        if freed > 0 {
            log::info!("[PERF] initialize_local_store: removed orphan preview cache bytes={freed}");
        }
    }
    log::info!(
        "[PERF] initialize_local_store: orphan preview cache sweep took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    resume_incomplete_analysis(&app)?;
    log::info!(
        "[PERF] initialize_local_store: resume_incomplete_analysis took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    resume_pending_jianying_registrations(&app)?;
    resume_pending_capcut_registrations(&app)?;
    log::info!(
        "[PERF] initialize_local_store: resume_pending_jianying_registrations took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    crate::runtime_models::maybe_start_runtime_model_download(&app);
    log::info!(
        "[PERF] initialize_local_store: runtime_model_download kickoff took {:?}",
        step_start.elapsed()
    );

    log::info!(
        "[PERF] initialize_local_store: total time {:?}",
        start.elapsed()
    );
    Ok(StoreStatus {
        database_ready: true,
        schema_version: crate::db::SCHEMA_VERSION,
    })
}

#[tauri::command(async)]
pub fn create_project(
    app: AppHandle,
    name: String,
    library_ids: Option<Vec<String>>,
) -> Result<Project, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name cannot be empty.".to_owned());
    }
    let timestamp = now_millis();
    let project = Project {
        id: Uuid::new_v4().to_string(),
        name: name.to_owned(),
        created_at: timestamp,
        updated_at: timestamp,
    };
    let connection = open_connection(&app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO projects (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                project.id,
                project.name,
                project.created_at,
                project.updated_at
            ],
        )
        .map_err(|error| error.to_string())?;
    if let Some(ids) = library_ids {
        for id in ids {
            transaction
                .execute(
                    "INSERT INTO project_libraries (project_id, library_id) VALUES (?1, ?2)",
                    params![project.id, id],
                )
                .map_err(|error| error.to_string())?;
        }
    } else {
        transaction.execute("INSERT INTO project_libraries (project_id, library_id) SELECT ?1, id FROM shared_libraries", [&project.id]).map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(project)
}

#[tauri::command(async)]
pub fn list_projects(app: AppHandle) -> Result<Vec<Project>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection
        .prepare("SELECT id, name, created_at, updated_at FROM projects ORDER BY updated_at DESC")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn rename_project(app: AppHandle, project_id: String, name: String) -> Result<Project, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name cannot be empty.".to_owned());
    }
    let connection = open_connection(&app)?;
    rename_project_record(&connection, &project_id, name)
}

fn rename_project_record(
    connection: &Connection,
    project_id: &str,
    name: &str,
) -> Result<Project, String> {
    let timestamp = now_millis();
    let changed = connection
        .execute(
            "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, timestamp, project_id],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("Project was not found.".to_owned());
    }
    connection
        .query_row(
            "SELECT id, name, created_at, updated_at FROM projects WHERE id = ?1",
            params![project_id],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn create_conversation(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    title: String,
) -> Result<Conversation, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Conversation title cannot be empty.".to_owned());
    }
    let timestamp = now_millis();
    let connection = open_connection(&app)?;
    let task_exists = connection
        .query_row(
            "SELECT COUNT(*) FROM editing_tasks WHERE id = ?1 AND project_id = ?2",
            params![editing_task_id, project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    if task_exists != 1 {
        return Err("Editing task does not belong to this project.".to_owned());
    }
    let existing = connection
        .query_row(
            "SELECT id, project_id, editing_task_id, title, summary, status, created_at, updated_at FROM conversations WHERE project_id = ?1 AND editing_task_id = ?2 ORDER BY updated_at DESC LIMIT 1",
            params![project_id, editing_task_id],
            |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    editing_task_id: row.get(2)?,
                    title: row.get(3)?,
                    summary: row.get(4)?,
                    status: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(conversation) = existing {
        return Ok(conversation);
    }
    let conversation = Conversation {
        id: Uuid::new_v4().to_string(),
        project_id,
        editing_task_id,
        title: title.to_owned(),
        summary: String::new(),
        status: "ready".to_owned(),
        created_at: timestamp,
        updated_at: timestamp,
    };
    connection.execute(
        "INSERT INTO conversations (id, project_id, editing_task_id, title, summary, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![conversation.id, conversation.project_id, conversation.editing_task_id, conversation.title, conversation.summary, conversation.status, conversation.created_at, conversation.updated_at],
    ).map_err(|error| error.to_string())?;
    Ok(conversation)
}

#[tauri::command(async)]
pub fn create_editing_session(
    app: AppHandle,
    project_id: String,
    title: String,
) -> Result<EditingSession, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Editing session title cannot be empty.".to_owned());
    }
    let timestamp = now_millis();
    let session_id = Uuid::new_v4().to_string();
    let conversation_id = Uuid::new_v4().to_string();
    let connection = open_connection(&app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at) VALUES (?1, ?2, ?3, '', ?4, ?4)",
            params![session_id, project_id, title, timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO conversations (id, project_id, editing_task_id, title, summary, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, '', 'ready', ?5, ?5)",
            params![conversation_id, project_id, session_id, title, timestamp],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(EditingSession {
        id: session_id,
        project_id,
        conversation_id: Some(conversation_id),
        title: title.to_owned(),
        brief: String::new(),
        summary: String::new(),
        status: "ready".to_owned(),
        created_at: timestamp,
        updated_at: timestamp,
    })
}

#[tauri::command(async)]
pub fn list_editing_sessions(
    app: AppHandle,
    project_id: String,
) -> Result<Vec<EditingSession>, String> {
    let connection = open_connection(&app)?;
    editing_sessions_for_project(&connection, &project_id)
}

#[tauri::command(async)]
pub fn rename_editing_session(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    title: String,
) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Editing session title cannot be empty.".to_owned());
    }
    let connection = open_connection(&app)?;
    rename_editing_session_record(&connection, &project_id, &editing_task_id, title)
}

fn rename_editing_session_record(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    title: &str,
) -> Result<(), String> {
    let timestamp = now_millis();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE editing_tasks SET title = ?1, updated_at = ?2 WHERE id = ?3 AND project_id = ?4",
            params![title, timestamp, editing_task_id, project_id],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("Editing session was not found in this project.".to_owned());
    }
    transaction
        .execute(
            "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE editing_task_id = ?3 AND project_id = ?4",
            params![title, timestamp, editing_task_id, project_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn delete_project(app: AppHandle, project_id: String, confirmed: bool) -> Result<(), String> {
    if !confirmed {
        return Err("Deleting a project requires explicit confirmation.".to_owned());
    }
    let connection = open_connection(&app)?;
    let (timeline_ids, asset_ids) = delete_project_records(&connection, &project_id)?;
    drop(connection);

    if let Ok(root) = app.path().app_data_dir() {
        for timeline_id in timeline_ids {
            let _ = fs::remove_dir_all(root.join("previews").join(timeline_id));
        }
        for asset_id in asset_ids {
            let _ = fs::remove_dir_all(root.join("derived").join(asset_id));
        }
        let _ = fs::remove_dir_all(root.join("previews").join("cache").join(&project_id));
    }
    Ok(())
}

fn delete_project_records(
    connection: &Connection,
    project_id: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !exists {
        return Err("Project was not found.".to_owned());
    }
    let timeline_ids = connection
        .prepare("SELECT id FROM timeline_versions WHERE project_id = ?1")
        .map_err(|error| error.to_string())?
        .query_map(params![project_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let asset_ids = connection
        .prepare("SELECT id FROM assets WHERE project_id = ?1")
        .map_err(|error| error.to_string())?
        .query_map(params![project_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    for sql in [
        "DELETE FROM agent_run_steps WHERE project_id = ?1",
        "DELETE FROM agent_diagnostics WHERE project_id = ?1",
        "DELETE FROM pending_clarifications WHERE project_id = ?1",
        "DELETE FROM task_route_receipts WHERE project_id = ?1",
        "DELETE FROM pending_task_routes WHERE project_id = ?1",
        "DELETE FROM operation_logs WHERE project_id = ?1",
        "DELETE FROM messages WHERE conversation_id IN (SELECT id FROM conversations WHERE project_id = ?1)",
        "DELETE FROM agent_tasks WHERE project_id = ?1",
        "DELETE FROM timeline_versions WHERE project_id = ?1",
        "DELETE FROM storyboard_versions WHERE project_id = ?1",
        "DELETE FROM conversations WHERE project_id = ?1",
        "DELETE FROM task_state_snapshots WHERE project_id = ?1",
        "DELETE FROM editing_tasks WHERE project_id = ?1",
        "DELETE FROM asset_collections WHERE project_id = ?1",
        "DELETE FROM asset_tags WHERE project_id = ?1",
        "DELETE FROM assets WHERE project_id = ?1",
        "DELETE FROM project_libraries WHERE project_id = ?1",
        "DELETE FROM projects WHERE id = ?1",
    ] {
        transaction
            .execute(sql, params![project_id])
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok((timeline_ids, asset_ids))
}

/// 删除剪辑会话（editing task）及其会话消息、Agent 记录、storyboard/timeline 与本地 preview。
/// 项目级素材保留。必须 `confirmed=true`。
#[tauri::command(async)]
pub fn delete_editing_session(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    confirmed: bool,
) -> Result<(), String> {
    if !confirmed {
        return Err("Deleting an editing session requires explicit confirmation.".to_owned());
    }
    if project_id.trim().is_empty() || editing_task_id.trim().is_empty() {
        return Err("Project and editing session identifiers are required.".to_owned());
    }
    let connection = open_connection(&app)?;
    let timeline_ids = delete_editing_session_records(&connection, &project_id, &editing_task_id)?;
    drop(connection);
    for timeline_id in timeline_ids {
        let preview_dir = match app.path().app_data_dir() {
            Ok(root) => root.join("previews").join(&timeline_id),
            Err(_) => continue,
        };
        let _ = fs::remove_dir_all(preview_dir);
    }
    Ok(())
}

fn delete_editing_session_records(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
) -> Result<Vec<String>, String> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM editing_tasks WHERE id = ?1 AND project_id = ?2)",
            params![editing_task_id, project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !exists {
        return Err("Editing session was not found in this project.".to_owned());
    }

    let mut timeline_statement = connection
        .prepare(
            "SELECT timeline_versions.id
             FROM timeline_versions
             JOIN storyboard_versions
               ON storyboard_versions.id = timeline_versions.storyboard_version_id
             WHERE storyboard_versions.project_id = ?1
               AND storyboard_versions.editing_task_id = ?2",
        )
        .map_err(|error| error.to_string())?;
    let timeline_ids = timeline_statement
        .query_map(params![project_id, editing_task_id], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(timeline_statement);

    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let timestamp = now_millis();
    transaction
        .execute(
            "UPDATE agent_tasks
             SET status = 'cancelled',
                 error_message = COALESCE(error_message, 'Editing session was deleted.'),
                 updated_at = ?1
             WHERE project_id = ?2
               AND editing_task_id = ?3
               AND status IN ('queued', 'running')",
            params![timestamp, project_id, editing_task_id],
        )
        .map_err(|error| error.to_string())?;

    let deletes = [
        "DELETE FROM agent_run_steps WHERE project_id = ?1 AND editing_task_id = ?2",
        "DELETE FROM agent_diagnostics WHERE project_id = ?1 AND editing_task_id = ?2",
        "DELETE FROM pending_clarifications WHERE project_id = ?1 AND editing_task_id = ?2",
        "DELETE FROM task_route_receipts
         WHERE project_id = ?1
           AND (
             target_editing_task_id = ?2
             OR target_conversation_id IN (
               SELECT id FROM conversations WHERE project_id = ?1 AND editing_task_id = ?2
             )
             OR pending_task_route_id IN (
               SELECT id FROM pending_task_routes
               WHERE project_id = ?1 AND active_editing_task_id = ?2
             )
           )",
        "DELETE FROM pending_task_routes WHERE project_id = ?1 AND active_editing_task_id = ?2",
        "DELETE FROM operation_logs
         WHERE project_id = ?1
           AND (
             editing_task_id = ?2
             OR conversation_id IN (
               SELECT id FROM conversations WHERE project_id = ?1 AND editing_task_id = ?2
             )
           )",
        "DELETE FROM messages
         WHERE conversation_id IN (
           SELECT id FROM conversations WHERE project_id = ?1 AND editing_task_id = ?2
         )",
        "DELETE FROM agent_tasks WHERE project_id = ?1 AND editing_task_id = ?2",
        "DELETE FROM timeline_versions
         WHERE project_id = ?1
           AND storyboard_version_id IN (
             SELECT id FROM storyboard_versions WHERE project_id = ?1 AND editing_task_id = ?2
           )",
        "DELETE FROM storyboard_versions WHERE project_id = ?1 AND editing_task_id = ?2",
        "DELETE FROM conversations WHERE project_id = ?1 AND editing_task_id = ?2",
        "DELETE FROM task_state_snapshots WHERE project_id = ?1 AND editing_task_id = ?2",
        "DELETE FROM editing_tasks WHERE id = ?2 AND project_id = ?1",
    ];
    for sql in deletes {
        transaction
            .execute(sql, params![project_id, editing_task_id])
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(timeline_ids)
}

fn editing_sessions_for_project(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<EditingSession>, String> {
    let mut statement = connection
        .prepare(
            "
            SELECT
              editing_tasks.id,
              editing_tasks.project_id,
              conversations.id,
              CASE
                WHEN editing_tasks.title NOT IN ('新的剪辑任务', '新的剪辑会话', 'New edit session')
                  THEN editing_tasks.title
                WHEN conversations.title IS NOT NULL
                  THEN conversations.title
                ELSE editing_tasks.title
              END,
              editing_tasks.brief,
              COALESCE(NULLIF(conversations.summary, ''), editing_tasks.brief, ''),
              COALESCE(conversations.status, 'ready'),
              editing_tasks.created_at,
              MAX(editing_tasks.updated_at, COALESCE(conversations.updated_at, editing_tasks.updated_at))
            FROM editing_tasks
            LEFT JOIN conversations ON conversations.id = (
              SELECT candidate.id
              FROM conversations AS candidate
              WHERE candidate.editing_task_id = editing_tasks.id
              ORDER BY candidate.updated_at DESC
              LIMIT 1
            )
            WHERE editing_tasks.project_id = ?1
              AND NOT (
                editing_tasks.id = 'legacy-' || editing_tasks.project_id
                AND editing_tasks.title = '已有剪辑任务'
                AND editing_tasks.brief = ''
                AND conversations.id IS NULL
                AND NOT EXISTS (
                  SELECT 1 FROM storyboard_versions
                  WHERE storyboard_versions.editing_task_id = editing_tasks.id
                )
              )
            ORDER BY MAX(editing_tasks.updated_at, COALESCE(conversations.updated_at, editing_tasks.updated_at)) DESC
            ",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok(EditingSession {
                id: row.get(0)?,
                project_id: row.get(1)?,
                conversation_id: row.get(2)?,
                title: row.get(3)?,
                brief: row.get(4)?,
                summary: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn list_conversations(
    app: AppHandle,
    project_id: String,
    editing_task_id: Option<String>,
) -> Result<Vec<Conversation>, String> {
    let connection = open_connection(&app)?;
    let query = if editing_task_id.is_some() {
        "SELECT id, project_id, editing_task_id, title, summary, status, created_at, updated_at FROM conversations WHERE project_id = ?1 AND editing_task_id = ?2 ORDER BY updated_at DESC"
    } else {
        "SELECT id, project_id, editing_task_id, title, summary, status, created_at, updated_at FROM conversations WHERE project_id = ?1 ORDER BY updated_at DESC"
    };
    let mut statement = connection
        .prepare(query)
        .map_err(|error| error.to_string())?;
    let map_row = |row: &rusqlite::Row<'_>| {
        Ok(Conversation {
            id: row.get(0)?,
            project_id: row.get(1)?,
            editing_task_id: row.get(2)?,
            title: row.get(3)?,
            summary: row.get(4)?,
            status: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    };
    let rows = if let Some(task_id) = editing_task_id {
        statement.query_map(params![project_id, task_id], map_row)
    } else {
        statement.query_map(params![project_id], map_row)
    }
    .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn create_editing_task(
    app: AppHandle,
    project_id: String,
    title: String,
) -> Result<EditingTask, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Editing task title cannot be empty.".to_owned());
    }
    let timestamp = now_millis();
    let task = EditingTask {
        id: Uuid::new_v4().to_string(),
        project_id,
        title: title.to_owned(),
        brief: String::new(),
        created_at: timestamp,
        updated_at: timestamp,
    };
    let connection = open_connection(&app)?;
    connection.execute(
        "INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![task.id, task.project_id, task.title, task.brief, task.created_at, task.updated_at],
    ).map_err(|error| error.to_string())?;
    Ok(task)
}

#[tauri::command(async)]
pub fn list_editing_tasks(app: AppHandle, project_id: String) -> Result<Vec<EditingTask>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection.prepare(
        "
        SELECT id, project_id, title, brief, created_at, updated_at
        FROM editing_tasks
        WHERE project_id = ?1
          AND NOT (
            id = 'legacy-' || project_id
            AND title = '已有剪辑任务'
            AND brief = ''
            AND NOT EXISTS (
              SELECT 1 FROM conversations WHERE conversations.editing_task_id = editing_tasks.id
            )
            AND NOT EXISTS (
              SELECT 1 FROM storyboard_versions WHERE storyboard_versions.editing_task_id = editing_tasks.id
            )
          )
        ORDER BY updated_at DESC
        ",
    )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok(EditingTask {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                brief: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn update_editing_task_brief(
    app: AppHandle,
    editing_task_id: String,
    brief: String,
) -> Result<(), String> {
    let brief = brief.trim();
    if brief.is_empty() {
        return Err("Editing task brief cannot be empty.".to_owned());
    }
    let connection = open_connection(&app)?;
    let changed = connection.execute(
        "UPDATE editing_tasks SET brief = ?1, title = CASE WHEN title IN ('新的剪辑任务', '新的剪辑会话', 'New edit session') THEN substr(?1, 1, 28) ELSE title END, updated_at = ?2 WHERE id = ?3",
        params![brief, now_millis(), editing_task_id],
    ).map_err(|error| error.to_string())?;
    if changed != 1 {
        return Err("Editing task could not be found.".to_owned());
    }
    Ok(())
}

/// 新消息写入后刷新会话、任务与项目时间；用户首条消息把占位标题换成请求摘要。
/// 占位标题含前端中英两种默认名，改前端默认名时必须同步这里。
fn touch_after_message(connection: &Connection, message: &Message) -> Result<(), String> {
    connection
        .execute(
            "UPDATE conversations SET updated_at = ?1, summary = ?2, title = CASE WHEN title IN ('新的剪辑会话', 'New edit session') AND ?3 = 'user' THEN substr(?2, 1, 28) ELSE title END WHERE id = ?4",
            params![message.created_at, message.content, message.role, message.conversation_id],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE editing_tasks SET updated_at = ?1, title = CASE WHEN title IN ('新的剪辑任务', '新的剪辑会话', 'New edit session') AND ?2 = 'user' THEN substr(?3, 1, 28) ELSE title END WHERE id = (SELECT editing_task_id FROM conversations WHERE id = ?4)",
            params![message.created_at, message.role, message.content, message.conversation_id],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE projects SET updated_at = ?1 WHERE id = (SELECT project_id FROM conversations WHERE id = ?2)",
            params![message.created_at, message.conversation_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command(async)]
pub fn create_message(
    app: AppHandle,
    conversation_id: String,
    role: String,
    content: String,
    route_receipt: Option<String>,
) -> Result<Message, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("Message content cannot be empty.".to_owned());
    }
    if !matches!(
        role.as_str(),
        "user" | "assistant" | "agent" | "tool" | "system"
    ) {
        return Err("Message role is invalid.".to_owned());
    }

    let message = Message {
        id: Uuid::new_v4().to_string(),
        conversation_id,
        role,
        content: content.to_owned(),
        created_at: now_millis(),
    };
    let connection = open_connection(&app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    if message.role == "user" {
        crate::taskrouter::claim_route_receipt_for_user_message(
            &transaction,
            &message.conversation_id,
            &message.content,
            route_receipt.as_deref().unwrap_or_default(),
            &message.id,
        )?;
    }
    transaction.execute(
        "INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![message.id, message.conversation_id, message.role, message.content, message.created_at],
    ).map_err(|error| error.to_string())?;
    touch_after_message(&transaction, &message)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(message)
}

#[tauri::command(async)]
pub fn set_conversation_status(
    app: AppHandle,
    conversation_id: String,
    status: String,
) -> Result<(), String> {
    if !matches!(status.as_str(), "ready" | "working" | "review") {
        return Err("Conversation status is invalid.".to_owned());
    }
    let connection = open_connection(&app)?;
    let changed = connection
        .execute(
            "UPDATE conversations SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now_millis(), conversation_id],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("Conversation is unavailable.".to_owned());
    }
    Ok(())
}

#[tauri::command(async)]
pub fn list_messages(app: AppHandle, conversation_id: String) -> Result<Vec<Message>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection.prepare(
        "SELECT id, conversation_id, role, content, created_at FROM messages WHERE conversation_id = ?1 ORDER BY created_at ASC",
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![conversation_id], |row| {
            Ok(Message {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_session_names_are_editable_and_project_delete_is_scoped() {
        let connection = Connection::open_in_memory().expect("open project edit test database");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::migrate(&connection).expect("create current schema");
        crate::shared_library::migrate(&connection).expect("create shared library schema");
        connection
            .execute_batch(
                "
                INSERT INTO projects (id, name, created_at, updated_at) VALUES
                  ('project-1', 'Original', 1, 1),
                  ('project-2', 'Keep', 1, 1);
                INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at)
                VALUES ('session-1', 'project-1', 'Old session', '', 2, 2);
                INSERT INTO conversations (id, project_id, editing_task_id, title, status, created_at, updated_at)
                VALUES ('conversation-1', 'project-1', 'session-1', 'Old session', 'ready', 2, 2);
                INSERT INTO messages (id, conversation_id, role, content, created_at)
                VALUES ('message-1', 'conversation-1', 'user', 'hello', 3);
                INSERT INTO assets (id, project_id, kind, display_name, source_reference, created_at, updated_at)
                VALUES ('asset-1', 'project-1', 'video', 'Clip', 'C:/clip.mp4', 3, 3);
                ",
            )
            .expect("seed editable project");

        let project = rename_project_record(&connection, "project-1", "Renamed project")
            .expect("rename project");
        rename_editing_session_record(&connection, "project-1", "session-1", "Renamed session")
            .expect("rename session");

        assert_eq!(project.name, "Renamed project");
        let titles: (String, String) = connection
            .query_row(
                "SELECT editing_tasks.title, conversations.title
                 FROM editing_tasks JOIN conversations ON conversations.editing_task_id = editing_tasks.id
                 WHERE editing_tasks.id = 'session-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read synchronized names");
        assert_eq!(
            titles,
            ("Renamed session".to_owned(), "Renamed session".to_owned())
        );

        let (_, asset_ids) =
            delete_project_records(&connection, "project-1").expect("delete project records");
        assert_eq!(asset_ids, vec!["asset-1"]);
        let remaining_projects: i64 = connection
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .expect("count remaining projects");
        let deleted_messages: i64 = connection
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .expect("count deleted messages");
        assert_eq!(remaining_projects, 1);
        assert_eq!(deleted_messages, 0);
    }

    #[test]
    fn first_user_message_retitles_english_placeholder_session() {
        let connection = Connection::open_in_memory().expect("open retitle test database");
        crate::db::migrate(&connection).expect("create current schema");
        connection
            .execute_batch(
                "
                INSERT INTO projects (id, name, created_at, updated_at)
                VALUES ('project-1', 'Project', 1, 1);
                INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at)
                VALUES ('session-1', 'project-1', 'New edit session', '', 2, 2);
                INSERT INTO conversations (id, project_id, editing_task_id, title, status, created_at, updated_at)
                VALUES ('conversation-1', 'project-1', 'session-1', 'New edit session', 'ready', 2, 2);
                ",
            )
            .expect("insert english placeholder session");
        let message = Message {
            id: "message-1".to_owned(),
            conversation_id: "conversation-1".to_owned(),
            role: "user".to_owned(),
            content: "Make a travel vlog".to_owned(),
            created_at: 3,
        };

        touch_after_message(&connection, &message).expect("touch after message");

        let titles: (String, String) = connection
            .query_row(
                "SELECT editing_tasks.title, conversations.title FROM editing_tasks
                 JOIN conversations ON conversations.editing_task_id = editing_tasks.id",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read retitled names");
        assert_eq!(
            titles,
            ("Make a travel vlog".to_owned(), "Make a travel vlog".to_owned())
        );
    }

    #[test]
    fn editing_session_projection_uses_the_latest_legacy_conversation() {
        let connection = Connection::open_in_memory().expect("open session test database");
        crate::db::migrate(&connection).expect("create current schema");
        connection
            .execute_batch(
                "
                INSERT INTO projects (id, name, created_at, updated_at)
                VALUES ('project-1', 'Project', 1, 1);
                INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at)
                VALUES ('session-1', 'project-1', '新的剪辑会话', 'Brief', 2, 2);
                INSERT INTO conversations (id, project_id, editing_task_id, title, summary, status, created_at, updated_at)
                VALUES
                  ('older', 'project-1', 'session-1', 'Older', 'Old summary', 'ready', 3, 3),
                  ('newer', 'project-1', 'session-1', 'Newest session', 'Latest summary', 'working', 4, 5);
                ",
            )
            .expect("insert compatibility records");

        let sessions =
            editing_sessions_for_project(&connection, "project-1").expect("list sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].conversation_id.as_deref(), Some("newer"));
        assert_eq!(sessions[0].title, "Newest session");
        assert_eq!(sessions[0].summary, "Latest summary");
        assert_eq!(sessions[0].status, "working");
        assert_eq!(sessions[0].updated_at, 5);
    }

    #[test]
    fn startup_recovery_marks_a_terminal_task_without_a_reply_for_review() {
        let connection = Connection::open_in_memory().expect("open recovery test database");
        crate::db::migrate(&connection).expect("create recovery schema");
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
            .expect("seed missing completion reply");

        assert_eq!(
            recover_missing_agent_completion_messages(&connection)
                .expect("recover missing completion reply"),
            1
        );
        assert_eq!(
            recover_missing_agent_completion_messages(&connection)
                .expect("repeat completion recovery"),
            0
        );
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
            .expect("read recovered completion state");
        assert_eq!(task_status, "needs_review");
        assert_eq!(conversation_status, "review");
        assert_eq!(message_count, 1);
    }
}
