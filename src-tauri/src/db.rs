//! SQLite location, connection policy, and schema migration boundary.
//!
//! Domain SQL remains in its owning module. Commands that need SQLite open the
//! same local database through this module so WAL, busy timeout, foreign keys,
//! and migrations are applied consistently. Cross-table domain transactions
//! must stay intact when large modules are later split.

use rusqlite::{params, Connection};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

pub(crate) const SCHEMA_VERSION: i64 = 14;

pub(crate) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory.join("assembly-video-agent.sqlite3"))
}

pub(crate) fn open_connection(app: &AppHandle) -> Result<Connection, String> {
    let connection = Connection::open(database_path(app)?).map_err(|error| error.to_string())?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| error.to_string())?;
    migrate(&connection)?;
    Ok(connection)
}

pub(crate) fn migrate(connection: &Connection) -> Result<(), String> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS projects (
          id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL, settings_json TEXT NOT NULL DEFAULT '{}',
          created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS conversations (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          title TEXT NOT NULL, summary TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'ready',
          created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS messages (
          id TEXT PRIMARY KEY NOT NULL, conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE RESTRICT,
          role TEXT NOT NULL CHECK(role IN ('user', 'agent', 'tool', 'system')), content TEXT NOT NULL, created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS assets (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          kind TEXT NOT NULL, display_name TEXT NOT NULL, source_reference TEXT NOT NULL,
          analysis_status TEXT NOT NULL DEFAULT 'queued', metadata_json TEXT NOT NULL DEFAULT '{}',
          created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS storyboard_versions (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          version_number INTEGER NOT NULL, status TEXT NOT NULL, content_json TEXT NOT NULL, created_at INTEGER NOT NULL,
          UNIQUE(project_id, version_number)
        );
        CREATE TABLE IF NOT EXISTS timeline_versions (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          storyboard_version_id TEXT REFERENCES storyboard_versions(id) ON DELETE RESTRICT,
          version_number INTEGER NOT NULL, status TEXT NOT NULL, content_json TEXT NOT NULL, created_at INTEGER NOT NULL,
          UNIQUE(project_id, version_number)
        );
        CREATE TABLE IF NOT EXISTS agent_tasks (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          conversation_id TEXT REFERENCES conversations(id) ON DELETE RESTRICT, tool_name TEXT NOT NULL, status TEXT NOT NULL,
          input_json TEXT NOT NULL, result_json TEXT, error_message TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS operation_logs (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          actor TEXT NOT NULL CHECK(actor IN ('user', 'agent', 'system')), operation_type TEXT NOT NULL,
          entity_type TEXT NOT NULL, entity_id TEXT NOT NULL, before_json TEXT, after_json TEXT, created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS conversations_project_updated_idx ON conversations(project_id, updated_at DESC);
        CREATE INDEX IF NOT EXISTS messages_conversation_created_idx ON messages(conversation_id, created_at ASC);
        CREATE INDEX IF NOT EXISTS assets_project_created_idx ON assets(project_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS agent_tasks_project_updated_idx ON agent_tasks(project_id, updated_at DESC);
        CREATE TABLE IF NOT EXISTS asset_user_metadata (
          asset_id TEXT PRIMARY KEY NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
          project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          favorite INTEGER NOT NULL DEFAULT 0 CHECK(favorite IN (0, 1)),
          rating INTEGER NOT NULL DEFAULT 0 CHECK(rating BETWEEN 0 AND 5),
          note TEXT NOT NULL DEFAULT '',
          excluded INTEGER NOT NULL DEFAULT 0 CHECK(excluded IN (0, 1)),
          updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS asset_user_metadata_project_idx ON asset_user_metadata(project_id, favorite, excluded, rating);
        CREATE TABLE IF NOT EXISTS asset_tags (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          name TEXT NOT NULL COLLATE NOCASE, created_at INTEGER NOT NULL,
          UNIQUE(project_id, name)
        );
        CREATE TABLE IF NOT EXISTS asset_tag_assignments (
          asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
          tag_id TEXT NOT NULL REFERENCES asset_tags(id) ON DELETE CASCADE,
          created_at INTEGER NOT NULL,
          PRIMARY KEY(asset_id, tag_id)
        );
        CREATE INDEX IF NOT EXISTS asset_tag_assignments_tag_idx ON asset_tag_assignments(tag_id, asset_id);
        CREATE TABLE IF NOT EXISTS asset_collections (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          name TEXT NOT NULL COLLATE NOCASE, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
          UNIQUE(project_id, name)
        );
        CREATE TABLE IF NOT EXISTS asset_collection_items (
          collection_id TEXT NOT NULL REFERENCES asset_collections(id) ON DELETE CASCADE,
          asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
          created_at INTEGER NOT NULL,
          PRIMARY KEY(collection_id, asset_id)
        );
        CREATE INDEX IF NOT EXISTS asset_collection_items_asset_idx ON asset_collection_items(asset_id, collection_id);
        CREATE TABLE IF NOT EXISTS asset_source_health (
          asset_id TEXT PRIMARY KEY NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
          project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          status TEXT NOT NULL DEFAULT 'unchecked' CHECK(status IN ('unchecked', 'online', 'missing', 'changed', 'unreadable')),
          baseline_size INTEGER, baseline_modified_ms INTEGER,
          observed_size INTEGER, observed_modified_ms INTEGER,
          reason_code TEXT,
          checked_at INTEGER, updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS asset_source_health_project_status_idx ON asset_source_health(project_id, status, checked_at DESC);
        ",
    ).map_err(|error| error.to_string())?;
    let folder_reference_column_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('assets') WHERE name = 'folder_reference'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if folder_reference_column_count == 0 {
        connection
            .execute("ALTER TABLE assets ADD COLUMN folder_reference TEXT", [])
            .map_err(|error| error.to_string())?;
    }
    let source_health_reason_column_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('asset_source_health') WHERE name = 'reason_code'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if source_health_reason_column_count == 0 {
        connection
            .execute(
                "ALTER TABLE asset_source_health ADD COLUMN reason_code TEXT",
                [],
            )
            .map_err(|error| error.to_string())?;
    }
    for (table, column, definition) in [
        ("agent_tasks", "editing_task_id", "TEXT"),
        ("operation_logs", "editing_task_id", "TEXT"),
        ("operation_logs", "conversation_id", "TEXT"),
        ("operation_logs", "agent_task_id", "TEXT"),
    ] {
        let column_count: i64 = connection
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"
                ),
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if column_count == 0 {
            connection
                .execute(
                    &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS editing_tasks (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          title TEXT NOT NULL, brief TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS editing_tasks_project_updated_idx ON editing_tasks(project_id, updated_at DESC);
        CREATE TABLE IF NOT EXISTS agent_run_steps (
          id TEXT PRIMARY KEY NOT NULL,
          project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          editing_task_id TEXT NOT NULL REFERENCES editing_tasks(id) ON DELETE RESTRICT,
          agent_task_id TEXT NOT NULL REFERENCES agent_tasks(id) ON DELETE RESTRICT,
          step_number INTEGER NOT NULL CHECK(step_number > 0),
          tool_name TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('queued', 'running', 'completed', 'failed')),
          artifact_type TEXT,
          artifact_id TEXT,
          error_code TEXT,
          created_at INTEGER NOT NULL,
          started_at INTEGER,
          completed_at INTEGER,
          updated_at INTEGER NOT NULL,
          UNIQUE(agent_task_id, step_number)
        );
        CREATE INDEX IF NOT EXISTS agent_run_steps_scope_idx
          ON agent_run_steps(project_id, editing_task_id, agent_task_id, step_number ASC);
        CREATE TABLE IF NOT EXISTS agent_diagnostics (
          id TEXT PRIMARY KEY NOT NULL,
          project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          editing_task_id TEXT NOT NULL REFERENCES editing_tasks(id) ON DELETE RESTRICT,
          conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE RESTRICT,
          agent_task_id TEXT NOT NULL REFERENCES agent_tasks(id) ON DELETE RESTRICT,
          step_number INTEGER,
          kind TEXT NOT NULL CHECK(kind IN ('model_response', 'tool_error', 'pipeline_error')),
          content TEXT NOT NULL,
          created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS agent_diagnostics_scope_idx
          ON agent_diagnostics(project_id, editing_task_id, agent_task_id, created_at ASC);
        CREATE TABLE IF NOT EXISTS pending_clarifications (
          id TEXT PRIMARY KEY NOT NULL,
          project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          editing_task_id TEXT NOT NULL REFERENCES editing_tasks(id) ON DELETE RESTRICT,
          conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE RESTRICT,
          source_kind TEXT NOT NULL CHECK(source_kind IN ('router', 'agent_run')),
          source_agent_task_id TEXT REFERENCES agent_tasks(id) ON DELETE RESTRICT,
          goal TEXT CHECK(goal IS NULL OR goal IN ('question', 'storyboard', 'timeline', 'preview', 'jianying_draft')),
          question TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('pending', 'resolved', 'superseded')),
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          resolved_at INTEGER
        );
        CREATE UNIQUE INDEX IF NOT EXISTS pending_clarifications_one_active_idx
          ON pending_clarifications(conversation_id) WHERE status = 'pending';
        CREATE INDEX IF NOT EXISTS pending_clarifications_scope_idx
          ON pending_clarifications(project_id, editing_task_id, conversation_id, status, updated_at DESC);
        CREATE TABLE IF NOT EXISTS task_state_snapshots (
          editing_task_id TEXT PRIMARY KEY NOT NULL REFERENCES editing_tasks(id) ON DELETE RESTRICT,
          project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          goal TEXT NOT NULL,
          active_subgoal TEXT NOT NULL DEFAULT '',
          status TEXT NOT NULL CHECK(status IN ('active', 'working', 'needs_clarification', 'needs_review')),
          current_stage TEXT NOT NULL CHECK(current_stage IN ('planning', 'storyboard', 'timeline', 'preview')),
          current_artifact_type TEXT,
          current_artifact_id TEXT,
          state_json TEXT NOT NULL,
          updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS task_state_snapshots_project_updated_idx
          ON task_state_snapshots(project_id, updated_at DESC);
        CREATE TABLE IF NOT EXISTS pending_task_routes (
          id TEXT PRIMARY KEY NOT NULL,
          project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          active_editing_task_id TEXT REFERENCES editing_tasks(id) ON DELETE RESTRICT,
          candidate_task_ids_json TEXT NOT NULL,
          original_request TEXT NOT NULL,
          question TEXT NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('pending', 'resolved', 'superseded')),
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          resolved_at INTEGER
        );
        CREATE UNIQUE INDEX IF NOT EXISTS pending_task_routes_one_active_idx
          ON pending_task_routes(project_id) WHERE status = 'pending';
        CREATE TABLE IF NOT EXISTS task_route_receipts (
          id TEXT PRIMARY KEY NOT NULL,
          project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          target_editing_task_id TEXT REFERENCES editing_tasks(id) ON DELETE RESTRICT,
          target_conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE RESTRICT,
          action TEXT NOT NULL CHECK(action IN ('continue_current', 'switch_existing', 'create_new')),
          request TEXT NOT NULL,
          pending_task_route_id TEXT REFERENCES pending_task_routes(id) ON DELETE RESTRICT,
          user_message_id TEXT,
          status TEXT NOT NULL CHECK(status IN ('issued', 'consumed')),
          created_at INTEGER NOT NULL,
          consumed_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS task_route_receipts_project_status_idx
          ON task_route_receipts(project_id, status, created_at DESC);
        ",
    ).map_err(|error| error.to_string())?;
    let snapshot_active_subgoal_column_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('task_state_snapshots') WHERE name = 'active_subgoal'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if snapshot_active_subgoal_column_count == 0 {
        connection
            .execute(
                "ALTER TABLE task_state_snapshots ADD COLUMN active_subgoal TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(|error| error.to_string())?;
    }
    let receipt_conversation_column_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('task_route_receipts') WHERE name = 'target_conversation_id'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if receipt_conversation_column_count == 0 {
        connection
            .execute(
                "ALTER TABLE task_route_receipts ADD COLUMN target_conversation_id TEXT REFERENCES conversations(id) ON DELETE RESTRICT",
                [],
            )
            .map_err(|error| error.to_string())?;
    }
    let receipt_message_column_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('task_route_receipts') WHERE name = 'user_message_id'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if receipt_message_column_count == 0 {
        connection
            .execute(
                "ALTER TABLE task_route_receipts ADD COLUMN user_message_id TEXT",
                [],
            )
            .map_err(|error| error.to_string())?;
    }
    for (table, column) in [
        ("conversations", "editing_task_id"),
        ("storyboard_versions", "editing_task_id"),
    ] {
        let column_count: i64 = connection
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"
                ),
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if column_count == 0 {
            connection
                .execute(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT"), [])
                .map_err(|error| error.to_string())?;
        }
    }
    let mut project_statement = connection
        .prepare(
            "
            SELECT project_id FROM conversations WHERE editing_task_id IS NULL
            UNION
            SELECT project_id FROM storyboard_versions WHERE editing_task_id IS NULL
            ",
        )
        .map_err(|error| error.to_string())?;
    let project_ids = project_statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for project_id in project_ids {
        let legacy_task_id = format!("legacy-{project_id}");
        connection.execute(
            "INSERT OR IGNORE INTO editing_tasks (id, project_id, title, brief, created_at, updated_at) VALUES (?1, ?2, '已有剪辑任务', '', ?3, ?3)",
            params![legacy_task_id, project_id, now_millis()],
        ).map_err(|error| error.to_string())?;
        connection.execute(
            "UPDATE conversations SET editing_task_id = ?1 WHERE project_id = ?2 AND editing_task_id IS NULL",
            params![legacy_task_id, project_id],
        ).map_err(|error| error.to_string())?;
        connection.execute(
            "UPDATE storyboard_versions SET editing_task_id = ?1 WHERE project_id = ?2 AND editing_task_id IS NULL",
            params![legacy_task_id, project_id],
        ).map_err(|error| error.to_string())?;
    }
    connection.execute(
        "UPDATE operation_logs SET editing_task_id = (SELECT storyboard_versions.editing_task_id FROM timeline_versions JOIN storyboard_versions ON storyboard_versions.id = timeline_versions.storyboard_version_id WHERE timeline_versions.id = operation_logs.entity_id) WHERE editing_task_id IS NULL AND entity_type = 'timeline_version'",
        [],
    ).map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![SCHEMA_VERSION, now_millis()],
        )
        .map_err(|error| error.to_string())?;
    connection.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS agent_tasks_project_editing_updated_idx ON agent_tasks(project_id, editing_task_id, updated_at DESC);
        CREATE INDEX IF NOT EXISTS operation_logs_project_editing_created_idx ON operation_logs(project_id, editing_task_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS operation_logs_agent_task_created_idx ON operation_logs(agent_task_id, created_at DESC);
        ",
    ).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_only_backfills_projects_with_unscoped_legacy_records() {
        let connection = Connection::open_in_memory().expect("open migration test database");
        migrate(&connection).expect("create current schema");
        connection
            .execute(
                "INSERT INTO projects (id, name, created_at, updated_at) VALUES ('new-project', 'New', 1, 1)",
                [],
            )
            .expect("insert new project");

        migrate(&connection).expect("reopen current schema");
        let fresh_task_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM editing_tasks WHERE project_id = 'new-project'",
                [],
                |row| row.get(0),
            )
            .expect("count fresh project tasks");
        assert_eq!(fresh_task_count, 0);

        connection
            .execute(
                "INSERT INTO conversations (id, project_id, title, summary, status, created_at, updated_at) VALUES ('legacy-conversation', 'new-project', 'Legacy', '', 'ready', 1, 1)",
                [],
            )
            .expect("insert unscoped legacy conversation");
        migrate(&connection).expect("backfill legacy conversation");
        let editing_task_id: String = connection
            .query_row(
                "SELECT editing_task_id FROM conversations WHERE id = 'legacy-conversation'",
                [],
                |row| row.get(0),
            )
            .expect("read backfilled task");
        assert_eq!(editing_task_id, "legacy-new-project");
    }

    #[test]
    fn migration_adds_scoped_agent_audit_columns() {
        let connection = Connection::open_in_memory().expect("open migration test database");
        migrate(&connection).expect("create current schema");
        for (table, column) in [
            ("agent_tasks", "editing_task_id"),
            ("operation_logs", "editing_task_id"),
            ("operation_logs", "conversation_id"),
            ("operation_logs", "agent_task_id"),
        ] {
            let count: i64 = connection
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("read audit column");
            assert_eq!(count, 1, "{table}.{column} should exist");
        }
    }

    #[test]
    fn migration_adds_payload_free_agent_run_steps() {
        let connection = Connection::open_in_memory().expect("open migration test database");
        migrate(&connection).expect("create current schema");

        let columns = connection
            .prepare("SELECT name FROM pragma_table_info('agent_run_steps') ORDER BY cid")
            .expect("prepare run-step columns")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("read run-step columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect run-step columns");

        assert!(columns.contains(&"agent_task_id".to_owned()));
        assert!(columns.contains(&"artifact_id".to_owned()));
        assert!(columns.contains(&"error_code".to_owned()));
        assert!(!columns.iter().any(|column| {
            matches!(
                column.as_str(),
                "input_json" | "result_json" | "model_response" | "media_evidence"
            )
        }));

        let diagnostics_table: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='agent_diagnostics'",
                [],
                |row| row.get(0),
            )
            .expect("read diagnostics table");
        assert_eq!(diagnostics_table, 1);

        let schema_version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("read schema version");
        assert_eq!(schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn migration_adds_structured_pending_clarifications() {
        let connection = Connection::open_in_memory().expect("open migration test database");
        migrate(&connection).expect("create current schema");

        let columns = connection
            .prepare("SELECT name FROM pragma_table_info('pending_clarifications') ORDER BY cid")
            .expect("prepare clarification columns")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("read clarification columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect clarification columns");
        assert!(columns.contains(&"question".to_owned()));
        assert!(columns.contains(&"source_agent_task_id".to_owned()));
        assert!(!columns.iter().any(|column| {
            matches!(
                column.as_str(),
                "context_json" | "model_response" | "media_evidence"
            )
        }));

        let active_index: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'pending_clarifications_one_active_idx'",
                [],
                |row| row.get(0),
            )
            .expect("read active clarification index");
        assert!(active_index.contains("WHERE status = 'pending'"));
    }

    #[test]
    fn migration_adds_task_routing_state_without_model_transcripts() {
        let connection = Connection::open_in_memory().expect("open migration test database");
        migrate(&connection).expect("create current schema");

        let snapshot_columns = connection
            .prepare("SELECT name FROM pragma_table_info('task_state_snapshots') ORDER BY cid")
            .expect("prepare task snapshot columns")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("read task snapshot columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect task snapshot columns");
        assert!(snapshot_columns.contains(&"current_artifact_id".to_owned()));
        assert!(snapshot_columns.contains(&"active_subgoal".to_owned()));
        assert!(snapshot_columns.contains(&"state_json".to_owned()));
        assert!(!snapshot_columns.iter().any(|column| {
            matches!(
                column.as_str(),
                "model_response" | "message_history" | "media_evidence"
            )
        }));

        let route_index: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'pending_task_routes_one_active_idx'",
                [],
                |row| row.get(0),
            )
            .expect("read pending route index");
        assert!(route_index.contains("WHERE status = 'pending'"));

        let receipt_table: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='task_route_receipts'",
                [],
                |row| row.get(0),
            )
            .expect("read receipt table");
        assert_eq!(receipt_table, 1);
        let receipt_columns = connection
            .prepare("SELECT name FROM pragma_table_info('task_route_receipts') ORDER BY cid")
            .expect("prepare receipt columns")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("read receipt columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect receipt columns");
        assert!(receipt_columns.contains(&"target_conversation_id".to_owned()));
        assert!(receipt_columns.contains(&"user_message_id".to_owned()));
    }

    #[test]
    fn migration_repairs_task_snapshots_created_before_active_subgoal() {
        let connection = Connection::open_in_memory().expect("open legacy migration database");
        connection
            .execute_batch(
                "CREATE TABLE task_state_snapshots (
                  editing_task_id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL,
                  goal TEXT NOT NULL, status TEXT NOT NULL, current_stage TEXT NOT NULL,
                  current_artifact_type TEXT, current_artifact_id TEXT,
                  state_json TEXT NOT NULL, updated_at INTEGER NOT NULL
                );",
            )
            .expect("create legacy task snapshot table");

        migrate(&connection).expect("repair legacy task snapshot table");

        let active_subgoal_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('task_state_snapshots') WHERE name = 'active_subgoal'",
                [],
                |row| row.get(0),
            )
            .expect("read repaired column");
        assert_eq!(active_subgoal_count, 1);
    }

    #[test]
    fn migration_separates_user_asset_metadata_from_analysis_evidence() {
        let connection = Connection::open_in_memory().expect("open migration test database");
        migrate(&connection).expect("run migration");
        for table in [
            "asset_user_metadata",
            "asset_tags",
            "asset_tag_assignments",
            "asset_collections",
            "asset_collection_items",
        ] {
            let exists: i64 = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                    params![table],
                    |row| row.get(0),
                )
                .expect("query table");
            assert_eq!(exists, 1, "missing {table}");
        }
        let schema_version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("read schema version");
        assert_eq!(schema_version, SCHEMA_VERSION);
    }
}
