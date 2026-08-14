use crate::db::{now_millis, open_connection};
use crate::models::{AgentDiagnostic, AgentRunStep, AgentTask, OperationLog};
use rusqlite::{params, Connection};
use std::time::Duration;
use tauri::AppHandle;
use uuid::Uuid;

pub(crate) fn record_agent_diagnostic(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    step_number: Option<i64>,
    kind: &str,
    content: &str,
) -> Result<(), String> {
    if !matches!(kind, "model_response" | "tool_error" | "pipeline_error")
        || !is_safe_diagnostic_content(content)
    {
        return Ok(());
    }
    connection.execute(
        "INSERT INTO agent_diagnostics (id, project_id, editing_task_id, conversation_id, agent_task_id, step_number, kind, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![Uuid::new_v4().to_string(), project_id, editing_task_id, conversation_id, agent_task_id, step_number, kind, content, now_millis()],
    ).map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) enum AgentTimingMetric {
    ModelRequest,
    SkillExecution,
    RunTotal,
}

pub(crate) fn record_agent_timing_diagnostic(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    step_number: Option<i64>,
    metric: AgentTimingMetric,
    elapsed: Duration,
) -> Result<(), String> {
    let metric = match metric {
        AgentTimingMetric::ModelRequest => "model_request_elapsed_ms",
        AgentTimingMetric::SkillExecution => "skill_execution_elapsed_ms",
        AgentTimingMetric::RunTotal => "run_total_elapsed_ms",
    };
    let elapsed_ms = elapsed.as_millis().min(i64::MAX as u128);
    record_agent_diagnostic(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        step_number,
        "model_response",
        &format!("{metric}={elapsed_ms}"),
    )
}

fn is_safe_diagnostic_content(content: &str) -> bool {
    !content.is_empty()
        && content.len() <= 128
        && content.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'=')
        })
}

