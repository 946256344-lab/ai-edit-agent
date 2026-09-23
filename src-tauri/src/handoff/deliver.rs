//! 输出端口：列出链接器、记住项目选择、按选择交付。

use super::{
    all_editor_ids, build_handoff_plan, editor_capabilities, fcpxml, otio, posix_media_path,
    DeliveryKind, EditorId, HandoffSource,
};
use crate::capcut;
use crate::db::{now_millis, open_connection};
use crate::jianying::{create_jianying_draft, draft_location_available};
use crate::models::{JianyingDraftResult, TimelineVersion};
use crate::timeline::load_timeline_version;
use rusqlite::params;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorLinkerInfo {
    pub id: String,
    pub label: String,
    pub summary: String,
    pub implemented: bool,
    pub available: bool,
    pub delivery_kind: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorLinkerCatalog {
    pub selected_id: String,
    pub linkers: Vec<EditorLinkerInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorDeliveryResult {
    pub editor_id: String,
    pub delivery_kind: String,
    pub status: String,
    pub display_name: String,
    pub message: String,
    pub output_path: Option<String>,
    pub jianying: Option<JianyingDraftResult>,
}

#[tauri::command]
pub fn list_editor_linkers(
    app: AppHandle,
    project_id: String,
) -> Result<EditorLinkerCatalog, String> {
    let connection = open_connection(&app)?;
    ensure_project(&connection, &project_id)?;
    let selected = read_output_editor(&connection, &project_id);
    let jianying_available = draft_location_available();
    let capcut_available = capcut::draft_location_available();
    Ok(EditorLinkerCatalog {
        selected_id: selected.as_str().to_owned(),
        linkers: all_editor_ids()
            .into_iter()
            .map(|id| {
                let caps = editor_capabilities(id);
                EditorLinkerInfo {
                    id: id.as_str().to_owned(),
                    label: id.label().to_owned(),
                    summary: id.summary().to_owned(),
                    implemented: caps.implemented,
                    available: match id {
                        EditorId::Jianying => jianying_available,
                        EditorId::CapCut => capcut_available,
                        EditorId::Fcpxml | EditorId::Otio => caps.implemented,
                    },
                    delivery_kind: match caps.delivery {
                        DeliveryKind::DropInDraft => "dropInDraft".to_owned(),
                        DeliveryKind::ImportFile => "importFile".to_owned(),
                    },
                }
            })
            .collect(),
    })
}

#[tauri::command]
pub fn set_output_editor(
    app: AppHandle,
    project_id: String,
    editor_id: String,
) -> Result<EditorLinkerCatalog, String> {
    let editor = EditorId::parse(&editor_id)?;
    if !editor.implemented() {
        return Err(format!("{}输出尚未实现。", editor.label()));
    }
    let connection = open_connection(&app)?;
    ensure_project(&connection, &project_id)?;
    let mut settings = read_settings(&connection, &project_id);
    settings["outputEditor"] = serde_json::Value::String(editor.as_str().to_owned());
    connection
        .execute(
            "UPDATE projects SET settings_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![settings.to_string(), now_millis(), project_id],
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    list_editor_linkers(app, project_id)
}

#[tauri::command]
pub fn deliver_to_editor(
    app: AppHandle,
    timeline_version_id: String,
    editor_id: Option<String>,
) -> Result<EditorDeliveryResult, String> {
    let connection = open_connection(&app)?;
    let timeline = load_timeline_version(&connection, &timeline_version_id)?;
    let editor = match editor_id.as_deref() {
        Some(raw) => EditorId::parse(raw)?,
        None => read_output_editor(&connection, &timeline.project_id),
    };
    if !editor.implemented() {
        return Err(format!("{}输出尚未实现。", editor.label()));
    }
    drop(connection);
    match editor {
        EditorId::Jianying => wrap_jianying(create_jianying_draft(app, timeline_version_id)?),
        EditorId::CapCut => wrap_capcut(capcut::create_capcut_draft(app, timeline_version_id)?),
        EditorId::Fcpxml | EditorId::Otio => write_import_file(app, &timeline, editor),
    }
}

fn wrap_jianying(draft: JianyingDraftResult) -> Result<EditorDeliveryResult, String> {
    let display_name = Path::new(&draft.draft_directory)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("剪映草稿")
        .to_owned();
    let pending = draft.registration_status == "pending";
    Ok(EditorDeliveryResult {
        editor_id: EditorId::Jianying.as_str().to_owned(),
        delivery_kind: "dropInDraft".to_owned(),
        status: draft.registration_status.clone(),
        message: if pending {
            format!("草稿「{display_name}」已写好，剪映正在运行，退出后会自动完成注册。")
        } else {
            format!("草稿「{display_name}」已生成，可在剪映本地草稿中打开。")
        },
        display_name,
        output_path: Some(draft.draft_directory.clone()),
        jianying: Some(draft),
    })
}

fn wrap_capcut(draft: JianyingDraftResult) -> Result<EditorDeliveryResult, String> {
    let display_name = Path::new(&draft.draft_directory)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("CapCut 草稿")
        .to_owned();
    let pending = draft.registration_status == "pending";
    Ok(EditorDeliveryResult {
        editor_id: EditorId::CapCut.as_str().to_owned(),
        delivery_kind: "dropInDraft".to_owned(),
        status: draft.registration_status.clone(),
        message: if pending {
            format!("草稿「{display_name}」已写好，CapCut 正在运行，退出后会自动完成注册。")
        } else {
            format!("草稿「{display_name}」已生成，可在 CapCut 本地草稿中打开。")
        },
        display_name,
        output_path: Some(draft.draft_directory.clone()),
        jianying: None,
    })
}

fn write_import_file(
    app: AppHandle,
    timeline: &TimelineVersion,
    editor: EditorId,
) -> Result<EditorDeliveryResult, String> {
    let connection = open_connection(&app)?;
    let sources = collect_export_sources(&connection, timeline, true)?;
    let plan = build_handoff_plan(timeline, &sources)?;
    let stem = unique_export_stem(&connection, &timeline.project_id);
    drop(connection);
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("editor-handoffs")
        .join(&timeline.project_id);
    fs::create_dir_all(&directory).map_err(|_| "无法准备编辑器导出目录。".to_owned())?;
    let (filename, contents) = match editor {
        EditorId::Fcpxml => (
            format!("{stem}.fcpxml"),
            fcpxml::render_fcpxml(&plan, &stem).into_bytes(),
        ),
        EditorId::Otio => (
            format!("{stem}.otio"),
            serde_json::to_vec_pretty(&otio::render_otio(&plan, &stem))
                .map_err(|error| error.to_string())?,
        ),
        _ => return Err("该编辑器不是文件导出。".to_owned()),
    };
    let output_path = unique_path(&directory, &filename);
    fs::write(&output_path, contents).map_err(|_| "无法写出编辑器时间线文件。".to_owned())?;
    let display_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&filename)
        .to_owned();
    Ok(EditorDeliveryResult {
        editor_id: editor.as_str().to_owned(),
        delivery_kind: "importFile".to_owned(),
        status: "written".to_owned(),
        message: format!("已导出「{display_name}」。{}", editor.summary()),
        display_name,
        output_path: Some(output_path.to_string_lossy().replace('\\', "/")),
        jianying: None,
    })
}

pub(crate) fn collect_export_sources(
    connection: &rusqlite::Connection,
    timeline: &TimelineVersion,
    include_voiceover: bool,
) -> Result<HashMap<String, HandoffSource>, String> {
    let mut sources = HashMap::new();
    for (index, clip) in timeline.clips.iter().enumerate() {
        insert_source(
            connection,
            &mut sources,
            &timeline.project_id,
            &clip.asset_id,
            false,
            "video",
            &format!(
                "Timeline source media file {} is unavailable. Re-import or relink the missing asset before exporting.",
                index + 1
            ),
        )?;
    }
    for (index, clip) in timeline.overlay_clips.iter().enumerate() {
        insert_source(
            connection,
            &mut sources,
            &timeline.project_id,
            &clip.asset_id,
            false,
            "video",
            &format!("Overlay source media file {} is unavailable.", index + 1),
        )?;
    }
    for track in &timeline.music_tracks {
        for cue in &track.cues {
            insert_source(
                connection,
                &mut sources,
                &timeline.project_id,
                &cue.asset_id,
                true,
                "audio",
                "Music source media is unavailable. Re-import or relink it before exporting.",
            )?;
        }
    }
    if include_voiceover {
        for track in &timeline.voiceover_tracks {
            for cue in &track.cues {
                insert_source(
                    connection,
                    &mut sources,
                    &timeline.project_id,
                    &cue.asset_id,
                    false,
                    "audio",
                    "Voiceover media is unavailable. Re-generate the narration before exporting.",
                )?;
            }
        }
    }
    Ok(sources)
}

fn insert_source(
    connection: &rusqlite::Connection,
    sources: &mut HashMap<String, HandoffSource>,
    project_id: &str,
    asset_id: &str,
    require_ready: bool,
    expected_kind: &str,
    missing_file: &str,
) -> Result<(), String> {
    if sources.contains_key(asset_id) {
        return Ok(());
    }
    let (source_reference, kind) = lookup_asset(connection, project_id, asset_id, require_ready)?;
    if kind != expected_kind {
        return Err(format!(
            "Export currently supports {expected_kind} media for this track."
        ));
    }
    if !Path::new(&source_reference).is_file() {
        return Err(missing_file.to_owned());
    }
    sources.insert(
        asset_id.to_owned(),
        HandoffSource {
            kind,
            path: posix_media_path(&source_reference),
        },
    );
    Ok(())
}

fn lookup_asset(
    connection: &rusqlite::Connection,
    project_id: &str,
    asset_id: &str,
    require_ready: bool,
) -> Result<(String, String), String> {
    if require_ready {
        connection
            .query_row(
                "SELECT source_reference, kind FROM assets WHERE id = ?1 AND id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?2) AND analysis_status = 'ready'",
                params![asset_id, project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Music asset is unavailable or has not finished analysis.".to_owned())
    } else {
        connection
            .query_row(
                "SELECT source_reference, kind FROM assets WHERE id = ?1 AND id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?2)",
                params![asset_id, project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Timeline references an unavailable asset.".to_owned())
    }
}

fn ensure_project(connection: &rusqlite::Connection, project_id: &str) -> Result<(), String> {
    let exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if exists == 0 {
        return Err("Project was not found.".to_owned());
    }
    Ok(())
}

fn read_settings(connection: &rusqlite::Connection, project_id: &str) -> serde_json::Value {
    connection
        .query_row(
            "SELECT settings_json FROM projects WHERE id = ?1",
            params![project_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn read_output_editor(connection: &rusqlite::Connection, project_id: &str) -> EditorId {
    read_settings(connection, project_id)
        .get("outputEditor")
        .and_then(|value| value.as_str())
        .and_then(|raw| EditorId::parse(raw).ok())
        .filter(|id| id.implemented())
        .unwrap_or(EditorId::Jianying)
}

fn unique_export_stem(connection: &rusqlite::Connection, project_id: &str) -> String {
    let project_name = connection
        .query_row(
            "SELECT name FROM projects WHERE id = ?1",
            params![project_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .map(|name| sanitize_stem(&name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Assembly".to_owned());
    let suffix = &Uuid::new_v4().to_string()[..8];
    format!("{project_name}-{suffix}")
}

fn sanitize_stem(raw: &str) -> String {
    let cleaned = raw
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>();
    let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed.chars().take(36).collect::<String>()
}

fn unique_path(directory: &Path, filename: &str) -> PathBuf {
    let candidate = directory.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("export");
    let ext = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    directory.join(format!("{stem}-{}.{ext}", &Uuid::new_v4().to_string()[..6]))
}
