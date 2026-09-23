//! CapCut 链接器：按本机草稿注册表识别库位置，只新建不覆盖、不反向同步。

use crate::db::{now_millis, open_connection};
use crate::handoff::deliver::collect_export_sources;
use crate::handoff::{build_handoff_plan, capcut_create_draft_input, JianyingDraftDestination};
use crate::jianying::{
    find_lveditor_draft_location, run_jianying_adapter, text_tracks_are_ready_for_jianying,
    unique_draft_name,
};
use crate::models::JianyingDraftResult;
use crate::process::hidden_command;
use crate::timeline::load_timeline_version;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingCapCutRegistration {
    input_format_version: i64,
    operation: String,
    editor: String,
    draft_root: String,
    draft_name: String,
    draft_registry_path: String,
    draft_directory: String,
    duration_ms: i64,
    timeline_version_id: String,
}

fn capcut_process_is_running() -> bool {
    hidden_command("tasklist")
        .args(["/FI", "IMAGENAME eq CapCut.exe", "/FO", "CSV", "/NH"])
        .output()
        .is_ok_and(|output| {
            output
                .stdout
                .windows(b"capcut.exe".len())
                .any(|window| window.eq_ignore_ascii_case(b"capcut.exe"))
        })
}

pub(crate) fn draft_location_available() -> bool {
    find_lveditor_draft_location("CapCut").is_some()
}

fn record_capcut_registration_task(
    app: &AppHandle,
    project_id: &str,
    input: &PendingCapCutRegistration,
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
            VALUES (?1, ?2, NULL, 'register_capcut_draft', ?3, ?4, ?5, NULL, ?6, ?6)
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

fn process_pending_capcut_registrations(app: &AppHandle) -> Result<bool, String> {
    if capcut_process_is_running() {
        return Ok(false);
    }
    let connection = open_connection(app)?;
    let mut statement = connection
        .prepare(
            "
            SELECT id, input_json
            FROM agent_tasks
            WHERE tool_name = 'register_capcut_draft' AND status = 'queued'
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
        if capcut_process_is_running() {
            break;
        }
        let input = match serde_json::from_str::<PendingCapCutRegistration>(&input_json) {
            Ok(input) => input,
            Err(_) => {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'failed', error_message = 'Stored CapCut registration input is invalid.', updated_at = ?1 WHERE id = ?2",
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
                    crate::models::JianyingRegistrationStatus {
                        timeline_version_id: input.timeline_version_id,
                        draft_name: input.draft_name,
                        status: "registered".to_owned(),
                    },
                );
                log::info!("Completed deferred CapCut draft registration.");
            }
            _ => {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'queued', error_message = 'Pending CapCut registration will retry.', updated_at = ?1 WHERE id = ?2",
                        params![now_millis(), task_id],
                    )
                    .map_err(|error| error.to_string())?;
                log::warn!("Deferred CapCut draft registration did not complete; it will retry.");
                break;
            }
        }
    }
    Ok(true)
}

pub(crate) fn resume_pending_capcut_registrations(app: &AppHandle) -> Result<(), String> {
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
            "UPDATE agent_tasks SET status = 'queued', updated_at = ?1 WHERE tool_name = 'register_capcut_draft' AND status = 'running'",
            params![now_millis()],
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    let app = app.clone();
    thread::spawn(move || loop {
        let idle = match process_pending_capcut_registrations(&app) {
            Ok(did_work) => !did_work,
            Err(error) => {
                log::warn!("Pending CapCut registration worker failed: {error}");
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

pub(crate) fn create_capcut_draft(
    app: AppHandle,
    timeline_version_id: String,
) -> Result<JianyingDraftResult, String> {
    log::info!(
        "Starting {} draft creation.",
        crate::handoff::EditorId::CapCut.as_str()
    );
    let connection = open_connection(&app)?;
    let timeline = load_timeline_version(&connection, &timeline_version_id)?;
    if !text_tracks_are_ready_for_jianying(&timeline) {
        return Err(
            "This timeline has text tracks that are not yet verified for CapCut draft delivery. Use only the verified default-font static, fade in/out, slide-up, slide-down, or pop templates, or render a local preview."
                .to_owned(),
        );
    }
    let (root, registry_path) = find_lveditor_draft_location("CapCut").ok_or_else(|| {
        "CapCut draft library is unavailable on this computer. Open CapCut and create a local draft once, then try again."
            .to_owned()
    })?;
    let sources = collect_export_sources(&connection, &timeline, false)?;
    let plan = build_handoff_plan(&timeline, &sources)?;
    let draft_name = unique_draft_name(&connection, &timeline.project_id);
    let draft_root = root.to_string_lossy().replace('\\', "/");
    let draft_registry_path = registry_path.to_string_lossy().replace('\\', "/");
    let duration_ms = plan.duration_ms;
    let input = capcut_create_draft_input(
        &plan,
        &JianyingDraftDestination {
            draft_root: draft_root.clone(),
            draft_name: draft_name.clone(),
            draft_registry_path: draft_registry_path.clone(),
        },
    );
    let result = run_jianying_adapter(&app, &input).map_err(|error| {
        log::error!("CapCut draft adapter failed.");
        format!("CapCut draft adapter could not create a draft: {error}")
    })?;
    let registration = PendingCapCutRegistration {
        input_format_version: 2,
        operation: "registerDraft".to_owned(),
        editor: "capcut".to_owned(),
        draft_root,
        draft_name,
        draft_registry_path,
        draft_directory: result.draft_directory.clone(),
        duration_ms,
        timeline_version_id: timeline.id.clone(),
    };
    record_capcut_registration_task(&app, &timeline.project_id, &registration, &result)?;
    if result.registration_status == "pending" {
        log::info!("Completed CapCut draft creation; registration is waiting for CapCut to exit.");
    } else {
        log::info!("Completed CapCut draft creation and registration.");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn capcut_process_detection_matches_tasklist_bytes() {
        assert!(b"\"CapCut.exe\",\"1234\",\"Console\",\"1\",\"100 K\""
            .windows(b"capcut.exe".len())
            .any(|window| window.eq_ignore_ascii_case(b"capcut.exe")));
        assert!(
            !b"INFO: No tasks are running which match the specified criteria."
                .windows(b"capcut.exe".len())
                .any(|window| window.eq_ignore_ascii_case(b"capcut.exe"))
        );
    }
}
