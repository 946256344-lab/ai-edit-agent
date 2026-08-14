use crate::assets::resume_incomplete_analysis;
use crate::db::{now_millis, open_connection};
use crate::jianying::resume_pending_jianying_registrations;
use crate::models::{Conversation, EditingSession, EditingTask, Message, Project, StoreStatus};
use rusqlite::{params, Connection, OptionalExtension};
use tauri::AppHandle;
use uuid::Uuid;

const MISSING_AGENT_REPLY_MESSAGE: &str = "上一条 Agent 任务已结束，但应用未能恢复其最终回复。请审阅当前 storyboard、时间线和 preview，并重新提问或继续操作。";

fn recover_missing_agent_completion_messages(connection: &Connection) -> Result<usize, String> {
    let mut statement = connection
        .prepare(
            "SELECT task.id, task.project_id, task.editing_task_id, task.conversation_id
             FROM agent_tasks AS task
             JOIN conversations AS conversation ON conversation.id = task.conversation_id
             WHERE conversation.status = 'working'
               AND task.editing_task_id IS NOT NULL
               AND task.conversation_id IS NOT NULL
               AND task.status IN ('completed', 'partially_completed', 'failed', 'needs_clarification', 'needs_review')
               AND task.id = (
                 SELECT latest.id FROM agent_tasks AS latest
                 WHERE latest.conversation_id = task.conversation_id
                 ORDER BY latest.created_at DESC, latest.updated_at DESC, latest.id DESC
                 LIMIT 1
               )
               AND NOT EXISTS (
                 SELECT 1 FROM messages
                 WHERE messages.id = 'agent-task-result-' || task.id
                   AND messages.conversation_id = task.conversation_id
               )",
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

#[tauri::command]
pub fn initialize_local_store(app: AppHandle) -> Result<StoreStatus, String> {
    let connection = open_connection(&app)?;
    connection.execute(
        "UPDATE agent_tasks SET status = 'needs_review', error_message = COALESCE(error_message, 'The application stopped before this Agent operation completed.'), updated_at = ?1 WHERE status IN ('queued', 'running') AND editing_task_id IS NOT NULL",
        params![now_millis()],
    ).map_err(|error| error.to_string())?;
    connection.execute(
        "UPDATE agent_run_steps SET status = 'failed', error_code = COALESCE(error_code, 'interrupted_requires_review'), completed_at = ?1, updated_at = ?1 WHERE status IN ('queued', 'running') AND agent_task_id IN (SELECT id FROM agent_tasks WHERE status = 'needs_review')",
        params![now_millis()],
    ).map_err(|error| error.to_string())?;
    recover_missing_agent_completion_messages(&connection)?;
    connection.execute(
        "UPDATE conversations SET status = 'review' WHERE status = 'working' AND id IN (SELECT conversation_id FROM agent_tasks WHERE status = 'needs_review' AND conversation_id IS NOT NULL)",
        [],
    ).map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE conversations SET status = 'ready' WHERE status = 'working' AND id NOT IN (SELECT conversation_id FROM agent_tasks WHERE status = 'needs_review' AND conversation_id IS NOT NULL)",
            [],
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    resume_incomplete_analysis(&app)?;
    resume_pending_jianying_registrations(&app)?;
    Ok(StoreStatus {
        database_ready: true,
        schema_version: crate::db::SCHEMA_VERSION,
    })
}

#[tauri::command]
pub fn create_project(app: AppHandle, name: String) -> Result<Project, String> {
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
    connection
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
    Ok(project)
}

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
pub fn list_editing_sessions(
    app: AppHandle,
    project_id: String,
) -> Result<Vec<EditingSession>, String> {
    let connection = open_connection(&app)?;
    editing_sessions_for_project(&connection, &project_id)
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
                WHEN editing_tasks.title NOT IN ('新的剪辑任务', '新的剪辑会话')
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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
        "UPDATE editing_tasks SET brief = ?1, title = CASE WHEN title IN ('新的剪辑任务', '新的剪辑会话') THEN substr(?1, 1, 28) ELSE title END, updated_at = ?2 WHERE id = ?3",
        params![brief, now_millis(), editing_task_id],
    ).map_err(|error| error.to_string())?;
    if changed != 1 {
        return Err("Editing task could not be found.".to_owned());
    }
    Ok(())
}

#[tauri::command]
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
    if !matches!(role.as_str(), "user" | "agent" | "tool" | "system") {
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
    transaction
        .execute(
            "UPDATE conversations SET updated_at = ?1, summary = ?2, title = CASE WHEN title = '新的剪辑会话' AND ?3 = 'user' THEN substr(?2, 1, 28) ELSE title END WHERE id = ?4",
            params![message.created_at, message.content, message.role, message.conversation_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE editing_tasks SET updated_at = ?1, title = CASE WHEN title IN ('新的剪辑任务', '新的剪辑会话') AND ?2 = 'user' THEN substr(?3, 1, 28) ELSE title END WHERE id = (SELECT editing_task_id FROM conversations WHERE id = ?4)",
            params![message.created_at, message.role, message.content, message.conversation_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE projects SET updated_at = ?1 WHERE id = (SELECT project_id FROM conversations WHERE id = ?2)",
            params![message.created_at, message.conversation_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(message)
}

#[tauri::command]
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

#[tauri::command]
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
