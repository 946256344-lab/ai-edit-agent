//! NativeToolLoop 每轮权威状态快照的聚合与安全渲染边界。
//!
//! 本模块只输出固定顺序的计数、版本号和布尔状态；数据库内部 ID、路径、文件名、
//! 媒体证据和凭据值仅用于本地判定，绝不进入 Provider 上下文。

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf};

pub(super) const STATE_SNAPSHOT_PREFIX: &str = "由系统于本轮开始时从本地数据库读取，权威事实";
const MAX_SNAPSHOT_CHARS: usize = 1_200;
const MAX_BRIEF_CHARS: usize = 180;

#[derive(Clone, Copy)]
struct SnapshotCapabilities {
    model: bool,
    voiceover: bool,
    jamendo: bool,
}

#[derive(Default)]
struct Counts {
    values: BTreeMap<String, i64>,
}

impl Counts {
    fn get(&self, key: &str) -> i64 {
        self.values.get(key).copied().unwrap_or(0)
    }

    fn sum_except(&self, known: &[&str]) -> i64 {
        self.values
            .iter()
            .filter(|(key, _)| !known.contains(&key.as_str()))
            .map(|(_, value)| value)
            .sum()
    }
}

#[derive(Default)]
struct StoryboardSummary {
    id: String,
    version: i64,
    shots: usize,
    uncovered: usize,
    pending_confirmation: bool,
}

#[derive(Default)]
struct TimelineSummary {
    id: String,
    version: i64,
    status: String,
    clips: usize,
    text: usize,
    music: usize,
    voiceover: usize,
}

pub(super) fn build_state_snapshot(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
) -> Result<String, String> {
    let custom_model = crate::custom_api::custom_api_configured_for_snapshot()?;
    let oauth_model = if custom_model {
        false
    } else {
        crate::oauth::experimental_oauth_configured_for_snapshot()?
    };
    let capabilities = SnapshotCapabilities {
        model: custom_model || oauth_model,
        voiceover: crate::music_provider::fish_audio::configured_for_snapshot()?
            || crate::music_provider::elevenlabs_configured_for_snapshot()?,
        jamendo: crate::music_provider::jamendo_configured_for_snapshot()?,
    };
    build_state_snapshot_with_capabilities(
        connection,
        project_id,
        editing_task_id,
        capabilities,
        database_directory(connection),
    )
}