#[tauri::command]
pub fn list_agent_diagnostics(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    agent_task_id: String,
) -> Result<Vec<AgentDiagnostic>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection.prepare("SELECT id, project_id, editing_task_id, conversation_id, agent_task_id, step_number, kind, content, created_at FROM agent_diagnostics WHERE project_id=?1 AND editing_task_id=?2 AND agent_task_id=?3 ORDER BY created_at ASC") .map_err(|error| error.to_string())?;
    let diagnostics = statement
        .query_map(params![project_id, editing_task_id, agent_task_id], |row| {
            Ok(AgentDiagnostic {
                id: row.get(0)?,
                project_id: row.get(1)?,
                editing_task_id: row.get(2)?,
                conversation_id: row.get(3)?,
                agent_task_id: row.get(4)?,
                step_number: row.get(5)?,
                kind: row.get(6)?,
                content: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(diagnostics)
}

fn audited_error(_error: &str) -> String {
    "Agent operation failed. Reissue the request or review the local desktop message.".to_owned()
}

fn validate_step_metadata(value: &str, field: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > max_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(format!("Agent run step {field} is invalid."));
    }
    Ok(())
}

pub(crate) fn begin_agent_run_step(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    agent_task_id: &str,
    step_number: i64,
    tool_name: &str,
) -> Result<String, String> {
    if step_number <= 0 {
        return Err("Agent run step number must be positive.".to_owned());
    }
    validate_step_metadata(tool_name, "tool name", 64)?;
    let step_id = Uuid::new_v4().to_string();
    let timestamp = now_millis();
    let inserted = connection
        .execute(
            "INSERT INTO agent_run_steps (id, project_id, editing_task_id, agent_task_id, step_number, tool_name, status, created_at, started_at, updated_at) SELECT ?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7, ?7, ?7 FROM agent_tasks WHERE id = ?4 AND project_id = ?2 AND editing_task_id = ?3",
            params![step_id, project_id, editing_task_id, agent_task_id, step_number, tool_name, timestamp],
        )
        .map_err(|error| error.to_string())?;
    if inserted != 1 {
        return Err("Agent run step scope does not match its run.".to_owned());
    }
    Ok(step_id)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_agent_run_step(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    agent_task_id: &str,
    step_id: &str,
    status: &str,
    artifact_type: Option<&str>,
    artifact_id: Option<&str>,
    error_code: Option<&str>,
) -> Result<(), String> {
    if !matches!(status, "completed" | "failed") {
        return Err("Agent run step terminal status is invalid.".to_owned());
    }
    if artifact_type.is_some() != artifact_id.is_some() {
        return Err("Agent run step artifact metadata must be complete.".to_owned());
    }
    if let Some(value) = artifact_type {
        validate_step_metadata(value, "artifact type", 64)?;
    }
    if let Some(value) = artifact_id {
        validate_step_metadata(value, "artifact id", 128)?;
    }
    if let Some(value) = error_code {
        validate_step_metadata(value, "error code", 128)?;
    }
    let updated = connection
        .execute(
            "UPDATE agent_run_steps SET status = ?1, artifact_type = ?2, artifact_id = ?3, error_code = ?4, completed_at = ?5, updated_at = ?5 WHERE id = ?6 AND project_id = ?7 AND editing_task_id = ?8 AND agent_task_id = ?9 AND status IN ('queued', 'running')",
            params![status, artifact_type, artifact_id, error_code, now_millis(), step_id, project_id, editing_task_id, agent_task_id],
        )
        .map_err(|error| error.to_string())?;
    if updated != 1 {
        return Err(
            "Agent run step was not found in the requested scope or is already terminal."
                .to_owned(),
        );
    }
    Ok(())
}

pub(crate) fn update_agent_task(
    connection: &Connection,
    task_id: &str,
    tool_name: Option<&str>,
    status: &str,
    result: Option<&serde_json::Value>,
    error: Option<&str>,
) -> Result<(), String> {
    connection.execute(
        "UPDATE agent_tasks SET tool_name = COALESCE(?1, tool_name), status = ?2, result_json = ?3, error_message = ?4, updated_at = ?5 WHERE id = ?6",
        params![tool_name, status, result.map(serde_json::Value::to_string), error.map(audited_error), now_millis(), task_id],
    ).map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn record_agent_operation(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    operation_type: &str,
    entity_type: &str,
    entity_id: &str,
    after: &serde_json::Value,
) -> Result<(), String> {
    connection.execute(
        "INSERT INTO operation_logs (id, project_id, editing_task_id, conversation_id, agent_task_id, actor, operation_type, entity_type, entity_id, before_json, after_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'agent', ?6, ?7, ?8, NULL, ?9, ?10)",
        params![Uuid::new_v4().to_string(), project_id, editing_task_id, conversation_id, agent_task_id, operation_type, entity_type, entity_id, after.to_string(), now_millis()],
    ).map_err(|error| error.to_string())?;
    Ok(())
}

fn parse_audit_json(value: Option<String>) -> Result<Option<serde_json::Value>, rusqlite::Error> {
    value
        .map(|json| serde_json::from_str(&json).map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()
}

#[tauri::command]
pub fn list_agent_tasks(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    conversation_id: Option<String>,
) -> Result<Vec<AgentTask>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection.prepare(
        "SELECT id, project_id, editing_task_id, conversation_id, tool_name, status, input_json, result_json, error_message, created_at, updated_at FROM agent_tasks WHERE project_id = ?1 AND editing_task_id = ?2 AND (?3 IS NULL OR conversation_id = ?3) ORDER BY updated_at DESC",
    ).map_err(|error| error.to_string())?;
    let tasks = statement
        .query_map(
            params![project_id, editing_task_id, conversation_id],
            |row| {
                let input: serde_json::Value = serde_json::from_str(&row.get::<_, String>(6)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(AgentTask {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    editing_task_id: row.get(2)?,
                    conversation_id: row.get(3)?,
                    tool_name: row.get(4)?,
                    status: row.get(5)?,
                    input,
                    result: parse_audit_json(row.get(7)?)?,
                    error: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(tasks)
}

pub(crate) fn list_agent_run_steps_in_connection(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    agent_task_id: &str,
) -> Result<Vec<AgentRunStep>, String> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, editing_task_id, agent_task_id, step_number, tool_name, status, artifact_type, artifact_id, error_code, created_at, started_at, completed_at, updated_at FROM agent_run_steps WHERE project_id = ?1 AND editing_task_id = ?2 AND agent_task_id = ?3 ORDER BY step_number ASC, created_at ASC",
    ).map_err(|error| error.to_string())?;
    let steps = statement
        .query_map(params![project_id, editing_task_id, agent_task_id], |row| {
            Ok(AgentRunStep {
                id: row.get(0)?,
                project_id: row.get(1)?,
                editing_task_id: row.get(2)?,
                agent_task_id: row.get(3)?,
                step_number: row.get(4)?,
                tool_name: row.get(5)?,
                status: row.get(6)?,
                artifact_type: row.get(7)?,
                artifact_id: row.get(8)?,
                error_code: row.get(9)?,
                created_at: row.get(10)?,
                started_at: row.get(11)?,
                completed_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(steps)
}

#[tauri::command]
pub fn list_agent_run_steps(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    agent_task_id: String,
) -> Result<Vec<AgentRunStep>, String> {
    let connection = open_connection(&app)?;
    list_agent_run_steps_in_connection(&connection, &project_id, &editing_task_id, &agent_task_id)
}

#[tauri::command]
pub fn list_operation_logs(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    agent_task_id: Option<String>,
) -> Result<Vec<OperationLog>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection.prepare(
        "SELECT id, project_id, editing_task_id, conversation_id, agent_task_id, actor, operation_type, entity_type, entity_id, before_json, after_json, created_at FROM operation_logs WHERE project_id = ?1 AND editing_task_id = ?2 AND (?3 IS NULL OR agent_task_id = ?3) ORDER BY created_at DESC",
    ).map_err(|error| error.to_string())?;
    let logs = statement
        .query_map(params![project_id, editing_task_id, agent_task_id], |row| {
            Ok(OperationLog {
                id: row.get(0)?,
                project_id: row.get(1)?,
                editing_task_id: row.get(2)?,
                conversation_id: row.get(3)?,
                agent_task_id: row.get(4)?,
                actor: row.get(5)?,
                operation_type: row.get(6)?,
                entity_type: row.get(7)?,
                entity_id: row.get(8)?,
                before: parse_audit_json(row.get(9)?)?,
                after: parse_audit_json(row.get(10)?)?,
                created_at: row.get(11)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(logs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate;
    use std::time::Duration;

    fn insert_run_scope(
        connection: &Connection,
        project_id: &str,
        editing_task_id: &str,
        agent_task_id: &str,
    ) {
        connection
            .execute(
                "INSERT INTO projects (id, name, created_at, updated_at) VALUES (?1, ?1, 1, 1)",
                params![project_id],
            )
            .expect("insert project");
        connection
            .execute(
                "INSERT INTO editing_tasks (id, project_id, title, created_at, updated_at) VALUES (?1, ?2, ?1, 1, 1)",
                params![editing_task_id, project_id],
            )
            .expect("insert editing task");
        connection
            .execute(
                "INSERT INTO conversations (id, project_id, editing_task_id, title, created_at, updated_at) VALUES (?2 || '-conversation', ?1, ?2, 'Conversation', 1, 1)",
                params![project_id, editing_task_id],
            )
            .expect("insert conversation");
        connection
            .execute(
                "INSERT INTO agent_tasks (id, project_id, editing_task_id, tool_name, status, input_json, created_at, updated_at) VALUES (?1, ?2, ?3, 'agent_edit', 'running', '{}', 1, 1)",
                params![agent_task_id, project_id, editing_task_id],
            )
            .expect("insert agent task");
    }

    #[test]
    fn timing_diagnostics_store_only_fixed_numeric_metrics() {
        let connection = Connection::open_in_memory().expect("open audit test database");
        migrate(&connection).expect("migrate audit test database");
        insert_run_scope(&connection, "project-a", "edit-a", "run-a");

        record_agent_timing_diagnostic(
            &connection,
            "project-a",
            "edit-a",
            "edit-a-conversation",
            "run-a",
            Some(1),
            AgentTimingMetric::ModelRequest,
            Duration::from_millis(1234),
        )
        .expect("record timing diagnostic");

        let content: String = connection
            .query_row(
                "SELECT content FROM agent_diagnostics WHERE agent_task_id = 'run-a'",
                [],
                |row| row.get(0),
            )
            .expect("read timing diagnostic");
        assert_eq!(content, "model_request_elapsed_ms=1234");
        assert!(is_safe_diagnostic_content(&content));
    }

    #[test]
    fn run_steps_are_written_and_queried_with_full_scope() {
        let connection = Connection::open_in_memory().expect("open audit test database");
        migrate(&connection).expect("migrate audit test database");
        insert_run_scope(&connection, "project-a", "edit-a", "run-a");
        insert_run_scope(&connection, "project-b", "edit-b", "run-b");

        let step_id = begin_agent_run_step(
            &connection,
            "project-a",
            "edit-a",
            "run-a",
            1,
            "list_assets",
        )
        .expect("begin run step");
        finish_agent_run_step(
            &connection,
            "project-a",
            "edit-a",
            "run-a",
            &step_id,
            "completed",
            Some("storyboard_version"),
            Some("storyboard-1"),
            None,
        )
        .expect("finish run step");
        begin_agent_run_step(
            &connection,
            "project-b",
            "edit-b",
            "run-b",
            1,
            "get_timeline",
        )
        .expect("begin other run step");

        let steps = list_agent_run_steps_in_connection(&connection, "project-a", "edit-a", "run-a")
            .expect("list scoped run steps");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].tool_name, "list_assets");
        assert_eq!(steps[0].status, "completed");
        assert_eq!(steps[0].artifact_id.as_deref(), Some("storyboard-1"));
        assert!(steps[0].completed_at.is_some());

        let cross_scope =
            list_agent_run_steps_in_connection(&connection, "project-b", "edit-b", "run-a")
                .expect("list mismatched scope");
        assert!(cross_scope.is_empty());
    }

    #[test]
    fn run_step_writes_reject_scope_mismatches_and_unsafe_metadata() {
        let connection = Connection::open_in_memory().expect("open audit test database");
        migrate(&connection).expect("migrate audit test database");
        insert_run_scope(&connection, "project-a", "edit-a", "run-a");
        insert_run_scope(&connection, "project-b", "edit-b", "run-b");

        assert!(begin_agent_run_step(
            &connection,
            "project-b",
            "edit-b",
            "run-a",
            1,
            "list_assets",
        )
        .is_err());
        assert!(begin_agent_run_step(
            &connection,
            "project-a",
            "edit-a",
            "run-a",
            1,
            "model output with spaces",
        )
        .is_err());

        let step_id = begin_agent_run_step(
            &connection,
            "project-a",
            "edit-a",
            "run-a",
            1,
            "list_assets",
        )
        .expect("begin valid step");
        assert!(finish_agent_run_step(
            &connection,
            "project-b",
            "edit-b",
            "run-b",
            &step_id,
            "completed",
            None,
            None,
            None,
        )
        .is_err());
    }

    #[test]
    fn diagnostics_accept_only_safe_codes() {
        let connection = Connection::open_in_memory().expect("open audit test database");
        migrate(&connection).expect("migrate audit test database");
        insert_run_scope(&connection, "project-a", "edit-a", "run-a");

        record_agent_diagnostic(
            &connection,
            "project-a",
            "edit-a",
            "edit-a-conversation",
            "run-a",
            Some(1),
            "pipeline_error",
            "provider_request_failed",
        )
        .expect("record safe diagnostic");
        record_agent_diagnostic(
            &connection,
            "project-a",
            "edit-a",
            "edit-a-conversation",
            "run-a",
            Some(1),
            "model_response",
            "the user's source media says secret.mp4",
        )
        .expect("ignore unsafe diagnostic");

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM agent_diagnostics", [], |row| {
                row.get(0)
            })
            .expect("count diagnostics");
        assert_eq!(count, 1);
    }
}
