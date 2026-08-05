use crate::oauth;
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 3;

fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // Console-based media tools must not create a visible window from the GUI app.
        command.creation_flags(0x0800_0000);
    }
    command
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreStatus {
    pub database_ready: bool,
    pub schema_version: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub project_id: String,
    pub editing_task_id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditingTask {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub brief: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditingSession {
    pub id: String,
    pub project_id: String,
    pub conversation_id: Option<String>,
    pub title: String,
    pub brief: String,
    pub summary: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub display_name: String,
    pub folder_name: Option<String>,
    pub relative_path: Option<String>,
    pub analysis_status: String,
    pub source_available: bool,
    pub duration_ms: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub fps: Option<f64>,
    pub has_audio: bool,
    pub thumbnail_path: Option<String>,
    pub keyframe_count: usize,
    pub scene_count: usize,
    pub ocr_text_count: usize,
    pub visual_tag_count: usize,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetEvidence {
    pub id: String,
    pub display_name: String,
    pub analysis_status: String,
    pub keyframes: Vec<KeyframeMetadata>,
    pub ocr_evidence: Vec<OcrEvidence>,
    pub visual_evidence: Vec<VisualEvidence>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardVersion {
    pub id: String,
    pub project_id: String,
    pub editing_task_id: String,
    pub version_number: i64,
    pub brief: String,
    pub title: String,
    pub summary: String,
    pub shots: Vec<StoryboardShot>,
    pub created_at: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineVersion {
    pub id: String,
    pub project_id: String,
    pub storyboard_version_id: String,
    pub version_number: i64,
    pub clips: Vec<TimelineClip>,
    pub quality_report: Option<PreviewQualityReport>,
    pub created_at: i64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewQualityReport {
    pub checks: Vec<PreviewQualityCheck>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewQualityCheck {
    pub category: String,
    pub severity: String,
    pub message: String,
    pub shot_indices: Vec<i64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineClip {
    pub shot_index: i64,
    pub asset_id: String,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub timeline_start_ms: i64,
    pub timeline_end_ms: i64,
    pub on_screen_text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimelineContent {
    clips: Vec<TimelineClip>,
    #[serde(default)]
    quality_report: Option<PreviewQualityReport>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    pub timeline_version_id: String,
    pub preview_path: String,
    pub quality_report: PreviewQualityReport,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JianyingDraftResult {
    pub draft_directory: String,
    pub draft_content_path: String,
    pub registration_status: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingJianyingRegistration {
    input_format_version: i64,
    operation: String,
    draft_root: String,
    draft_name: String,
    draft_registry_path: String,
    draft_directory: String,
    duration_ms: i64,
    timeline_version_id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JianyingRegistrationStatus {
    pub timeline_version_id: String,
    pub draft_name: String,
    pub status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestTimeline {
    pub timeline: TimelineVersion,
    pub preview: Option<PreviewResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentEditDecision {
    tool: String,
    reason: String,
    #[serde(default)]
    reply: String,
    #[serde(default)]
    task_brief: Option<String>,
    #[serde(default)]
    timeline_version_id: Option<String>,
    #[serde(default)]
    shot_index: Option<i64>,
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    source_start_ms: Option<i64>,
    #[serde(default)]
    source_end_ms: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEditResult {
    pub message: String,
    pub storyboard: Option<StoryboardVersion>,
    pub timeline: Option<TimelineVersion>,
    pub preview: Option<PreviewResult>,
    pub jianying_draft: Option<JianyingDraftResult>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardShot {
    pub order_index: i64,
    pub duration_ms: i64,
    pub purpose: String,
    pub on_screen_text: String,
    pub asset_id: String,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub reason: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoryboardContent {
    #[serde(default)]
    brief: String,
    title: String,
    summary: String,
    shots: Vec<StoryboardShot>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoryboardSource {
    asset_id: String,
    kind: String,
    duration_ms: Option<i64>,
    scene_segments: Vec<SceneSegment>,
    ocr_evidence: Vec<OcrEvidence>,
    visual_evidence: Vec<VisualEvidence>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TechnicalMetadata {
    duration_ms: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
    fps: Option<f64>,
    has_audio: bool,
    thumbnail_path: Option<String>,
    #[serde(default)]
    keyframes: Vec<KeyframeMetadata>,
    #[serde(default)]
    scene_segments: Vec<SceneSegment>,
    #[serde(default)]
    ocr_evidence: Vec<OcrEvidence>,
    #[serde(default)]
    visual_evidence: Vec<VisualEvidence>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyframeMetadata {
    time_ms: i64,
    image_path: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SceneSegment {
    start_ms: i64,
    end_ms: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrEvidence {
    time_ms: Option<i64>,
    text: String,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualEvidence {
    time_ms: Option<i64>,
    #[serde(default)]
    subjects: Vec<String>,
    scene: Option<String>,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    products: Vec<String>,
    #[serde(default)]
    quality_notes: Vec<String>,
}

#[derive(Deserialize)]
struct FfprobeOutput {
    format: Option<FfprobeFormat>,
    #[serde(default)]
    streams: Vec<FfprobeStream>,
}

#[derive(Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    r_frame_rate: Option<String>,
}

fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory.join("assembly-video-agent.sqlite3"))
}

fn open_connection(app: &AppHandle) -> Result<Connection, String> {
    let connection = Connection::open(database_path(app)?).map_err(|error| error.to_string())?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| error.to_string())?;
    migrate(&connection)?;
    Ok(connection)
}

fn migrate(connection: &Connection) -> Result<(), String> {
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
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS editing_tasks (
          id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
          title TEXT NOT NULL, brief TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS editing_tasks_project_updated_idx ON editing_tasks(project_id, updated_at DESC);
        ",
    ).map_err(|error| error.to_string())?;
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
    connection
        .execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![SCHEMA_VERSION, now_millis()],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub fn initialize_local_store(app: AppHandle) -> Result<StoreStatus, String> {
    let connection = open_connection(&app)?;
    connection
        .execute(
            "UPDATE conversations SET status = 'ready' WHERE status = 'working'",
            [],
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    resume_incomplete_analysis(&app)?;
    resume_pending_jianying_registrations(&app)?;
    Ok(StoreStatus {
        database_ready: true,
        schema_version: SCHEMA_VERSION,
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

fn asset_kind(path: &Path) -> String {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => "video",
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" => "image",
        "mp3" | "wav" | "aac" | "m4a" | "flac" | "ogg" => "audio",
        _ => "other",
    }
    .to_owned()
}

fn supported_media_file(path: &Path) -> bool {
    asset_kind(path) != "other"
}

fn collect_media_files(directory: &Path, sources: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        fs::read_dir(directory).map_err(|_| "The selected folder could not be read.".to_owned())?
    {
        let entry = entry.map_err(|_| "The selected folder could not be read.".to_owned())?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|_| "The selected folder could not be read.".to_owned())?;
        if file_type.is_dir() && !file_type.is_symlink() {
            collect_media_files(&path, sources)?;
        } else if file_type.is_file() && supported_media_file(&path) {
            sources.push(path);
        }
    }
    Ok(())
}

fn parse_frame_rate(value: Option<&str>) -> Option<f64> {
    let value = value?;
    let mut parts = value.split('/');
    let numerator = parts.next()?.parse::<f64>().ok()?;
    let denominator = parts
        .next()
        .and_then(|part| part.parse::<f64>().ok())
        .unwrap_or(1.0);
    (denominator > 0.0).then_some(numerator / denominator)
}

fn probe_media(source: &Path) -> Result<TechnicalMetadata, String> {
    let output = hidden_command("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,width,height,r_frame_rate",
            "-of",
            "json",
        ])
        .arg(source)
        .output()
        .map_err(|_| "FFprobe is not available on this computer.".to_owned())?;
    if !output.status.success() {
        return Err("FFprobe could not read this media file.".to_owned());
    }
    let probe: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|_| "FFprobe returned invalid media metadata.".to_owned())?;
    let video_stream = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"));
    Ok(TechnicalMetadata {
        duration_ms: probe
            .format
            .and_then(|format| format.duration)
            .and_then(|duration| duration.parse::<f64>().ok())
            .map(|duration| (duration * 1000.0).round() as i64),
        width: video_stream.and_then(|stream| stream.width),
        height: video_stream.and_then(|stream| stream.height),
        fps: parse_frame_rate(video_stream.and_then(|stream| stream.r_frame_rate.as_deref())),
        has_audio: probe
            .streams
            .iter()
            .any(|stream| stream.codec_type.as_deref() == Some("audio")),
        thumbnail_path: None,
        keyframes: Vec::new(),
        scene_segments: Vec::new(),
        ocr_evidence: Vec::new(),
        visual_evidence: Vec::new(),
    })
}

fn derived_directory(app: &AppHandle, asset_id: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("derived")
        .join(asset_id);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

fn thumbnail_destination(app: &AppHandle, asset_id: &str) -> Result<PathBuf, String> {
    Ok(derived_directory(app, asset_id)?.join("thumbnail.jpg"))
}

fn generate_thumbnail(
    app: &AppHandle,
    asset_id: &str,
    source: &Path,
    kind: &str,
) -> Option<String> {
    if !matches!(kind, "video" | "image") {
        return None;
    }
    let destination = thumbnail_destination(app, asset_id).ok()?;
    let mut command = hidden_command("ffmpeg");
    command.args(["-y", "-hide_banner", "-loglevel", "error"]);
    if kind == "video" {
        command.args(["-ss", "0.5"]);
    }
    let status = command
        .arg("-i")
        .arg(source)
        .args(["-frames:v", "1", "-vf", "scale=320:-2"])
        .arg(&destination)
        .status()
        .ok()?;
    (status.success() && destination.is_file()).then(|| destination.to_string_lossy().into_owned())
}

fn extract_scene_times(output: &[u8]) -> Vec<f64> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| line.split("pts_time:").nth(1))
        .filter_map(|value| value.split_whitespace().next())
        .filter_map(|value| value.parse::<f64>().ok())
        .collect()
}

fn generate_video_keyframes(
    app: &AppHandle,
    asset_id: &str,
    source: &Path,
    duration_ms: Option<i64>,
) -> (Vec<KeyframeMetadata>, Vec<SceneSegment>) {
    let Ok(directory) = derived_directory(app, asset_id) else {
        return (Vec::new(), Vec::new());
    };
    let pattern = directory.join("keyframe_%03d.jpg");
    let output = hidden_command("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "info", "-i"])
        .arg(source)
        .args([
            "-vf",
            "select=gt(scene\\,0.30),showinfo,scale=320:-2",
            "-frames:v",
            "8",
            "-fps_mode",
            "vfr",
        ])
        .arg(&pattern)
        .output();
    let mut times = output
        .as_ref()
        .ok()
        .filter(|result| result.status.success())
        .map(|result| extract_scene_times(&result.stderr))
        .unwrap_or_default();
    if times.is_empty() {
        let duration_seconds = duration_ms.unwrap_or(0) as f64 / 1000.0;
        times = [
            0.0,
            duration_seconds * 0.5,
            (duration_seconds - 0.1).max(0.0),
        ]
        .into_iter()
        .filter(|time| duration_seconds > 0.0 || *time == 0.0)
        .collect();
        for (index, time) in times.iter().enumerate() {
            let destination = directory.join(format!("keyframe_{:03}.jpg", index + 1));
            let _ = hidden_command("ffmpeg")
                .args([
                    "-y",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-ss",
                    &format!("{time:.3}"),
                    "-i",
                ])
                .arg(source)
                .args(["-frames:v", "1", "-vf", "scale=320:-2"])
                .arg(destination)
                .status();
        }
    }
    let keyframes = times
        .into_iter()
        .enumerate()
        .filter_map(|(index, time)| {
            let image_path = directory.join(format!("keyframe_{:03}.jpg", index + 1));
            image_path.is_file().then(|| KeyframeMetadata {
                time_ms: (time * 1000.0).round() as i64,
                image_path: image_path.to_string_lossy().into_owned(),
            })
        })
        .collect::<Vec<_>>();
    let mut boundaries = keyframes
        .iter()
        .map(|frame| frame.time_ms)
        .collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();
    if boundaries.first().copied() != Some(0) {
        boundaries.insert(0, 0);
    }
    if let Some(duration_ms) = duration_ms.filter(|duration| *duration > 0) {
        if boundaries.last().copied() != Some(duration_ms) {
            boundaries.push(duration_ms);
        }
    }
    let scenes = boundaries
        .windows(2)
        .filter(|pair| pair[1] > pair[0])
        .map(|pair| SceneSegment {
            start_ms: pair[0],
            end_ms: pair[1],
        })
        .collect();
    (keyframes, scenes)
}

fn tesseract_program() -> PathBuf {
    if let Some(configured) = env::var_os("TESSERACT_PATH") {
        return PathBuf::from(configured);
    }
    if let Some(program_files) = env::var_os("ProgramFiles") {
        let installed = PathBuf::from(program_files)
            .join("Tesseract-OCR")
            .join("tesseract.exe");
        if installed.is_file() {
            return installed;
        }
    }
    PathBuf::from("tesseract")
}

fn extract_ocr(image_path: &Path, time_ms: Option<i64>) -> Option<OcrEvidence> {
    let output = hidden_command(tesseract_program())
        .arg(image_path)
        .arg("stdout")
        .args(["-l", "eng", "--psm", "6"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(OcrEvidence { time_ms, text })
}

fn extract_ocr_evidence(
    kind: &str,
    source: &Path,
    keyframes: &[KeyframeMetadata],
) -> Vec<OcrEvidence> {
    if kind == "image" {
        return extract_ocr(source, None).into_iter().collect();
    }
    if kind == "video" {
        return keyframes
            .iter()
            .filter_map(|frame| extract_ocr(Path::new(&frame.image_path), Some(frame.time_ms)))
            .collect();
    }
    Vec::new()
}

fn find_json_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text)
            if text.trim_start().starts_with('{')
                && serde_json::from_str::<serde_json::Value>(text).is_ok() =>
        {
            Some(text.to_owned())
        }
        serde_json::Value::Array(items) => items.iter().find_map(find_json_text),
        serde_json::Value::Object(entries) => entries.values().find_map(find_json_text),
        _ => None,
    }
}

fn response_json_text(body: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(text) = find_json_text(&value) {
            return Some(text);
        }
    }
    let mut delta = String::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if let Some(text) = find_json_text(&event) {
            return Some(text);
        }
        if let Some(part) = event.get("delta").and_then(|value| value.as_str()) {
            delta.push_str(part);
        }
    }
    serde_json::from_str::<serde_json::Value>(&delta)
        .ok()
        .and_then(|value| find_json_text(&value).or(Some(delta)))
}

fn analyze_visual_frame(
    access: &oauth::AuthorizedOAuth,
    image_path: &Path,
    time_ms: Option<i64>,
) -> Option<VisualEvidence> {
    let image = fs::read(image_path).ok()?;
    let data_url = format!("data:image/jpeg;base64,{}", STANDARD.encode(image));
    let request = serde_json::json!({
        "model": "gpt-5.4",
        "store": false,
        "stream": true,
        "input": [{
            "role": "user",
            "content": [
                { "type": "input_text", "text": "Analyze only visible evidence in this video-editing frame. Return JSON with subjects (array), scene (string or null), actions (array), products (array), and qualityNotes (array). Do not infer facts not visible." },
                { "type": "input_image", "image_url": data_url }
            ]
        }],
        "text": { "format": { "type": "json_object" } }
    });
    let mut response = ureq::post("https://chatgpt.com/backend-api/codex/responses")
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", access.access_token))
        .set("originator", "opencode");
    if let Some(account_id) = &access.account_id {
        response = response.set("ChatGPT-Account-Id", account_id);
    }
    let body = response
        .send_string(&request.to_string())
        .ok()?
        .into_string()
        .ok()?;
    let text = response_json_text(&body)?;
    let mut evidence: VisualEvidence = serde_json::from_str(&text).ok()?;
    evidence.time_ms = time_ms;
    Some(evidence)
}

fn extract_visual_evidence(kind: &str, metadata: &TechnicalMetadata) -> Vec<VisualEvidence> {
    let Ok(access) = oauth::experimental_access() else {
        return Vec::new();
    };
    let frames = if kind == "video" {
        metadata
            .keyframes
            .iter()
            .take(3)
            .map(|frame| (Path::new(&frame.image_path), Some(frame.time_ms)))
            .collect::<Vec<_>>()
    } else if kind == "image" {
        metadata
            .thumbnail_path
            .as_deref()
            .map(|path| vec![(Path::new(path), None)])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    frames
        .into_iter()
        .filter_map(|(path, time_ms)| analyze_visual_frame(&access, path, time_ms))
        .collect()
}

fn update_analysis_status(
    app: &AppHandle,
    asset_id: &str,
    task_id: &str,
    status: &str,
    metadata: Option<&TechnicalMetadata>,
    error_message: Option<&str>,
) -> Result<(), String> {
    let timestamp = now_millis();
    let task_status = match status {
        "analyzing" => "running",
        "ready" => "completed",
        "failed" => "failed",
        _ => status,
    };
    let metadata_json = metadata
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| error.to_string())?;
    let connection = open_connection(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    if let Some(metadata_json) = metadata_json {
        transaction.execute(
            "UPDATE assets SET analysis_status = ?1, metadata_json = ?2, updated_at = ?3 WHERE id = ?4",
            params![status, metadata_json, timestamp, asset_id],
        ).map_err(|error| error.to_string())?;
    } else {
        transaction
            .execute(
                "UPDATE assets SET analysis_status = ?1, updated_at = ?2 WHERE id = ?3",
                params![status, timestamp, asset_id],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.execute(
        "UPDATE agent_tasks SET status = ?1, result_json = ?2, error_message = ?3, updated_at = ?4 WHERE id = ?5",
        params![task_status, metadata.map(serde_json::to_string).transpose().map_err(|error| error.to_string())?, error_message, timestamp, task_id],
    ).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn run_technical_analysis(app: AppHandle, asset_id: String, task_id: String) {
    log::info!("Starting local media analysis for asset {asset_id}.");
    let source = (|| -> Result<(PathBuf, String), String> {
        let connection = open_connection(&app)?;
        let (source_reference, kind): (String, String) = connection
            .query_row(
                "SELECT source_reference, kind FROM assets WHERE id = ?1",
                params![asset_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "The media asset is no longer available.".to_owned())?;
        Ok((PathBuf::from(source_reference), kind))
    })();
    let result = source.and_then(|(source, kind)| {
        update_analysis_status(&app, &asset_id, &task_id, "analyzing", None, None)?;
        let mut metadata = probe_media(&source)?;
        metadata.thumbnail_path = generate_thumbnail(&app, &asset_id, &source, &kind);
        if kind == "video" {
            (metadata.keyframes, metadata.scene_segments) =
                generate_video_keyframes(&app, &asset_id, &source, metadata.duration_ms);
        }
        metadata.ocr_evidence = extract_ocr_evidence(&kind, &source, &metadata.keyframes);
        metadata.visual_evidence = extract_visual_evidence(&kind, &metadata);
        Ok(metadata)
    });
    match result {
        Ok(metadata) => {
            let _ =
                update_analysis_status(&app, &asset_id, &task_id, "ready", Some(&metadata), None);
            log::info!("Completed local media analysis for asset {asset_id}.");
        }
        Err(error) => {
            log::warn!("Local media analysis failed for asset {asset_id}: {error}");
            let _ = update_analysis_status(&app, &asset_id, &task_id, "failed", None, Some(&error));
        }
    }
}

fn spawn_technical_analysis_tasks(app: AppHandle, tasks: Vec<(String, String)>) {
    if tasks.is_empty() {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        for (asset_id, task_id) in tasks {
            run_technical_analysis(app.clone(), asset_id, task_id);
        }
    });
}

fn resume_incomplete_analysis(app: &AppHandle) -> Result<(), String> {
    static RECOVERY_STARTED: AtomicBool = AtomicBool::new(false);
    if RECOVERY_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }

    let result = (|| {
        let connection = open_connection(app)?;
        let mut statement = connection
            .prepare(
                "
                SELECT id, input_json
                FROM agent_tasks
                WHERE tool_name = 'analyze_asset' AND status IN ('queued', 'running')
                ORDER BY created_at ASC
                ",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        drop(statement);

        let mut asset_ids = HashSet::new();
        let mut tasks = Vec::new();
        for (task_id, input_json) in rows {
            let asset_id = serde_json::from_str::<serde_json::Value>(&input_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("assetId")
                        .and_then(|asset_id| asset_id.as_str())
                        .map(str::to_owned)
                });
            let Some(asset_id) = asset_id else {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'failed', error_message = 'Stored analysis input is invalid.', updated_at = ?1 WHERE id = ?2",
                        params![now_millis(), task_id],
                    )
                    .map_err(|error| error.to_string())?;
                continue;
            };
            if !asset_ids.insert(asset_id.clone()) {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'cancelled', error_message = 'Superseded duplicate analysis task.', updated_at = ?1 WHERE id = ?2",
                        params![now_millis(), task_id],
                    )
                    .map_err(|error| error.to_string())?;
                continue;
            }
            tasks.push((asset_id, task_id));
        }
        spawn_technical_analysis_tasks(app.clone(), tasks);
        Ok(())
    })();

    if result.is_err() {
        RECOVERY_STARTED.store(false, Ordering::Release);
    }
    result
}

fn enqueue_technical_analysis(app: &AppHandle, assets: &[Asset]) -> Result<(), String> {
    let timestamp = now_millis();
    let connection = open_connection(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let tasks = assets
        .iter()
        .map(|asset| {
            (
                asset.id.clone(),
                asset.project_id.clone(),
                Uuid::new_v4().to_string(),
            )
        })
        .collect::<Vec<_>>();
    for (asset_id, project_id, task_id) in &tasks {
        transaction.execute(
            "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset', 'queued', ?3, ?4, ?5)",
            params![task_id, project_id, serde_json::json!({ "assetId": asset_id }).to_string(), timestamp, timestamp],
        ).map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    spawn_technical_analysis_tasks(
        app.clone(),
        tasks
            .into_iter()
            .map(|(asset_id, _, task_id)| (asset_id, task_id))
            .collect(),
    );
    Ok(())
}

fn store_assets(
    app: &AppHandle,
    project_id: &str,
    sources: Vec<PathBuf>,
    folder_reference: Option<&Path>,
) -> Result<Vec<Asset>, String> {
    if sources.iter().any(|source| !source.is_file()) {
        return Err("One or more selected media files are no longer available.".to_owned());
    }

    let timestamp = now_millis();
    let connection = open_connection(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut imported = Vec::with_capacity(sources.len());
    for source in sources {
        let display_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "A selected file name is invalid.".to_owned())?
            .to_owned();
        let asset = Asset {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.to_owned(),
            kind: asset_kind(&source),
            display_name,
            folder_name: None,
            relative_path: None,
            analysis_status: "queued".to_owned(),
            source_available: true,
            duration_ms: None,
            width: None,
            height: None,
            fps: None,
            has_audio: false,
            thumbnail_path: None,
            keyframe_count: 0,
            scene_count: 0,
            ocr_text_count: 0,
            visual_tag_count: 0,
            created_at: timestamp,
            updated_at: timestamp,
        };
        transaction.execute(
            "INSERT INTO assets (id, project_id, kind, display_name, source_reference, folder_reference, analysis_status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![asset.id, asset.project_id, asset.kind, asset.display_name, source.to_string_lossy(), folder_reference.map(|path| path.to_string_lossy().into_owned()), asset.analysis_status, asset.created_at, asset.updated_at],
        ).map_err(|error| error.to_string())?;
        imported.push(asset);
    }
    transaction.commit().map_err(|error| error.to_string())?;
    enqueue_technical_analysis(app, &imported)?;
    Ok(imported)
}

#[tauri::command]
pub fn import_assets(
    app: AppHandle,
    project_id: String,
    source_references: Vec<String>,
) -> Result<Vec<Asset>, String> {
    let sources = source_references.into_iter().map(PathBuf::from).collect();
    store_assets(&app, &project_id, sources, None)
}

#[tauri::command]
pub fn import_asset_folder(
    app: AppHandle,
    project_id: String,
    source_directory: String,
) -> Result<Vec<Asset>, String> {
    let directory = PathBuf::from(source_directory);
    if !directory.is_dir() {
        return Err("The selected folder is no longer available.".to_owned());
    }
    let mut sources = Vec::new();
    collect_media_files(&directory, &mut sources)?;
    store_assets(&app, &project_id, sources, Some(&directory))
}

fn asset_folder_metadata(
    source_reference: &str,
    folder_reference: Option<String>,
) -> (Option<String>, Option<String>) {
    let Some(folder_reference) = folder_reference else {
        return (None, None);
    };
    let folder = Path::new(&folder_reference);
    let folder_name = folder
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    let relative_path = Path::new(source_reference)
        .strip_prefix(folder)
        .ok()
        .and_then(|path| path.to_str())
        .map(str::to_owned);
    (folder_name, relative_path)
}

#[tauri::command]
pub fn list_assets(app: AppHandle, project_id: String) -> Result<Vec<Asset>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection.prepare(
        "SELECT id, project_id, kind, display_name, source_reference, folder_reference, analysis_status, metadata_json, created_at, updated_at FROM assets WHERE project_id = ?1 ORDER BY created_at DESC",
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            let source_reference: String = row.get(4)?;
            let folder_reference: Option<String> = row.get(5)?;
            let (folder_name, relative_path) =
                asset_folder_metadata(&source_reference, folder_reference);
            let metadata: TechnicalMetadata =
                serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default();
            Ok(Asset {
                id: row.get(0)?,
                project_id: row.get(1)?,
                kind: row.get(2)?,
                display_name: row.get(3)?,
                folder_name,
                relative_path,
                analysis_status: row.get(6)?,
                source_available: Path::new(&source_reference).is_file(),
                duration_ms: metadata.duration_ms,
                width: metadata.width,
                height: metadata.height,
                fps: metadata.fps,
                has_audio: metadata.has_audio,
                thumbnail_path: metadata.thumbnail_path,
                keyframe_count: metadata.keyframes.len(),
                scene_count: metadata.scene_segments.len(),
                ocr_text_count: metadata.ocr_evidence.len(),
                visual_tag_count: metadata
                    .visual_evidence
                    .iter()
                    .map(|evidence| {
                        evidence.subjects.len()
                            + evidence.actions.len()
                            + evidence.products.len()
                            + usize::from(evidence.scene.is_some())
                    })
                    .sum(),
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_asset_evidence(app: AppHandle, asset_id: String) -> Result<AssetEvidence, String> {
    let connection = open_connection(&app)?;
    connection
        .query_row(
            "SELECT id, display_name, analysis_status, metadata_json FROM assets WHERE id = ?1",
            params![asset_id],
            |row| {
                let metadata: TechnicalMetadata =
                    serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default();
                Ok(AssetEvidence {
                    id: row.get(0)?,
                    display_name: row.get(1)?,
                    analysis_status: row.get(2)?,
                    keyframes: metadata.keyframes,
                    ocr_evidence: metadata.ocr_evidence,
                    visual_evidence: metadata.visual_evidence,
                })
            },
        )
        .map_err(|_| "Asset evidence is unavailable.".to_owned())
}

fn storyboard_sources(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<StoryboardSource>, String> {
    let mut statement = connection.prepare(
        "SELECT id, kind, metadata_json, source_reference FROM assets WHERE project_id = ?1 AND analysis_status = 'ready'",
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            let metadata: TechnicalMetadata =
                serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_default();
            Ok((
                StoryboardSource {
                    asset_id: row.get(0)?,
                    kind: row.get(1)?,
                    duration_ms: metadata.duration_ms,
                    scene_segments: metadata.scene_segments,
                    ocr_evidence: metadata.ocr_evidence,
                    visual_evidence: metadata.visual_evidence,
                },
                Path::new(&row.get::<_, String>(3)?).is_file(),
            ))
        })
        .map_err(|error| error.to_string())?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter_map(|(source, available)| available.then_some(source))
        .collect())
}

fn request_storyboard(
    access: &oauth::AuthorizedOAuth,
    brief: &str,
    sources: &[StoryboardSource],
) -> Result<StoryboardContent, String> {
    let evidence = serde_json::to_string(sources)
        .map_err(|_| "Could not prepare media evidence.".to_owned())?;
    let prompt = format!("Create an editable storyboard for this brief: {brief}\nUse ONLY the supplied media evidence JSON below. Return JSON with title, summary, and shots. Each shot must contain orderIndex, durationMs, purpose, onScreenText, assetId, sourceStartMs, sourceEndMs, and reason. For video, source times must be inside the provided duration and preferably align with sceneSegments. For images, sourceStartMs and sourceEndMs must both be 0. Do not use file names, unknown asset IDs, or unverified claims. Evidence: {evidence}");
    let request = serde_json::json!({
        "model": "gpt-5.4",
        "store": false,
        "stream": true,
        "input": [{ "role": "user", "content": [{ "type": "input_text", "text": prompt }] }],
        "text": { "format": { "type": "json_object" } }
    });
    let mut response = ureq::post("https://chatgpt.com/backend-api/codex/responses")
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", access.access_token))
        .set("originator", "opencode");
    if let Some(account_id) = &access.account_id {
        response = response.set("ChatGPT-Account-Id", account_id);
    }
    let body = response
        .send_string(&request.to_string())
        .map_err(|_| "Experimental storyboard request failed.".to_owned())?
        .into_string()
        .map_err(|_| "Experimental storyboard response was empty.".to_owned())?;
    let text = response_json_text(&body)
        .ok_or_else(|| "Experimental storyboard response did not contain JSON.".to_owned())?;
    serde_json::from_str(&text)
        .map_err(|_| "Experimental storyboard JSON did not match the required schema.".to_owned())
}

fn validate_storyboard(
    content: &StoryboardContent,
    sources: &[StoryboardSource],
) -> Result<(), String> {
    if content.shots.is_empty() || content.shots.len() > 12 {
        return Err("Storyboard must contain between 1 and 12 shots.".to_owned());
    }
    let total_duration = content
        .shots
        .iter()
        .map(|shot| shot.duration_ms)
        .sum::<i64>();
    if !(10_000..=45_000).contains(&total_duration) {
        return Err("Storyboard duration must be between 10 and 45 seconds.".to_owned());
    }
    for (index, shot) in content.shots.iter().enumerate() {
        if shot.order_index != index as i64 + 1
            || shot.duration_ms <= 0
            || shot.purpose.trim().is_empty()
            || shot.reason.trim().is_empty()
        {
            return Err("Storyboard shot fields are invalid.".to_owned());
        }
        let source = sources
            .iter()
            .find(|source| source.asset_id == shot.asset_id)
            .ok_or_else(|| "Storyboard referenced an unavailable asset.".to_owned())?;
        if source.kind == "video" {
            let duration = source.duration_ms.ok_or_else(|| {
                "Storyboard referenced video without a verified duration.".to_owned()
            })?;
            if shot.source_start_ms < 0
                || shot.source_end_ms <= shot.source_start_ms
                || shot.source_end_ms > duration
            {
                return Err("Storyboard referenced an invalid video time range.".to_owned());
            }
            if shot.duration_ms > shot.source_end_ms - shot.source_start_ms {
                return Err(
                    "Storyboard shot duration exceeds its verified video source range.".to_owned(),
                );
            }
        } else if source.kind != "image" || shot.source_start_ms != 0 || shot.source_end_ms != 0 {
            return Err("Storyboard image references must use a zero source range.".to_owned());
        }
    }
    Ok(())
}

#[tauri::command]
pub fn generate_storyboard(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    brief: String,
) -> Result<StoryboardVersion, String> {
    log::info!("Starting AI storyboard generation.");
    let brief = brief.trim();
    if brief.is_empty() {
        return Err("Storyboard brief cannot be empty.".to_owned());
    }
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
    let sources = storyboard_sources(&connection, &project_id)?;
    if sources.is_empty() {
        return Err("No analyzed media is available for storyboard generation.".to_owned());
    }
    let access = oauth::experimental_access().map_err(|error| {
        log::warn!("AI storyboard generation could not access the configured provider: {error}");
        error
    })?;
    let content = request_storyboard(&access, brief, &sources).map_err(|error| {
        log::warn!("AI storyboard request failed: {error}");
        error
    })?;
    validate_storyboard(&content, &sources).map_err(|error| {
        log::warn!("AI storyboard validation failed: {error}");
        error
    })?;
    let version_number = connection.query_row(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM storyboard_versions WHERE project_id = ?1",
        params![project_id], |row| row.get::<_, i64>(0),
    ).map_err(|error| error.to_string())?;
    let version = StoryboardVersion {
        id: Uuid::new_v4().to_string(),
        project_id,
        editing_task_id: editing_task_id.clone(),
        version_number,
        brief: brief.to_owned(),
        title: content.title,
        summary: content.summary,
        shots: content.shots,
        created_at: now_millis(),
    };
    connection.execute(
        "INSERT INTO storyboard_versions (id, project_id, editing_task_id, version_number, status, content_json, created_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
        params![version.id, version.project_id, version.editing_task_id, version.version_number, serde_json::to_string(&StoryboardContent { brief: version.brief.clone(), title: version.title.clone(), summary: version.summary.clone(), shots: version.shots.clone() }).map_err(|error| error.to_string())?, version.created_at],
    ).map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE editing_tasks SET brief = ?1, title = CASE WHEN title IN ('新的剪辑任务', '新的剪辑会话') THEN substr(?1, 1, 28) ELSE title END, updated_at = ?2 WHERE id = ?3",
            params![brief, now_millis(), editing_task_id],
        )
        .map_err(|error| error.to_string())?;
    log::info!("Completed AI storyboard generation.");
    Ok(version)
}

#[tauri::command]
pub fn get_latest_storyboard(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
) -> Result<Option<StoryboardVersion>, String> {
    let connection = open_connection(&app)?;
    connection.query_row(
        "SELECT id, version_number, content_json, created_at FROM storyboard_versions WHERE project_id = ?1 AND editing_task_id = ?2 ORDER BY version_number DESC LIMIT 1",
        params![project_id, editing_task_id],
        |row| {
            let content: StoryboardContent = serde_json::from_str(&row.get::<_, String>(2)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(StoryboardVersion {
                id: row.get(0)?,
                project_id: project_id.clone(),
                editing_task_id: editing_task_id.clone(),
                version_number: row.get(1)?,
                brief: content.brief,
                title: content.title,
                summary: content.summary,
                shots: content.shots,
                created_at: row.get(3)?,
            })
        },
    ).optional().map_err(|_| "Storyboard version could not be read.".to_owned())
}

fn load_storyboard_version(
    connection: &Connection,
    storyboard_version_id: &str,
) -> Result<StoryboardVersion, String> {
    connection.query_row(
        "SELECT id, project_id, editing_task_id, version_number, content_json, created_at FROM storyboard_versions WHERE id = ?1",
        params![storyboard_version_id],
        |row| {
            let content: StoryboardContent = serde_json::from_str(&row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(StoryboardVersion {
                id: row.get(0)?, project_id: row.get(1)?, editing_task_id: row.get(2)?, version_number: row.get(3)?, brief: content.brief,
                title: content.title, summary: content.summary, shots: content.shots, created_at: row.get(5)?,
            })
        },
    ).map_err(|_| "Storyboard version could not be read.".to_owned())
}

#[tauri::command]
pub fn create_timeline_draft(
    app: AppHandle,
    project_id: String,
    storyboard_version_id: String,
) -> Result<TimelineVersion, String> {
    let connection = open_connection(&app)?;
    let storyboard = load_storyboard_version(&connection, &storyboard_version_id)?;
    if storyboard.project_id != project_id {
        return Err("Storyboard does not belong to this project.".to_owned());
    }
    let mut cursor = 0_i64;
    let clips = storyboard
        .shots
        .iter()
        .map(|shot| {
            let end = cursor + shot.duration_ms;
            let clip = TimelineClip {
                shot_index: shot.order_index,
                asset_id: shot.asset_id.clone(),
                source_start_ms: shot.source_start_ms,
                source_end_ms: shot.source_end_ms,
                timeline_start_ms: cursor,
                timeline_end_ms: end,
                on_screen_text: shot.on_screen_text.clone(),
            };
            cursor = end;
            clip
        })
        .collect::<Vec<_>>();
    let version_number = connection.query_row(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM timeline_versions WHERE project_id = ?1",
        params![project_id], |row| row.get::<_, i64>(0),
    ).map_err(|error| error.to_string())?;
    let version = TimelineVersion {
        id: Uuid::new_v4().to_string(),
        project_id,
        storyboard_version_id,
        version_number,
        clips,
        quality_report: None,
        created_at: now_millis(),
    };
    connection.execute(
        "INSERT INTO timeline_versions (id, project_id, storyboard_version_id, version_number, status, content_json, created_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
        params![version.id, version.project_id, version.storyboard_version_id, version.version_number, serde_json::to_string(&TimelineContent { clips: version.clips.clone(), quality_report: None }).map_err(|error| error.to_string())?, version.created_at],
    ).map_err(|error| error.to_string())?;
    Ok(version)
}

fn create_replaced_timeline_version(
    connection: &Connection,
    project_id: &str,
    timeline: &TimelineVersion,
    shot_index: i64,
    asset_id: String,
    source_start_ms: i64,
    source_end_ms: i64,
) -> Result<TimelineVersion, String> {
    if timeline.project_id != project_id {
        return Err("Timeline does not belong to this project.".to_owned());
    }
    let original = timeline
        .clips
        .iter()
        .find(|clip| clip.shot_index == shot_index)
        .ok_or_else(|| "Requested timeline shot does not exist.".to_owned())?;
    let clip_duration = original.timeline_end_ms - original.timeline_start_ms;
    let (kind, metadata_json): (String, String) = connection
        .query_row(
            "SELECT kind, metadata_json FROM assets WHERE id = ?1 AND project_id = ?2 AND analysis_status = 'ready'",
            params![asset_id, project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| "Replacement asset is unavailable or has not finished analysis.".to_owned())?;
    let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
    if kind == "video" {
        let duration = metadata
            .duration_ms
            .ok_or_else(|| "Replacement video has no verified duration.".to_owned())?;
        if source_start_ms < 0
            || source_end_ms <= source_start_ms
            || source_end_ms > duration
            || source_end_ms - source_start_ms != clip_duration
        {
            return Err(
                "Replacement video range must be verified and match the existing shot duration."
                    .to_owned(),
            );
        }
    } else if kind == "image" {
        if source_start_ms != 0 || source_end_ms != 0 {
            return Err("Replacement images must use a zero source range.".to_owned());
        }
    } else {
        return Err("Replacement asset must be a video or image.".to_owned());
    }
    let clips = timeline
        .clips
        .iter()
        .cloned()
        .map(|mut clip| {
            if clip.shot_index == shot_index {
                clip.asset_id = asset_id.clone();
                clip.source_start_ms = source_start_ms;
                clip.source_end_ms = source_end_ms;
            }
            clip
        })
        .collect::<Vec<_>>();
    let version_number = connection
        .query_row(
            "SELECT COALESCE(MAX(version_number), 0) + 1 FROM timeline_versions WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    let version = TimelineVersion {
        id: Uuid::new_v4().to_string(),
        project_id: project_id.to_owned(),
        storyboard_version_id: timeline.storyboard_version_id.clone(),
        version_number,
        clips,
        quality_report: None,
        created_at: now_millis(),
    };
    let content_json = serde_json::to_string(&TimelineContent {
        clips: version.clips.clone(),
        quality_report: None,
    })
    .map_err(|error| error.to_string())?;
    let replacement_clip = version
        .clips
        .iter()
        .find(|clip| clip.shot_index == shot_index)
        .ok_or_else(|| "Replacement timeline shot could not be saved.".to_owned())?;
    connection
        .execute(
            "INSERT INTO timeline_versions (id, project_id, storyboard_version_id, version_number, status, content_json, created_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
            params![version.id, version.project_id, version.storyboard_version_id, version.version_number, content_json, version.created_at],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, before_json, after_json, created_at) VALUES (?1, ?2, 'agent', 'replace_timeline_clip', 'timeline_version', ?3, ?4, ?5, ?6)",
            params![Uuid::new_v4().to_string(), project_id, version.id, serde_json::to_string(original).map_err(|error| error.to_string())?, serde_json::to_string(replacement_clip).map_err(|error| error.to_string())?, now_millis()],
        )
        .map_err(|error| error.to_string())?;
    Ok(version)
}

fn load_timeline_version(
    connection: &Connection,
    timeline_version_id: &str,
) -> Result<TimelineVersion, String> {
    connection.query_row(
        "SELECT id, project_id, storyboard_version_id, version_number, content_json, created_at FROM timeline_versions WHERE id = ?1",
        params![timeline_version_id],
        |row| {
            let content: TimelineContent = serde_json::from_str(&row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(TimelineVersion {
                id: row.get(0)?, project_id: row.get(1)?, storyboard_version_id: row.get(2)?, version_number: row.get(3)?, clips: content.clips, quality_report: content.quality_report, created_at: row.get(5)?,
            })
        },
    ).map_err(|_| "Timeline version could not be read.".to_owned())
}

fn timeline_candidates_for_storyboard(
    connection: &Connection,
    project_id: &str,
    storyboard_version_id: &str,
) -> Result<Vec<TimelineVersion>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, project_id, storyboard_version_id, version_number, content_json, created_at FROM timeline_versions WHERE project_id = ?1 AND storyboard_version_id = ?2 ORDER BY version_number DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id, storyboard_version_id], |row| {
            let content: TimelineContent = serde_json::from_str(&row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(TimelineVersion {
                id: row.get(0)?,
                project_id: row.get(1)?,
                storyboard_version_id: row.get(2)?,
                version_number: row.get(3)?,
                clips: content.clips,
                quality_report: content.quality_report,
                created_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn select_timeline_candidate(
    timelines: &[TimelineVersion],
    decision_timeline_id: Option<&str>,
    requested_timeline_id: Option<&str>,
) -> Option<TimelineVersion> {
    decision_timeline_id
        .and_then(|timeline_id| {
            timelines
                .iter()
                .find(|timeline| timeline.id == timeline_id)
                .cloned()
        })
        .or_else(|| {
            requested_timeline_id.and_then(|timeline_id| {
                timelines
                    .iter()
                    .find(|timeline| timeline.id == timeline_id)
                    .cloned()
            })
        })
        .or_else(|| (timelines.len() == 1).then(|| timelines[0].clone()))
}

#[tauri::command]
pub fn get_latest_timeline(
    app: AppHandle,
    project_id: String,
    storyboard_version_id: String,
) -> Result<Option<LatestTimeline>, String> {
    let connection = open_connection(&app)?;
    let latest = connection.query_row(
        "SELECT id, project_id, storyboard_version_id, version_number, content_json, created_at, status FROM timeline_versions WHERE project_id = ?1 AND storyboard_version_id = ?2 ORDER BY version_number DESC LIMIT 1",
        params![project_id, storyboard_version_id],
        |row| {
            let content: TimelineContent = serde_json::from_str(&row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok((
                TimelineVersion {
                    id: row.get(0)?, project_id: row.get(1)?, storyboard_version_id: row.get(2)?, version_number: row.get(3)?, clips: content.clips, quality_report: content.quality_report, created_at: row.get(5)?,
                },
                row.get::<_, String>(6)?,
            ))
        },
    ).optional().map_err(|_| "Latest timeline could not be read.".to_owned())?;
    let Some((timeline, status)) = latest else {
        return Ok(None);
    };
    let preview_path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("previews")
        .join(&timeline.id)
        .join("preview.mp4");
    let preview = (status == "preview_ready" && preview_path.is_file()).then(|| PreviewResult {
        timeline_version_id: timeline.id.clone(),
        preview_path: preview_path.to_string_lossy().into_owned(),
        quality_report: timeline
            .quality_report
            .clone()
            .unwrap_or(PreviewQualityReport { checks: Vec::new() }),
    });
    Ok(Some(LatestTimeline { timeline, preview }))
}

fn preview_directory(app: &AppHandle, timeline_version_id: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("previews")
        .join(timeline_version_id);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

fn find_jianying_draft_location() -> Option<(PathBuf, PathBuf)> {
    let registry_path = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|directory| {
            directory
                .join("JianyingPro")
                .join("User Data")
                .join("Projects")
                .join("com.lveditor.draft")
                .join("root_meta_info.json")
        })?;
    let registry: serde_json::Value =
        serde_json::from_slice(&fs::read(&registry_path).ok()?).ok()?;
    let draft_root = registry
        .get("all_draft_store")?
        .as_array()?
        .iter()
        .filter_map(|draft| draft.get("draft_root_path")?.as_str())
        .map(PathBuf::from)
        .find(|path| path.is_dir())?;
    Some((draft_root, registry_path))
}

fn tasklist_contains_jianying(output: &[u8]) -> bool {
    const PROCESS_NAME: &[u8] = b"jianyingpro.exe";
    output
        .windows(PROCESS_NAME.len())
        .any(|window| window.eq_ignore_ascii_case(PROCESS_NAME))
}

fn jianying_process_is_running() -> bool {
    hidden_command("tasklist")
        .args(["/FI", "IMAGENAME eq JianyingPro.exe", "/FO", "CSV", "/NH"])
        .output()
        .is_ok_and(|output| tasklist_contains_jianying(&output.stdout))
}

fn jianying_adapter_script(app: &AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scripts")
            .join("create_jianying_draft.py"))
    } else {
        Ok(app
            .path()
            .resource_dir()
            .map_err(|error| error.to_string())?
            .join("scripts")
            .join("create_jianying_draft.py"))
    }
}

fn run_jianying_adapter(
    app: &AppHandle,
    input: &serde_json::Value,
) -> Result<JianyingDraftResult, String> {
    let input_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("jianying-draft-inputs");
    fs::create_dir_all(&input_directory)
        .map_err(|_| "Could not prepare Jianying draft input.".to_owned())?;
    let input_path = input_directory.join(format!("{}.json", Uuid::new_v4()));
    fs::write(&input_path, input.to_string())
        .map_err(|_| "Could not prepare Jianying draft input.".to_owned())?;
    let output = (|| {
        let child = hidden_command("py")
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .arg(jianying_adapter_script(app)?)
            .arg(&input_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| {
                "Python with pyJianYingDraft is unavailable on this computer.".to_owned()
            })?;
        child
            .wait_with_output()
            .map_err(|_| "Jianying draft adapter did not finish.".to_owned())
    })();
    let _ = fs::remove_file(&input_path);
    let output = output?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr)
            .lines()
            .last()
            .unwrap_or("unknown adapter failure")
            .trim()
            .to_owned();
        return Err(format!("Jianying draft adapter failed: {reason}"));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| "Jianying draft adapter returned an invalid result.".to_owned())
}

fn record_jianying_registration_task(
    app: &AppHandle,
    project_id: &str,
    input: &PendingJianyingRegistration,
    result: &JianyingDraftResult,
) -> Result<(), String> {
    let connection = open_connection(app)?;
    let status = if result.registration_status == "registered" {
        "completed"
    } else {
        "queued"
    };
    let timestamp = now_millis();
    connection
        .execute(
            "
            INSERT INTO agent_tasks
              (id, project_id, conversation_id, tool_name, status, input_json, result_json, error_message, created_at, updated_at)
            VALUES (?1, ?2, NULL, 'register_jianying_draft', ?3, ?4, ?5, NULL, ?6, ?6)
            ",
            params![
                Uuid::new_v4().to_string(),
                project_id,
                status,
                serde_json::to_string(input).map_err(|error| error.to_string())?,
                serde_json::to_string(result).map_err(|error| error.to_string())?,
                timestamp,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_jianying_registration_status(
    app: AppHandle,
    timeline_version_id: String,
) -> Result<Option<JianyingRegistrationStatus>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection
        .prepare(
            "
            SELECT status, input_json
            FROM agent_tasks
            WHERE tool_name = 'register_jianying_draft'
            ORDER BY created_at DESC
            ",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (status, input_json) = row.map_err(|error| error.to_string())?;
        let Ok(input) = serde_json::from_str::<PendingJianyingRegistration>(&input_json) else {
            continue;
        };
        if input.timeline_version_id != timeline_version_id {
            continue;
        }
        return Ok(Some(JianyingRegistrationStatus {
            timeline_version_id,
            draft_name: input.draft_name,
            status: match status.as_str() {
                "completed" => "registered",
                "failed" | "cancelled" => "failed",
                _ => "pending",
            }
            .to_owned(),
        }));
    }
    Ok(None)
}

fn process_pending_jianying_registrations(app: &AppHandle) -> Result<(), String> {
    if jianying_process_is_running() {
        return Ok(());
    }
    let connection = open_connection(app)?;
    let mut statement = connection
        .prepare(
            "
            SELECT id, input_json
            FROM agent_tasks
            WHERE tool_name = 'register_jianying_draft' AND status = 'queued'
            ORDER BY created_at ASC
            ",
        )
        .map_err(|error| error.to_string())?;
    let tasks = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);

    for (task_id, input_json) in tasks {
        if jianying_process_is_running() {
            break;
        }
        let input = match serde_json::from_str::<PendingJianyingRegistration>(&input_json) {
            Ok(input) => input,
            Err(_) => {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'failed', error_message = 'Stored Jianying registration input is invalid.', updated_at = ?1 WHERE id = ?2",
                        params![now_millis(), task_id],
                    )
                    .map_err(|error| error.to_string())?;
                continue;
            }
        };
        connection
            .execute(
                "UPDATE agent_tasks SET status = 'running', error_message = NULL, updated_at = ?1 WHERE id = ?2",
                params![now_millis(), task_id],
            )
            .map_err(|error| error.to_string())?;
        let adapter_input = serde_json::to_value(&input).map_err(|error| error.to_string())?;
        match run_jianying_adapter(app, &adapter_input) {
            Ok(result) if result.registration_status == "registered" => {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'completed', result_json = ?1, error_message = NULL, updated_at = ?2 WHERE id = ?3",
                        params![
                            serde_json::to_string(&result).map_err(|error| error.to_string())?,
                            now_millis(),
                            task_id,
                        ],
                    )
                    .map_err(|error| error.to_string())?;
                let _ = app.emit(
                    "jianying-draft-registration-status",
                    JianyingRegistrationStatus {
                        timeline_version_id: input.timeline_version_id,
                        draft_name: input.draft_name,
                        status: "registered".to_owned(),
                    },
                );
                log::info!("Completed deferred Jianying draft registration.");
            }
            _ => {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'queued', error_message = 'Pending Jianying registration will retry.', updated_at = ?1 WHERE id = ?2",
                        params![now_millis(), task_id],
                    )
                    .map_err(|error| error.to_string())?;
                log::warn!("Deferred Jianying draft registration did not complete; it will retry.");
                break;
            }
        }
    }
    Ok(())
}

fn resume_pending_jianying_registrations(app: &AppHandle) -> Result<(), String> {
    static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
    if WORKER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }
    let connection = open_connection(app)?;
    connection
        .execute(
            "UPDATE agent_tasks SET status = 'queued', updated_at = ?1 WHERE tool_name = 'register_jianying_draft' AND status = 'running'",
            params![now_millis()],
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    let app = app.clone();
    thread::spawn(move || loop {
        if !jianying_process_is_running() {
            if let Err(error) = process_pending_jianying_registrations(&app) {
                log::warn!("Pending Jianying registration worker failed: {error}");
            }
        }
        thread::sleep(Duration::from_secs(2));
    });
    Ok(())
}

fn render_timeline_clip(
    source: &Path,
    kind: &str,
    clip: &TimelineClip,
    destination: &Path,
) -> Result<(), String> {
    let duration = (clip.timeline_end_ms - clip.timeline_start_ms) as f64 / 1000.0;
    let mut command = hidden_command("ffmpeg");
    command.args(["-y", "-hide_banner", "-loglevel", "error"]);
    if kind == "video" {
        command
            .args([
                "-ss",
                &format!("{:.3}", clip.source_start_ms as f64 / 1000.0),
                "-i",
            ])
            .arg(source)
            .args(["-t", &format!("{duration:.3}")]);
    } else if kind == "image" {
        command
            .args(["-loop", "1", "-i"])
            .arg(source)
            .args(["-t", &format!("{duration:.3}")]);
    } else {
        return Err("Timeline clip uses unsupported media.".to_owned());
    }
    let status = command
        .args([
            "-vf",
            "scale=540:960:force_original_aspect_ratio=increase,crop=540:960,fps=30,format=yuv420p",
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-movflags",
            "+faststart",
        ])
        .arg(destination)
        .status()
        .map_err(|_| "FFmpeg is not available on this computer.".to_owned())?;
    if status.success() {
        Ok(())
    } else {
        Err("FFmpeg could not render a timeline clip.".to_owned())
    }
}

fn visual_signature(path: &Path, duration_ms: i64) -> Option<Vec<u8>> {
    let midpoint = (duration_ms.max(1) as f64 / 2_000.0).to_string();
    let output = hidden_command("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-ss", &midpoint, "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=24:24,format=gray",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .ok()?;
    (output.status.success() && output.stdout.len() == 24 * 24).then_some(output.stdout)
}

fn mean_pixel_difference(first: &[u8], second: &[u8]) -> Option<f64> {
    (first.len() == second.len() && !first.is_empty()).then(|| {
        first
            .iter()
            .zip(second)
            .map(|(left, right)| left.abs_diff(*right) as u64)
            .sum::<u64>() as f64
            / first.len() as f64
    })
}

fn inspect_preview_quality(
    preview_path: &Path,
    clips: &[TimelineClip],
    rendered: &[PathBuf],
) -> PreviewQualityReport {
    let mut checks = Vec::new();
    let output = hidden_command("ffmpeg")
        .args(["-hide_banner", "-loglevel", "info", "-i"])
        .arg(preview_path)
        .args([
            "-vf",
            "blackdetect=d=0.10:pix_th=0.10",
            "-an",
            "-f",
            "null",
            "-",
        ])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let black_segments = String::from_utf8_lossy(&output.stderr)
                .lines()
                .filter(|line| line.contains("black_start:"))
                .count();
            if black_segments > 0 {
                checks.push(PreviewQualityCheck {
                    category: "black_frames".to_owned(),
                    severity: "warning".to_owned(),
                    message: format!(
                        "Detected {black_segments} black-frame segment(s) in the rendered preview."
                    ),
                    shot_indices: Vec::new(),
                });
            }
        }
        _ => checks.push(PreviewQualityCheck {
            category: "black_frames".to_owned(),
            severity: "info".to_owned(),
            message: "Black-frame scan could not complete; the preview remains available."
                .to_owned(),
            shot_indices: Vec::new(),
        }),
    }
    for (index, clip) in clips.iter().enumerate() {
        for other in clips.iter().skip(index + 1) {
            if clip.asset_id == other.asset_id
                && clip.source_start_ms == other.source_start_ms
                && clip.source_end_ms == other.source_end_ms
            {
                checks.push(PreviewQualityCheck {
                    category: "duplicate_footage".to_owned(),
                    severity: "warning".to_owned(),
                    message: "Two timeline shots use the same source range.".to_owned(),
                    shot_indices: vec![clip.shot_index, other.shot_index],
                });
            }
        }
    }
    let signatures = clips
        .iter()
        .zip(rendered)
        .map(|(clip, path)| visual_signature(path, clip.timeline_end_ms - clip.timeline_start_ms))
        .collect::<Vec<_>>();
    for (index, clip) in clips.iter().enumerate() {
        for (other_index, other) in clips.iter().enumerate().skip(index + 1) {
            if clip.asset_id == other.asset_id
                && clip.source_start_ms == other.source_start_ms
                && clip.source_end_ms == other.source_end_ms
            {
                continue;
            }
            if let (Some(first), Some(second)) = (&signatures[index], &signatures[other_index]) {
                if mean_pixel_difference(first, second).is_some_and(|difference| difference < 12.0)
                {
                    checks.push(PreviewQualityCheck {
                        category: "visual_similarity".to_owned(), severity: "warning".to_owned(),
                        message: "Two different source ranges have highly similar sampled frames; review for repeated footage.".to_owned(),
                        shot_indices: vec![clip.shot_index, other.shot_index],
                    });
                }
            }
        }
    }
    let pacing_shots = clips
        .iter()
        .filter_map(|clip| {
            let duration = clip.timeline_end_ms - clip.timeline_start_ms;
            (!(750..=6_000).contains(&duration)).then_some(clip.shot_index)
        })
        .collect::<Vec<_>>();
    if !pacing_shots.is_empty() {
        checks.push(PreviewQualityCheck {
            category: "pacing".to_owned(),
            severity: "info".to_owned(),
            message:
                "Some shots are shorter than 0.75s or longer than 6s; review pacing in the preview."
                    .to_owned(),
            shot_indices: pacing_shots,
        });
    }
    let caption_shots = clips
        .iter()
        .filter_map(|clip| (!clip.on_screen_text.trim().is_empty()).then_some(clip.shot_index))
        .collect::<Vec<_>>();
    if !caption_shots.is_empty() {
        checks.push(PreviewQualityCheck {
            category: "subtitles".to_owned(),
            severity: "info".to_owned(),
            message: "Storyboard text is not yet rendered as captions in previews.".to_owned(),
            shot_indices: caption_shots,
        });
    }
    PreviewQualityReport { checks }
}

#[tauri::command]
pub fn render_preview(
    app: AppHandle,
    timeline_version_id: String,
) -> Result<PreviewResult, String> {
    log::info!("Starting local preview render.");
    let connection = open_connection(&app)?;
    let timeline = load_timeline_version(&connection, &timeline_version_id)?;
    if timeline.clips.is_empty() {
        return Err("Timeline has no clips to render.".to_owned());
    }
    let directory = preview_directory(&app, &timeline.id)?;
    let mut rendered = Vec::with_capacity(timeline.clips.len());
    for (index, clip) in timeline.clips.iter().enumerate() {
        let (source_reference, kind): (String, String) = connection
            .query_row(
                "SELECT source_reference, kind FROM assets WHERE id = ?1 AND project_id = ?2",
                params![clip.asset_id, timeline.project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Timeline references an unavailable asset.".to_owned())?;
        if !Path::new(&source_reference).is_file() {
            return Err("Timeline source media is no longer available. Reconnect or replace the missing asset before rendering.".to_owned());
        }
        let destination = directory.join(format!("clip_{index:03}.mp4"));
        render_timeline_clip(Path::new(&source_reference), &kind, clip, &destination)?;
        rendered.push(destination);
    }
    let list_path = directory.join("concat.txt");
    let list = rendered
        .iter()
        .map(|path| {
            format!(
                "file '{}'",
                path.to_string_lossy()
                    .replace('\\', "/")
                    .replace('\'', "\\'")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&list_path, list).map_err(|_| "Could not prepare preview sequence.".to_owned())?;
    let preview_path = directory.join("preview.mp4");
    let status = hidden_command("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&list_path)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(&preview_path)
        .status()
        .map_err(|_| "FFmpeg is not available on this computer.".to_owned())?;
    if !status.success() {
        return Err("FFmpeg could not assemble the preview.".to_owned());
    }
    let quality_report = inspect_preview_quality(&preview_path, &timeline.clips, &rendered);
    let content_json = serde_json::to_string(&TimelineContent {
        clips: timeline.clips.clone(),
        quality_report: Some(quality_report.clone()),
    })
    .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE timeline_versions SET status = 'preview_ready', content_json = ?1 WHERE id = ?2",
            params![content_json, timeline.id],
        )
        .map_err(|error| error.to_string())?;
    log::info!("Completed local preview render.");
    Ok(PreviewResult {
        timeline_version_id: timeline.id,
        preview_path: preview_path.to_string_lossy().into_owned(),
        quality_report,
    })
}

#[tauri::command]
pub fn create_jianying_draft(
    app: AppHandle,
    timeline_version_id: String,
) -> Result<JianyingDraftResult, String> {
    log::info!("Starting Jianying draft creation.");
    let connection = open_connection(&app)?;
    let timeline = load_timeline_version(&connection, &timeline_version_id)?;
    let (root, registry_path) = find_jianying_draft_location().ok_or_else(|| {
        "Jianying Pro 8.0 draft library is unavailable. Open Jianying Pro and create a local draft before creating a draft here."
            .to_owned()
    })?;
    let mut clips = Vec::with_capacity(timeline.clips.len());
    for (index, clip) in timeline.clips.iter().enumerate() {
        let (source_reference, kind): (String, String) = connection
            .query_row(
                "SELECT source_reference, kind FROM assets WHERE id = ?1 AND project_id = ?2",
                params![clip.asset_id, timeline.project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Timeline references an unavailable asset.".to_owned())?;
        if kind != "video" {
            return Err(
                "Jianying draft export currently supports video timeline clips only.".to_owned(),
            );
        }
        if !Path::new(&source_reference).is_file() {
            return Err(format!(
                "Timeline source media file {} is unavailable. Re-import or relink the missing asset before creating a Jianying draft.",
                index + 1
            ));
        }
        clips.push(serde_json::json!({
            "sourceReference": source_reference.replace('\\', "/"), "sourceStartMs": clip.source_start_ms,
            "timelineStartMs": clip.timeline_start_ms, "timelineEndMs": clip.timeline_end_ms,
        }));
    }
    let draft_name = format!("Assembly Video Agent {}", Uuid::new_v4());
    let draft_root = root.to_string_lossy().replace('\\', "/");
    let draft_registry_path = registry_path.to_string_lossy().replace('\\', "/");
    let duration_ms = timeline
        .clips
        .iter()
        .map(|clip| clip.timeline_end_ms)
        .max()
        .unwrap_or(0);
    let input = serde_json::json!({
        "inputFormatVersion": 1,
        "operation": "createDraft",
        "draftRoot": draft_root,
        "draftName": draft_name,
        "draftRegistryPath": draft_registry_path,
        "clips": clips
    });
    let result = run_jianying_adapter(&app, &input).map_err(|error| {
        log::error!("{error}");
        format!("Jianying draft adapter could not create a draft: {error}")
    })?;
    let registration = PendingJianyingRegistration {
        input_format_version: 1,
        operation: "registerDraft".to_owned(),
        draft_root,
        draft_name,
        draft_registry_path,
        draft_directory: result.draft_directory.clone(),
        duration_ms,
        timeline_version_id: timeline.id.clone(),
    };
    record_jianying_registration_task(&app, &timeline.project_id, &registration, &result)?;
    if result.registration_status == "pending" {
        log::info!(
            "Completed Jianying draft creation; registration is waiting for Jianying to exit."
        );
    } else {
        log::info!("Completed Jianying draft creation and registration.");
    }
    Ok(result)
}

fn request_agent_edit_decision(
    access: &oauth::AuthorizedOAuth,
    request: &str,
    task_brief: &str,
    has_storyboard: bool,
    timelines: &[TimelineVersion],
    sources: &[StoryboardSource],
) -> Result<AgentEditDecision, String> {
    let timeline_json = serde_json::to_string(timelines)
        .map_err(|_| "Could not prepare timeline context.".to_owned())?;
    let evidence = serde_json::to_string(sources)
        .map_err(|_| "Could not prepare media evidence.".to_owned())?;
    let prompt = format!("You are Assembly Agent, a helpful Chinese-speaking local video editing Agent. User request: {request}\nCurrent task brief: {task_brief}\nA validated storyboard exists: {has_storyboard}\nCurrent storyboard timeline candidates: {timeline_json}\nAvailable analyzed media evidence: {evidence}\nReturn JSON with tool, reason, reply, and optional taskBrief. reply is required and must answer naturally in concise Chinese. taskBrief is present only when the user gives or materially revises a video-creation goal; never set it for ordinary questions. Never claim to have seen media beyond supplied evidence. tool must be exactly one of generate_storyboard, create_timeline_draft, replace_timeline_clip, render_preview, create_jianying_draft, no_action. Choose generate_storyboard only when the user explicitly asks to generate a storyboard/video and there is a non-empty taskBrief plus available evidence. Choose create_timeline_draft only when the user explicitly asks for an internal timeline or internal editing timeline and a storyboard exists. Choose render_preview when the user asks to make, render, or view a preview and a storyboard exists. Choose create_jianying_draft when the user asks to create or generate a draft without limiting it to an internal timeline, or explicitly asks to create, send, or write the current timeline to Jianying, and a storyboard exists. In Chinese, an unqualified 草稿 means a Jianying draft; 内部时间线 or 时间线 means the internal timeline. Choose replace_timeline_clip only when the user clearly asks to replace one existing shot and there is an existing candidate. For timeline-specific actions, return timelineVersionId when choosing among multiple candidates. For replace_timeline_clip, also return shotIndex, assetId, sourceStartMs, and sourceEndMs. Use only supplied asset IDs and verified source ranges. A video range must exactly match the selected shot's timeline duration; an image uses 0 for both source times. Choose no_action for questions, planning, unclear requests, or unsupported actions. Do not invent tools. Do not claim that an action has completed in reply; the backend will append the verified result after the tool finishes.");
    let request = serde_json::json!({ "model": "gpt-5.4", "store": false, "stream": true, "input": [{ "role": "user", "content": [{ "type": "input_text", "text": prompt }] }], "text": { "format": { "type": "json_object" } } });
    let mut response = ureq::post("https://chatgpt.com/backend-api/codex/responses")
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", access.access_token))
        .set("originator", "opencode");
    if let Some(account_id) = &access.account_id {
        response = response.set("ChatGPT-Account-Id", account_id);
    }
    let body = response
        .send_string(&request.to_string())
        .map_err(|error| match error {
            ureq::Error::Status(status, response) => {
                let detail = response
                    .into_string()
                    .unwrap_or_default()
                    .chars()
                    .take(300)
                    .collect::<String>();
                format!("Experimental Agent request failed with HTTP {status}: {detail}")
            }
            _ => "Experimental Agent request failed before receiving a response.".to_owned(),
        })?
        .into_string()
        .map_err(|_| "Experimental Agent response was empty.".to_owned())?;
    let text = response_json_text(&body)
        .ok_or_else(|| "Experimental Agent response did not contain JSON.".to_owned())?;
    serde_json::from_str(&text).map_err(|_| {
        format!(
            "Experimental Agent tool decision was invalid: {}",
            text.chars().take(300).collect::<String>()
        )
    })
}

fn explicit_draft_tool(request: &str) -> Option<&'static str> {
    let normalized = request.trim().trim_matches(|character: char| {
        matches!(
            character,
            '。' | '！' | '？' | '.' | '!' | '?' | '，' | ',' | ' '
        )
    });
    match normalized {
        "生成草稿" | "创建草稿" | "生成一个草稿" | "创建一个草稿" | "做一个草稿" => {
            Some("create_jianying_draft")
        }
        "生成内部时间线"
        | "创建内部时间线"
        | "生成时间线"
        | "创建时间线"
        | "生成时间线草稿"
        | "创建时间线草稿" => Some("create_timeline_draft"),
        _ => None,
    }
}

fn verified_action_message(reply: &str, outcome: String) -> String {
    let reply = reply.trim();
    if reply.is_empty() {
        outcome
    } else {
        format!("{reply}\n\n{outcome}")
    }
}

#[tauri::command]
pub fn execute_agent_edit(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    storyboard_version_id: Option<String>,
    timeline_version_id: Option<String>,
    request: String,
) -> Result<AgentEditResult, String> {
    if request.trim().is_empty() {
        return Err("Agent request cannot be empty.".to_owned());
    }
    log::info!("Starting AI edit decision.");
    let access = oauth::experimental_access().map_err(|error| {
        log::warn!("AI edit could not access the configured provider: {error}");
        error
    })?;
    let connection = open_connection(&app)?;
    let storyboard = storyboard_version_id
        .as_ref()
        .map(|id| load_storyboard_version(&connection, id))
        .transpose()?;
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
    let timelines = match &storyboard {
        Some(value) => timeline_candidates_for_storyboard(&connection, &project_id, &value.id)?,
        None => Vec::new(),
    };
    if timeline_version_id
        .as_ref()
        .is_some_and(|timeline_id| !timelines.iter().any(|timeline| timeline.id == *timeline_id))
    {
        return Err("Selected timeline does not belong to the current storyboard.".to_owned());
    }
    let sources = storyboard_sources(&connection, &project_id)?;
    let mut decision = request_agent_edit_decision(
        &access,
        &request,
        &task_brief,
        storyboard.is_some(),
        &timelines,
        &sources,
    )
    .map_err(|error| {
        log::warn!("AI edit decision failed: {error}");
        error
    })?;
    if let Some(tool) = explicit_draft_tool(&request) {
        decision.tool = tool.to_owned();
        decision.reply.clear();
    }
    if let Some(brief) = decision
        .task_brief
        .as_deref()
        .map(str::trim)
        .filter(|brief| !brief.is_empty())
    {
        connection
            .execute(
                "UPDATE editing_tasks SET brief = ?1, title = CASE WHEN title IN ('新的剪辑任务', '新的剪辑会话') THEN substr(?1, 1, 28) ELSE title END, updated_at = ?2 WHERE id = ?3",
                params![brief, now_millis(), editing_task_id],
            )
            .map_err(|error| error.to_string())?;
    }
    let selected_timeline = select_timeline_candidate(
        &timelines,
        decision.timeline_version_id.as_deref(),
        timeline_version_id.as_deref(),
    );
    match decision.tool.as_str() {
        "generate_storyboard" => {
            let brief = decision
                .task_brief
                .as_deref()
                .map(str::trim)
                .filter(|brief| !brief.is_empty())
                .unwrap_or(task_brief.trim());
            if brief.is_empty() {
                return Err("Agent needs a video goal before generating a storyboard.".to_owned());
            }
            let generated =
                generate_storyboard(app, project_id, editing_task_id, brief.to_owned())?;
            Ok(AgentEditResult {
                message: decision.reply,
                timeline: None,
                preview: None,
                storyboard: Some(generated),
                jianying_draft: None,
            })
        }
        "create_timeline_draft" => {
            let timeline = create_timeline_draft(
                app,
                project_id,
                storyboard
                    .ok_or_else(|| "Create a storyboard before creating a timeline.".to_owned())?
                    .id,
            )?;
            Ok(AgentEditResult {
                message: verified_action_message(
                    &decision.reply,
                    format!(
                        "已创建内部时间线 v{}。它只保存在本应用中，不会自动出现在剪映；如需写入剪映，请说“创建剪映草稿”。",
                        timeline.version_number
                    ),
                ),
                storyboard: None,
                timeline: Some(timeline),
                preview: None,
                jianying_draft: None,
            })
        }
        "render_preview" => {
            let timeline = if timelines.is_empty() {
                create_timeline_draft(
                    app.clone(),
                    project_id,
                    storyboard
                        .as_ref()
                        .ok_or_else(|| {
                            "Create a storyboard before rendering a preview.".to_owned()
                        })?
                        .id
                        .clone(),
                )?
            } else {
                selected_timeline.ok_or_else(|| {
                    "Agent must select a timeline that belongs to the current storyboard before rendering."
                        .to_owned()
                })?
            };
            let preview = render_preview(app, timeline.id.clone())?;
            Ok(AgentEditResult {
                message: if decision.reply.is_empty() {
                    format!("已按你的请求生成本地低清预览。{}", decision.reason)
                } else {
                    decision.reply
                },
                storyboard: None,
                timeline: Some(timeline),
                preview: Some(preview),
                jianying_draft: None,
            })
        }
        "create_jianying_draft" => {
            let timeline = if timelines.is_empty() {
                create_timeline_draft(
                    app.clone(),
                    project_id,
                    storyboard
                        .as_ref()
                        .ok_or_else(|| {
                            "Create a storyboard before creating a Jianying draft.".to_owned()
                        })?
                        .id
                        .clone(),
                )?
            } else {
                selected_timeline.ok_or_else(|| {
                    "Agent must select a timeline that belongs to the current storyboard before creating a Jianying draft."
                        .to_owned()
                })?
            };
            let draft = create_jianying_draft(app, timeline.id.clone())?;
            let draft_name = Path::new(&draft.draft_directory)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Assembly Video Agent");
            let verified_message = if draft.registration_status == "pending" {
                format!(
                    "已生成剪映草稿“{draft_name}”。剪映当前正在运行；退出剪映后，Assembly Video Agent 会自动完成注册。内部时间线 v{} 仍保留在本应用中。",
                    timeline.version_number
                )
            } else {
                format!(
                    "已创建并注册剪映草稿“{draft_name}”。现在可以打开剪映，并在“本地草稿”中查看；内部时间线 v{} 仍保留在本应用中。",
                    timeline.version_number
                )
            };
            Ok(AgentEditResult {
                message: verified_action_message(&decision.reply, verified_message),
                storyboard: None,
                timeline: Some(timeline),
                preview: None,
                jianying_draft: Some(draft),
            })
        }
        "replace_timeline_clip" => {
            let existing = selected_timeline.ok_or_else(|| {
                "Agent must select a timeline that belongs to the current storyboard before replacing a shot."
                    .to_owned()
            })?;
            let shot_index = decision
                .shot_index
                .ok_or_else(|| "Agent did not identify a timeline shot to replace.".to_owned())?;
            let asset_id = decision
                .asset_id
                .ok_or_else(|| "Agent did not identify replacement media.".to_owned())?;
            let source_start_ms = decision
                .source_start_ms
                .ok_or_else(|| "Agent did not provide a replacement start time.".to_owned())?;
            let source_end_ms = decision
                .source_end_ms
                .ok_or_else(|| "Agent did not provide a replacement end time.".to_owned())?;
            let replacement = create_replaced_timeline_version(
                &connection,
                &project_id,
                &existing,
                shot_index,
                asset_id,
                source_start_ms,
                source_end_ms,
            )?;
            Ok(AgentEditResult {
                message: if decision.reply.is_empty() {
                    format!(
                        "已替换第 {shot_index} 个镜头并创建本地时间线 v{}。{}",
                        replacement.version_number, decision.reason
                    )
                } else {
                    decision.reply
                },
                storyboard: None,
                timeline: Some(replacement),
                preview: None,
                jianying_draft: None,
            })
        }
        "no_action" => Ok(AgentEditResult {
            message: if decision.reply.is_empty() {
                "我可以回答项目问题、规划视频，并在需要时创建草稿、替换镜头或生成预览。".to_owned()
            } else {
                decision.reply
            },
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        }),
        _ => Err("Agent attempted to call a disallowed editing tool.".to_owned()),
    }
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
    fn editing_session_projection_uses_the_latest_legacy_conversation() {
        let connection = Connection::open_in_memory().expect("open session test database");
        migrate(&connection).expect("create current schema");
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
    fn requested_timeline_is_used_when_multiple_candidates_exist() {
        let timeline = |id: &str, version_number| TimelineVersion {
            id: id.to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number,
            clips: Vec::new(),
            quality_report: None,
            created_at: version_number,
        };
        let timelines = vec![timeline("timeline-2", 2), timeline("timeline-1", 1)];

        let selected =
            select_timeline_candidate(&timelines, None, Some("timeline-1")).expect("selection");
        assert_eq!(selected.id, "timeline-1");
    }

    #[test]
    fn unqualified_draft_command_targets_jianying() {
        assert_eq!(
            explicit_draft_tool("生成草稿"),
            Some("create_jianying_draft")
        );
        assert_eq!(
            explicit_draft_tool("创建时间线草稿"),
            Some("create_timeline_draft")
        );
        assert_eq!(explicit_draft_tool("草稿为什么没出现"), None);
    }

    #[test]
    fn verified_action_outcome_is_never_hidden_by_the_model_reply() {
        assert_eq!(
            verified_action_message("好的，我来处理。", "已创建内部时间线 v3。".to_owned()),
            "好的，我来处理。\n\n已创建内部时间线 v3。"
        );
        assert_eq!(
            verified_action_message("", "已创建剪映草稿。".to_owned()),
            "已创建剪映草稿。"
        );
    }

    #[test]
    fn replacing_a_clip_creates_a_new_version_without_moving_timeline_bounds() {
        let connection = Connection::open_in_memory().expect("open test database");
        connection
            .execute_batch(
                "
                CREATE TABLE assets (id TEXT, project_id TEXT, kind TEXT, analysis_status TEXT, metadata_json TEXT);
                CREATE TABLE timeline_versions (id TEXT, project_id TEXT, storyboard_version_id TEXT, version_number INTEGER, status TEXT, content_json TEXT, created_at INTEGER);
                CREATE TABLE operation_logs (id TEXT, project_id TEXT, actor TEXT, operation_type TEXT, entity_type TEXT, entity_id TEXT, before_json TEXT, after_json TEXT, created_at INTEGER);
                ",
            )
            .expect("create test tables");
        let replacement_metadata = serde_json::to_string(&TechnicalMetadata {
            duration_ms: Some(10_000),
            ..TechnicalMetadata::default()
        })
        .expect("serialize replacement metadata");
        connection
            .execute(
                "INSERT INTO assets VALUES ('replacement-video', 'project-1', 'video', 'ready', ?1)",
                params![replacement_metadata],
            )
            .expect("insert replacement asset");
        connection
            .execute(
                "INSERT INTO timeline_versions VALUES ('existing', 'project-1', 'storyboard-1', 1, 'draft', '{}', 1)",
                [],
            )
            .expect("insert existing version");
        let existing = TimelineVersion {
            id: "existing".to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number: 1,
            clips: vec![TimelineClip {
                shot_index: 1,
                asset_id: "original-video".to_owned(),
                source_start_ms: 0,
                source_end_ms: 2_000,
                timeline_start_ms: 0,
                timeline_end_ms: 2_000,
                on_screen_text: "Quality".to_owned(),
            }],
            quality_report: None,
            created_at: 1,
        };
        let replacement = create_replaced_timeline_version(
            &connection,
            "project-1",
            &existing,
            1,
            "replacement-video".to_owned(),
            3_000,
            5_000,
        )
        .expect("replace scoped clip");
        assert_eq!(replacement.version_number, 2);
        assert_eq!(replacement.clips[0].asset_id, "replacement-video");
        assert_eq!(replacement.clips[0].source_start_ms, 3_000);
        assert_eq!(replacement.clips[0].source_end_ms, 5_000);
        assert_eq!(replacement.clips[0].timeline_start_ms, 0);
        assert_eq!(replacement.clips[0].timeline_end_ms, 2_000);
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM operation_logs", [], |row| row
                    .get::<_, i64>(0))
                .expect("count operation logs"),
            1
        );
    }

    #[test]
    fn visual_signature_comparison_distinguishes_similar_frames() {
        assert_eq!(
            mean_pixel_difference(&[10, 12, 14], &[11, 13, 15]),
            Some(1.0)
        );
        assert_eq!(mean_pixel_difference(&[0, 0], &[255, 255]), Some(255.0));
        assert_eq!(mean_pixel_difference(&[1], &[1, 2]), None);
    }

    #[test]
    fn jianying_process_detection_uses_raw_tasklist_bytes() {
        assert!(tasklist_contains_jianying(
            b"\"JianyingPro.exe\",\"1234\",\"Console\",\"1\",\"100 K\""
        ));
        assert!(tasklist_contains_jianying(b"JIANYINGPRO.EXE"));
        assert!(!tasklist_contains_jianying(
            b"INFO: No tasks are running which match the specified criteria."
        ));
    }

    #[test]
    fn ffmpeg_renders_a_source_bound_vertical_clip() {
        let directory =
            std::env::temp_dir().join(format!("assembly-video-agent-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create temporary test directory");
        let source = directory.join("source.mp4");
        let source_status = hidden_command("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=640x360:rate=30",
                "-t",
                "2",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .expect("run ffmpeg test source generation");
        assert!(
            source_status.success(),
            "ffmpeg must generate a test source"
        );
        let destination = directory.join("clip.mp4");
        let clip = TimelineClip {
            shot_index: 1,
            asset_id: "test".to_owned(),
            source_start_ms: 0,
            source_end_ms: 1_000,
            timeline_start_ms: 0,
            timeline_end_ms: 1_000,
            on_screen_text: String::new(),
        };
        render_timeline_clip(&source, "video", &clip, &destination)
            .expect("render vertical timeline clip");
        assert!(
            destination.is_file(),
            "timeline render must create an MP4 clip"
        );
        let probe = hidden_command("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0",
            ])
            .arg(&destination)
            .output()
            .expect("run ffprobe on rendered clip");
        assert!(
            probe.status.success(),
            "ffprobe must read the rendered clip"
        );
        assert!(
            String::from_utf8_lossy(&probe.stdout).contains("540,960"),
            "rendered preview clip must be 540 x 960"
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn ffmpeg_assembles_normalized_clips_into_a_preview() {
        let directory = std::env::temp_dir().join(format!(
            "assembly-video-agent-preview-test-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("create temporary preview directory");
        let source = directory.join("source.mp4");
        let source_status = hidden_command("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=30",
                "-t",
                "3",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .expect("generate preview source");
        assert!(
            source_status.success(),
            "ffmpeg must generate preview source"
        );
        let first = directory.join("clip_000.mp4");
        let second = directory.join("clip_001.mp4");
        let clip = TimelineClip {
            shot_index: 1,
            asset_id: "test".to_owned(),
            source_start_ms: 0,
            source_end_ms: 1_000,
            timeline_start_ms: 0,
            timeline_end_ms: 1_000,
            on_screen_text: String::new(),
        };
        render_timeline_clip(&source, "video", &clip, &first).expect("render first clip");
        let second_clip = TimelineClip {
            shot_index: 2,
            asset_id: "test".to_owned(),
            source_start_ms: 1_000,
            source_end_ms: 2_000,
            timeline_start_ms: 1_000,
            timeline_end_ms: 2_000,
            on_screen_text: String::new(),
        };
        render_timeline_clip(&source, "video", &second_clip, &second).expect("render second clip");
        let list = directory.join("concat.txt");
        fs::write(
            &list,
            format!(
                "file '{}'\nfile '{}'",
                first.to_string_lossy().replace('\\', "/"),
                second.to_string_lossy().replace('\\', "/")
            ),
        )
        .expect("write concat list");
        let preview = directory.join("preview.mp4");
        let status = hidden_command("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
            ])
            .arg(&list)
            .args(["-c", "copy"])
            .arg(&preview)
            .status()
            .expect("assemble preview");
        assert!(
            status.success() && preview.is_file(),
            "ffmpeg must assemble the preview"
        );
        let probe = hidden_command("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=nw=1:nk=1",
            ])
            .arg(&preview)
            .output()
            .expect("probe preview duration");
        let duration = String::from_utf8_lossy(&probe.stdout)
            .trim()
            .parse::<f64>()
            .expect("parse preview duration");
        assert!(
            (1.8..=2.2).contains(&duration),
            "preview must contain both one-second clips"
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    #[ignore = "requires an authenticated experimental OpenCode-compatible Provider"]
    fn experimental_agent_selects_a_preview_tool() {
        let access = oauth::experimental_access().expect("experimental OAuth access");
        let decision =
            request_agent_edit_decision(&access, "请生成低清预览", "生成预览", true, &[], &[])
                .expect("experimental model tool decision");
        assert_eq!(
            decision.tool, "render_preview",
            "model must select only the requested preview tool"
        );
    }

    #[test]
    #[ignore = "requires an authenticated experimental OpenCode-compatible Provider"]
    fn experimental_agent_selects_a_scoped_clip_replacement() {
        let timeline = TimelineVersion {
            id: "timeline-001".to_owned(),
            project_id: "project-001".to_owned(),
            storyboard_version_id: "storyboard-001".to_owned(),
            version_number: 1,
            clips: vec![TimelineClip {
                shot_index: 1,
                asset_id: "video-001".to_owned(),
                source_start_ms: 0,
                source_end_ms: 2_000,
                timeline_start_ms: 0,
                timeline_end_ms: 2_000,
                on_screen_text: String::new(),
            }],
            quality_report: None,
            created_at: 0,
        };
        let source = StoryboardSource {
            asset_id: "video-002".to_owned(),
            kind: "video".to_owned(),
            duration_ms: Some(10_000),
            scene_segments: vec![SceneSegment {
                start_ms: 3_000,
                end_ms: 5_000,
            }],
            ocr_evidence: Vec::new(),
            visual_evidence: Vec::new(),
        };
        let access = oauth::experimental_access().expect("experimental OAuth access");
        let decision = request_agent_edit_decision(
            &access,
            "Replace shot 1 with video-002 from exactly 3000ms to 5000ms.",
            "Replace the first shot.",
            true,
            &[timeline],
            &[source],
        )
        .expect("experimental model tool decision");
        assert_eq!(decision.tool, "replace_timeline_clip");
        assert_eq!(decision.shot_index, Some(1));
        assert_eq!(decision.asset_id.as_deref(), Some("video-002"));
        assert_eq!(decision.source_start_ms, Some(3_000));
        assert_eq!(decision.source_end_ms, Some(5_000));
    }

    #[test]
    #[ignore = "requires an authenticated experimental OpenCode-compatible Provider"]
    fn experimental_model_generates_a_valid_source_bound_storyboard() {
        let source = StoryboardSource {
            asset_id: "video-001".to_owned(),
            kind: "video".to_owned(),
            duration_ms: Some(60_000),
            scene_segments: vec![
                SceneSegment {
                    start_ms: 0,
                    end_ms: 20_000,
                },
                SceneSegment {
                    start_ms: 20_000,
                    end_ms: 40_000,
                },
                SceneSegment {
                    start_ms: 40_000,
                    end_ms: 60_000,
                },
            ],
            ocr_evidence: vec![OcrEvidence {
                time_ms: Some(5_000),
                text: "Precision delivery".to_owned(),
            }],
            visual_evidence: vec![VisualEvidence {
                time_ms: Some(5_000),
                subjects: vec!["industrial product".to_owned()],
                scene: Some("factory floor".to_owned()),
                actions: vec!["quality inspection".to_owned()],
                products: vec!["finished component".to_owned()],
                quality_notes: Vec::new(),
            }],
        };
        let access = oauth::experimental_access().expect("experimental OAuth access");
        let storyboard = request_storyboard(&access, "Create a 20-second English vertical product promotion focused on quality and delivery.", &[source])
            .expect("experimental model storyboard response");
        validate_storyboard(
            &storyboard,
            &[StoryboardSource {
                asset_id: "video-001".to_owned(),
                kind: "video".to_owned(),
                duration_ms: Some(60_000),
                scene_segments: Vec::new(),
                ocr_evidence: Vec::new(),
                visual_evidence: Vec::new(),
            }],
        )
        .expect("storyboard must use only valid source ranges");
    }
}