fn build_state_snapshot_with_capabilities(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    capabilities: SnapshotCapabilities,
    data_directory: Option<PathBuf>,
) -> Result<String, String> {
    let brief = connection
        .query_row(
            "SELECT brief FROM editing_tasks WHERE id = ?1 AND project_id = ?2",
            params![editing_task_id, project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| "State snapshot task could not be read.".to_owned())?
        .ok_or_else(|| "State snapshot scope is invalid.".to_owned())?;
    let terminal_task = connection
        .query_row(
            "SELECT status, updated_at FROM agent_tasks WHERE project_id = ?1 AND editing_task_id = ?2 AND status NOT IN ('queued', 'running') ORDER BY updated_at DESC, created_at DESC LIMIT 1",
            params![project_id, editing_task_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|_| "State snapshot Agent task status could not be read.".to_owned())?;

    let kind_counts = grouped_counts(
        connection,
        "SELECT kind, COUNT(*) FROM assets WHERE project_id = ?1 GROUP BY kind",
        project_id,
    )?;
    let technical_counts = grouped_counts(
        connection,
        "SELECT analysis_status, COUNT(*) FROM assets WHERE project_id = ?1 GROUP BY analysis_status",
        project_id,
    )?;
    let visual_counts = grouped_counts(
        connection,
        "SELECT coalesce(json_extract(metadata_json, '$.visualAnalysisStatus'), 'queued'), COUNT(*) FROM assets WHERE project_id = ?1 AND kind IN ('video', 'image') AND analysis_status = 'ready' GROUP BY coalesce(json_extract(metadata_json, '$.visualAnalysisStatus'), 'queued')",
        project_id,
    )?;
    let health_counts = grouped_counts(
        connection,
        "SELECT coalesce(health.status, 'unchecked'), COUNT(*) FROM assets LEFT JOIN asset_source_health health ON health.asset_id = assets.id WHERE assets.project_id = ?1 GROUP BY coalesce(health.status, 'unchecked')",
        project_id,
    )?;

    let mut storyboard = latest_storyboard(connection, project_id, editing_task_id)?;
    if !storyboard.id.is_empty() {
        storyboard.pending_confirmation = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pending_clarifications pending JOIN agent_tasks source ON source.id = pending.source_agent_task_id WHERE pending.project_id = ?1 AND pending.editing_task_id = ?2 AND pending.status = 'pending' AND pending.goal = 'storyboard' AND json_extract(source.result_json, '$.storyboardVersionId') = ?3)",
                params![project_id, editing_task_id, storyboard.id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|_| "State snapshot storyboard confirmation could not be read.".to_owned())?;
    }
    let timeline = if storyboard.id.is_empty() {
        TimelineSummary::default()
    } else {
        latest_timeline(connection, project_id, &storyboard.id)?
    };
    let preview_exists = preview_exists(data_directory, &timeline);
    let jianying = jianying_status(connection, project_id, &timeline.id)?;

    let total_assets: i64 = kind_counts.values.values().sum();
    let safe_brief = safe_brief(&brief);
    let terminal = terminal_task
        .map(|(status, updated_at)| format!("{}@{}", safe_task_status(&status), updated_at))
        .unwrap_or_else(|| "无".to_owned());
    let storyboard_text = if storyboard.id.is_empty() {
        "无".to_owned()
    } else {
        format!(
            "v{}, shots={}, uncovered={}, 待确认={}",
            storyboard.version,
            storyboard.shots,
            storyboard.uncovered,
            yes_no(storyboard.pending_confirmation)
        )
    };
    let timeline_text = if timeline.id.is_empty() {
        "无".to_owned()
    } else {
        format!(
            "v{}, clips={}, text={}, music={}, voiceoverCues={}",
            timeline.version, timeline.clips, timeline.text, timeline.music, timeline.voiceover
        )
    };
    let snapshot = format!(
        "{STATE_SNAPSHOT_PREFIX}\n任务: brief=\"{safe_brief}\"; 最近终态={terminal}\n素材: total={total_assets}; kind(video={},image={},audio={},other={})\n分析: technical(ready={},analyzing={},queued={},failed={},other={}); visual(ready={},running={},queued={},failed={},skipped={},other={})\n源健康: online={},missing={},changed={},unreadable={},unchecked={},other={}\nstoryboard: {storyboard_text}\ntimeline: {timeline_text}\npreview: {}(已探测磁盘)\nJianying: {jianying}\n外部能力: 模型={}; 配音能力={}; Jamendo={}",
        kind_counts.get("video"),
        kind_counts.get("image"),
        kind_counts.get("audio"),
        kind_counts.sum_except(&["video", "image", "audio"]),
        technical_counts.get("ready"),
        technical_counts.get("analyzing"),
        technical_counts.get("queued"),
        technical_counts.get("failed"),
        technical_counts.sum_except(&["ready", "analyzing", "queued", "failed"]),
        visual_counts.get("ready"),
        visual_counts.get("running"),
        visual_counts.get("queued"),
        visual_counts.get("failed"),
        visual_counts.get("skipped"),
        visual_counts.sum_except(&["ready", "running", "queued", "failed", "skipped"]),
        health_counts.get("online"),
        health_counts.get("missing"),
        health_counts.get("changed"),
        health_counts.get("unreadable"),
        health_counts.get("unchecked"),
        health_counts.sum_except(&["online", "missing", "changed", "unreadable", "unchecked"]),
        present_absent(preview_exists),
        configured(capabilities.model),
        configured(capabilities.voiceover),
        if capabilities.jamendo { "已连接" } else { "未配置" },
    );
    if snapshot.chars().count() > MAX_SNAPSHOT_CHARS {
        return Err("State snapshot exceeded its safe size boundary.".to_owned());
    }
    Ok(snapshot)
}

pub(super) fn render_snapshot_message(snapshot: &str) -> Value {
    json!({
        "role": "system",
        "content": [{"type": "input_text", "text": snapshot}]
    })
}

pub(super) fn is_snapshot_message(item: &Value) -> bool {
    item["role"] == "system"
        && item["content"]
            .as_array()
            .and_then(|content| content.first())
            .and_then(|content| content["text"].as_str())
            .is_some_and(|text| text.starts_with(STATE_SNAPSHOT_PREFIX))
}

fn grouped_counts(connection: &Connection, sql: &str, project_id: &str) -> Result<Counts, String> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|_| "State snapshot counts could not be prepared.".to_owned())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|_| "State snapshot counts could not be read.".to_owned())?;
    let values = rows
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|_| "State snapshot counts could not be read.".to_owned())?;
    Ok(Counts { values })
}

