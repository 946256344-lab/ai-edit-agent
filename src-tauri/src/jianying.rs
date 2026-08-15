//! Jianying draft 的单向创建、回滚与延迟注册边界。
//! 每次交付创建唯一新目录，绝不覆盖或反向同步已有 Jianying 项目。

use crate::db::{now_millis, open_connection};
use crate::models::{JianyingDraftResult, JianyingRegistrationStatus, TimelineVersion};
use crate::process::hidden_command;
use crate::timeline::load_timeline_version;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

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

fn process_pending_jianying_registrations(app: &AppHandle) -> Result<bool, String> {
    if jianying_process_is_running() {
        return Ok(false);
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
    if tasks.is_empty() {
        return Ok(false);
    }

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
    Ok(true)
}

pub(crate) fn resume_pending_jianying_registrations(app: &AppHandle) -> Result<(), String> {
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
        let idle = match process_pending_jianying_registrations(&app) {
            Ok(did_work) => !did_work,
            Err(error) => {
                log::warn!("Pending Jianying registration worker failed: {error}");
                true
            }
        };
        thread::sleep(if idle {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(2)
        });
    });
    Ok(())
}

fn text_tracks_are_ready_for_jianying(timeline: &TimelineVersion) -> bool {
    timeline
        .text_tracks
        .iter()
        .flat_map(|track| &track.cues)
        .all(|cue| cue.jianying_compatibility == "verified")
}

#[tauri::command]
pub fn create_jianying_draft(
    app: AppHandle,
    timeline_version_id: String,
) -> Result<JianyingDraftResult, String> {
    log::info!("Starting Jianying draft creation.");
    let connection = open_connection(&app)?;
    let timeline = load_timeline_version(&connection, &timeline_version_id)?;
    if !text_tracks_are_ready_for_jianying(&timeline) {
        return Err(
            "This timeline has text tracks that are not yet verified for Jianying draft delivery. Use only the verified default-font static, fade in/out, slide-up, slide-down, or pop templates, or render a local preview."
                .to_owned(),
        );
    }
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
    let mut music_tracks = Vec::with_capacity(timeline.music_tracks.len());
    for track in &timeline.music_tracks {
        let mut cues = Vec::with_capacity(track.cues.len());
        for cue in &track.cues {
            let (source_reference, kind): (String, String) = connection
                .query_row(
                    "SELECT source_reference, kind FROM assets WHERE id = ?1 AND project_id = ?2 AND analysis_status = 'ready'",
                    params![cue.asset_id, timeline.project_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| "Music asset is unavailable or has not finished analysis.".to_owned())?;
            if kind != "audio" || !Path::new(&source_reference).is_file() {
                return Err("Music source media is unavailable. Re-import or relink it before creating a Jianying draft.".to_owned());
            }
            let mut cue_json = serde_json::to_value(cue).map_err(|error| error.to_string())?;
            let cue_json = cue_json
                .as_object_mut()
                .ok_or_else(|| "Could not prepare the music cue for Jianying.".to_owned())?;
            cue_json.insert(
                "sourceReference".to_owned(),
                serde_json::Value::String(source_reference.replace('\\', "/")),
            );
            cues.push(serde_json::Value::Object(cue_json.clone()));
        }
        music_tracks.push(serde_json::json!({
            "id": track.id,
            "enabled": track.enabled,
            "cues": cues,
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
        "inputFormatVersion": 2,
        "operation": "createDraft",
        "draftRoot": draft_root,
        "draftName": draft_name,
        "draftRegistryPath": draft_registry_path,
        "clips": clips,
        "textTracks": timeline.text_tracks,
        "musicTracks": music_tracks
    });
    let result = run_jianying_adapter(&app, &input).map_err(|error| {
        log::error!("Jianying draft adapter failed.");
        format!("Jianying draft adapter could not create a draft: {error}")
    })?;
    let registration = PendingJianyingRegistration {
        input_format_version: 2,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{TextCue, TextLayout, TextStyle, TextTrack};

    fn test_timeline(compatibility: &str) -> TimelineVersion {
        TimelineVersion {
            id: "timeline-1".to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number: 1,
            clips: Vec::new(),
            text_tracks: vec![TextTrack {
                id: "text-1".to_owned(),
                role: "subtitle".to_owned(),
                layer: 1,
                enabled: true,
                cues: vec![TextCue {
                    id: "cue-1".to_owned(),
                    template_id: None,
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "字幕".to_owned(),
                    style: TextStyle {
                        font_key: "jianying_default".to_owned(),
                        font_size: 0.06,
                        bold: false,
                        color: "#FFFFFF".to_owned(),
                        stroke_color: None,
                        stroke_width: 0.0,
                        shadow: false,
                        background_color: None,
                        alignment: "center".to_owned(),
                        letter_spacing: 0,
                        line_spacing: 0,
                    },
                    layout: TextLayout {
                        anchor: "bottom".to_owned(),
                        x: 0.5,
                        y: 0.82,
                        max_width: 0.8,
                        safe_area: "title_safe".to_owned(),
                    },
                    entrance: None,
                    exit: None,
                    loop_animation: None,
                    jianying_compatibility: compatibility.to_owned(),
                }],
            }],
            music_tracks: Vec::new(),
            quality_report: None,
            created_at: 1,
        }
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
    fn jianying_delivery_rejects_local_preview_only_text() {
        assert!(text_tracks_are_ready_for_jianying(&test_timeline(
            "verified"
        )));
        assert!(!text_tracks_are_ready_for_jianying(&test_timeline(
            "local_preview_only"
        )));
    }

    #[test]
    fn jianying_input_preserves_unicode_text() {
        let payload = serde_json::json!({
            "textTracks": test_timeline("verified").text_tracks,
        });

        assert_eq!(
            payload["textTracks"][0]["cues"][0]["text"],
            "\u{5b57}\u{5e55}"
        );
    }
}