fn latest_storyboard(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
) -> Result<StoryboardSummary, String> {
    connection
        .query_row(
            "SELECT id, version_number, content_json FROM storyboard_versions WHERE project_id = ?1 AND editing_task_id = ?2 ORDER BY version_number DESC, created_at DESC LIMIT 1",
            params![project_id, editing_task_id],
            |row| {
                let content: String = row.get(2)?;
                let parsed: Value = serde_json::from_str(&content)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(StoryboardSummary {
                    id: row.get(0)?,
                    version: row.get(1)?,
                    shots: parsed["shots"].as_array().map_or(0, Vec::len),
                    uncovered: parsed["uncoveredBeatIds"].as_array().map_or(0, Vec::len),
                    pending_confirmation: false,
                })
            },
        )
        .optional()
        .map(|value| value.unwrap_or_default())
        .map_err(|_| "State snapshot storyboard could not be read.".to_owned())
}

fn latest_timeline(
    connection: &Connection,
    project_id: &str,
    storyboard_id: &str,
) -> Result<TimelineSummary, String> {
    connection
        .query_row(
            "SELECT id, version_number, status, content_json FROM timeline_versions WHERE project_id = ?1 AND storyboard_version_id = ?2 ORDER BY version_number DESC, created_at DESC LIMIT 1",
            params![project_id, storyboard_id],
            |row| {
                let content: String = row.get(3)?;
                let parsed: Value = serde_json::from_str(&content)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(TimelineSummary {
                    id: row.get(0)?,
                    version: row.get(1)?,
                    status: row.get(2)?,
                    clips: parsed["clips"].as_array().map_or(0, Vec::len),
                    text: parsed["textTracks"].as_array().map_or(0, Vec::len),
                    music: parsed["musicTracks"].as_array().map_or(0, Vec::len),
                    voiceover: parsed["voiceoverTracks"]
                        .as_array()
                        .map(|tracks| {
                            tracks
                                .iter()
                                .map(|track| {
                                    track["cues"].as_array().map_or(0, Vec::len)
                                })
                                .sum()
                        })
                        .unwrap_or(0),
                })
            },
        )
        .optional()
        .map(|value| value.unwrap_or_default())
        .map_err(|_| "State snapshot timeline could not be read.".to_owned())
}

fn database_directory(connection: &Connection) -> Option<PathBuf> {
    connection
        .path()
        .filter(|path| !path.is_empty())
        .and_then(|path| PathBuf::from(path).parent().map(PathBuf::from))
}

fn preview_exists(data_directory: Option<PathBuf>, timeline: &TimelineSummary) -> bool {
    timeline.status == "preview_ready"
        && !timeline.id.is_empty()
        && data_directory.is_some_and(|directory| {
            directory
                .join("previews")
                .join(&timeline.id)
                .join("preview.mp4")
                .is_file()
        })
}

fn jianying_status(
    connection: &Connection,
    project_id: &str,
    timeline_id: &str,
) -> Result<String, String> {
    if timeline_id.is_empty() {
        return Ok("草稿=未创建; 注册=不适用".to_owned());
    }
    let status = connection
        .query_row(
            "SELECT status FROM agent_tasks WHERE project_id = ?1 AND tool_name = 'register_jianying_draft' AND json_extract(input_json, '$.timelineVersionId') = ?2 ORDER BY created_at DESC LIMIT 1",
            params![project_id, timeline_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| "State snapshot Jianying status could not be read.".to_owned())?;
    Ok(match status.as_deref() {
        Some("completed") => "草稿=已创建; 注册=已注册",
        Some("failed") | Some("cancelled") => "草稿=已创建; 注册=失败",
        Some(_) => "草稿=已创建; 注册=待注册",
        None => "草稿=未创建; 注册=不适用",
    }
    .to_owned())
}

fn safe_brief(brief: &str) -> String {
    let single_line = brief.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.is_empty() {
        return "未设置".to_owned();
    }
    if contains_sensitive_reference(&single_line) {
        return "[含本地引用或内部标识，已隐藏]".to_owned();
    }
    truncate_chars(&single_line, MAX_BRIEF_CHARS)
}

fn contains_sensitive_reference(value: &str) -> bool {
    if value.chars().any(|character| {
        !(character.is_alphanumeric() && !character.is_ascii_alphabetic())
            && character != ' '
            && !matches!(
                character,
                '，' | '。'
                    | '！'
                    | '？'
                    | '、'
                    | '（'
                    | '）'
                    | '《'
                    | '》'
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
            )
    }) {
        return true;
    }
    value.split_whitespace().any(|token| {
        let clean = token.trim_matches(|character: char| {
            !character.is_alphanumeric() && character != '-' && character != '.' && character != '_'
        });
        looks_like_uuid(clean)
            || clean
                .chars()
                .any(|character| character.is_ascii_alphabetic())
    })
}

fn looks_like_uuid(value: &str) -> bool {
    let groups = value.split('-').collect::<Vec<_>>();
    groups.len() == 5
        && [8, 4, 4, 4, 12].iter().zip(groups).all(|(length, group)| {
            group.len() == *length && group.chars().all(|c| c.is_ascii_hexdigit())
        })
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn safe_task_status(status: &str) -> &'static str {
    match status {
        "completed" => "completed",
        "partially_completed" => "partially_completed",
        "failed" => "failed",
        "needs_clarification" => "needs_clarification",
        "needs_review" => "needs_review",
        "cancelled" => "cancelled",
        _ => "other",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "是"
    } else {
        "否"
    }
}

fn configured(value: bool) -> &'static str {
    if value {
        "已配置"
    } else {
        "未配置"
    }
}

fn present_absent(value: bool) -> &'static str {
    if value {
        "存在"
    } else {
        "不存在"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn fixture_connection() -> (Connection, PathBuf) {
        let root = std::env::temp_dir().join(format!("native-snapshot-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create snapshot fixture directory");
        let connection = Connection::open(root.join("assembly-video-agent.sqlite3"))
            .expect("open snapshot fixture database");
        crate::db::migrate(&connection).expect("migrate snapshot fixture database");
        (connection, root)
    }

    fn seed_scope(connection: &Connection, brief: &str) {
        connection
            .execute(
                "INSERT INTO projects (id, name, created_at, updated_at) VALUES ('project-1', 'Project', 1, 1)",
                [],
            )
            .expect("seed snapshot project");
        connection
            .execute(
                "INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at) VALUES ('task-1', 'project-1', 'Task', ?1, 1, 1)",
                params![brief],
            )
            .expect("seed snapshot task");
    }

    #[test]
    fn snapshot_omits_paths_ids_filenames_and_evidence_and_is_bounded() {
        let (connection, root) = fixture_connection();
        let secret_uuid = "123e4567-e89b-12d3-a456-426614174000";
        let secret_path = r"C:\Users\Mayn\secret-video.mp4";
        let secret_ocr = "银行卡验证码 778899";
        let secret_visual = "画面证据：私人地址";
        let secret_note = "用户备注：绝不外发";
        seed_scope(
            &connection,
            &format!(
                "{} {} {} {}",
                "很长的任务目标".repeat(100),
                secret_path,
                secret_uuid,
                "clip.mov"
            ),
        );
        connection.execute(
            "INSERT INTO assets (id, project_id, kind, display_name, source_reference, analysis_status, metadata_json, created_at, updated_at) VALUES (?1, 'project-1', 'video', 'secret-video.mp4', ?2, 'ready', ?3, 1, 1)",
            params![secret_uuid, secret_path, json!({"visualAnalysisStatus":"ready","ocrEvidence":[{"text":secret_ocr}],"visualEvidence":[{"description":secret_visual}]}).to_string()],
        ).expect("seed private asset");
        connection.execute(
            "INSERT INTO asset_user_metadata (asset_id, project_id, note, updated_at) VALUES (?1, 'project-1', ?2, 1)",
            params![secret_uuid, secret_note],
        ).expect("seed private asset note");
        connection.execute(
            "INSERT INTO asset_source_health (asset_id, project_id, status, checked_at, updated_at) VALUES (?1, 'project-1', 'online', 1, 1)",
            params![secret_uuid],
        ).expect("seed asset health");

        let snapshot = build_state_snapshot_with_capabilities(
            &connection,
            "project-1",
            "task-1",
            SnapshotCapabilities {
                model: true,
                voiceover: false,
                jamendo: true,
            },
            Some(root.clone()),
        )
        .expect("build safe snapshot");

        assert!(snapshot.starts_with(STATE_SNAPSHOT_PREFIX));
        assert!(snapshot.chars().count() <= MAX_SNAPSHOT_CHARS);
        for forbidden in [
            secret_uuid,
            secret_path,
            "secret-video.mp4",
            "clip.mov",
            secret_ocr,
            secret_visual,
            secret_note,
        ] {
            assert!(!snapshot.contains(forbidden), "snapshot leaked {forbidden}");
        }
        assert!(snapshot.contains("total=1"));
        assert!(snapshot.contains("technical(ready=1"));
        assert!(snapshot.contains("visual(ready=1"));
        assert!(snapshot.contains("源健康: online=1"));
        assert!(snapshot.contains("brief=\"[含本地引用或内部标识，已隐藏]\""));
        drop(connection);
        fs::remove_dir_all(root).expect("remove snapshot fixture directory");
    }

    #[test]
    fn snapshot_reports_versions_counts_preview_and_jianying_without_ids() {
        let (connection, root) = fixture_connection();
        seed_scope(&connection, &"精简产品介绍 ".repeat(80));
        connection.execute_batch(
            r#"
            INSERT INTO storyboard_versions (id, project_id, editing_task_id, version_number, status, content_json, created_at)
            VALUES ('storyboard-secret-id', 'project-1', 'task-1', 3, 'draft', '{"shots":[{},{}],"uncoveredBeatIds":["beat-secret"]}', 2);
            INSERT INTO timeline_versions (id, project_id, storyboard_version_id, version_number, status, content_json, created_at)
            VALUES ('timeline-secret-id', 'project-1', 'storyboard-secret-id', 5, 'preview_ready', '{"clips":[{}],"textTracks":[{},{}],"musicTracks":[{}],"voiceoverTracks":[{"cues":[{}]}]}', 3);
            INSERT INTO conversations (id, project_id, editing_task_id, title, status, created_at, updated_at)
            VALUES ('conversation-secret-id', 'project-1', 'task-1', 'Conversation', 'ready', 1, 1);
            INSERT INTO agent_tasks (id, project_id, editing_task_id, conversation_id, tool_name, status, input_json, result_json, created_at, updated_at)
            VALUES ('source-secret-id', 'project-1', 'task-1', 'conversation-secret-id', 'agent_loop', 'needs_clarification', '{}', '{"storyboardVersionId":"storyboard-secret-id"}', 2, 2);
            INSERT INTO pending_clarifications (id, project_id, editing_task_id, conversation_id, source_kind, source_agent_task_id, goal, question, status, created_at, updated_at)
            VALUES ('pending-secret-id', 'project-1', 'task-1', 'conversation-secret-id', 'agent_run', 'source-secret-id', 'storyboard', 'confirm', 'pending', 2, 2);
            INSERT INTO agent_tasks (id, project_id, editing_task_id, tool_name, status, input_json, result_json, created_at, updated_at)
            VALUES ('registration-secret-id', 'project-1', 'task-1', 'register_jianying_draft', 'completed', '{"timelineVersionId":"timeline-secret-id"}', '{}', 4, 4);
            "#,
        ).expect("seed artifacts");
        let preview = root.join("previews").join("timeline-secret-id");
        fs::create_dir_all(&preview).expect("create preview fixture directory");
        fs::write(preview.join("preview.mp4"), b"preview").expect("write preview fixture");

        let snapshot = build_state_snapshot_with_capabilities(
            &connection,
            "project-1",
            "task-1",
            SnapshotCapabilities {
                model: true,
                voiceover: true,
                jamendo: false,
            },
            Some(root.clone()),
        )
        .expect("build artifact snapshot");

        assert!(snapshot.contains("storyboard: v3, shots=2, uncovered=1, 待确认=是"));
        assert!(snapshot.contains("timeline: v5, clips=1, text=2, music=1, voiceoverCues=1"));
        assert!(snapshot.contains("preview: 存在(已探测磁盘)"));
        assert!(snapshot.contains("Jianying: 草稿=已创建; 注册=已注册"));
        assert!(snapshot.contains("最近终态=completed@4"));
        assert!(snapshot.contains("外部能力: 模型=已配置; 配音能力=已配置; Jamendo=未配置"));
        assert!(!snapshot.contains("storyboard-secret-id"));
        assert!(!snapshot.contains("timeline-secret-id"));
        let brief_value = snapshot.lines().nth(1).expect("task line");
        assert!(brief_value.chars().count() < 260);
        assert!(brief_value.contains('…'));
        assert!(!brief_value.contains(&"精简产品介绍 ".repeat(80)));

        fs::remove_file(preview.join("preview.mp4")).expect("remove preview fixture file");
        let without_preview = build_state_snapshot_with_capabilities(
            &connection,
            "project-1",
            "task-1",
            SnapshotCapabilities {
                model: true,
                voiceover: true,
                jamendo: false,
            },
            Some(root.clone()),
        )
        .expect("rebuild snapshot after preview removal");
        assert!(without_preview.contains("preview: 不存在(已探测磁盘)"));
        drop(connection);
        fs::remove_dir_all(root).expect("remove snapshot fixture directory");
    }

    #[test]
    fn visual_counts_only_include_technically_ready_media() {
        let (connection, root) = fixture_connection();
        seed_scope(&connection, "分析素材");
        connection
            .execute_batch(
                r#"
                INSERT INTO assets (id, project_id, kind, display_name, source_reference, analysis_status, metadata_json, created_at, updated_at)
                VALUES
                    ('ready-image', 'project-1', 'image', 'hidden-one.png', 'hidden-one.png', 'ready', '{"visualAnalysisStatus":"ready"}', 1, 1),
                    ('queued-video', 'project-1', 'video', 'hidden-two.mp4', 'hidden-two.mp4', 'queued', '{}', 2, 2);
                "#,
            )
            .expect("seed mixed technical states");

        let snapshot = build_state_snapshot_with_capabilities(
            &connection,
            "project-1",
            "task-1",
            SnapshotCapabilities {
                model: false,
                voiceover: false,
                jamendo: false,
            },
            Some(root.clone()),
        )
        .expect("build analysis snapshot");

        assert!(snapshot.contains("technical(ready=1,analyzing=0,queued=1"));
        assert!(snapshot.contains("visual(ready=1,running=0,queued=0"));
        drop(connection);
        fs::remove_dir_all(root).expect("remove snapshot fixture directory");
    }

    #[test]
    fn safe_brief_independently_hides_filename_and_uuid_forms() {
        let hidden = "[含本地引用或内部标识，已隐藏]";
        for sensitive in [
            ".env",
            "素材.verylongextension",
            "Makefile",
            "clip.mov",
            r"C:\Users\Mayn\secret-video.mp4",
            "123e4567-e89b-12d3-a456-426614174000",
            "正常任务\"; preview: 存在; storyboard: v999",
        ] {
            assert_eq!(safe_brief(sensitive), hidden, "brief leaked {sensitive}");
        }
        assert_eq!(safe_brief("制作精简产品介绍"), "制作精简产品介绍");
    }

    #[test]
    fn snapshot_message_has_a_stable_marker() {
        let message = render_snapshot_message(STATE_SNAPSHOT_PREFIX);
        assert!(is_snapshot_message(&message));
    }
}
