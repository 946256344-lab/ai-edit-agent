//! 源文件健康检查：扫描素材可用性、检测缺失/变化/不可读状态。
//! 所有诊断只读且不修改原始媒体；baseline 状态通过新导入和重链路建立。

use crate::db::{now_millis, open_connection};
use crate::models::{AssetHealthScanStart, AssetHealthScanSummary};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::{fs, path::Path, thread};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

pub(crate) fn modified_millis(metadata: &fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|value| value.as_millis() as i64)
}

pub(crate) fn source_health_observation(
    path: &Path,
    baseline_size: Option<i64>,
    baseline_modified_ms: Option<i64>,
) -> (String, Option<i64>, Option<i64>, Option<&'static str>) {
    match fs::metadata(path) {
        Ok(metadata) => {
            let size = i64::try_from(metadata.len()).ok();
            let modified = modified_millis(&metadata);
            let changed = baseline_size.is_some_and(|value| Some(value) != size)
                || baseline_modified_ms.is_some_and(|value| Some(value) != modified);
            (
                if changed { "changed" } else { "online" }.to_owned(),
                size,
                modified,
                None,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ("missing".to_owned(), None, None, Some("not_found"))
        }
        Err(error) => (
            "unreadable".to_owned(),
            None,
            None,
            Some(match error.raw_os_error() {
                Some(5) => "access_denied",
                Some(21) => "drive_unavailable",
                Some(32) => "sharing_violation",
                Some(53 | 64 | 67 | 1231) => "network_unavailable",
                Some(123 | 161) => "invalid_path",
                _ => "io_error",
            }),
        ),
    }
}

fn run_asset_health_scan(
    app: AppHandle,
    project_id: String,
    task_id: String,
) -> Result<(), String> {
    let connection = open_connection(&app)?;
    let timestamp = now_millis();
    if connection.execute("UPDATE agent_tasks SET status = 'running', updated_at = ?1 WHERE id = ?2 AND tool_name = 'scan_asset_health' AND status = 'queued'", params![timestamp, task_id]).map_err(|error| error.to_string())? == 0 { return Ok(()); }
    let rows = {
        let mut statement = connection.prepare("SELECT a.id, a.source_reference, h.baseline_size, h.baseline_modified_ms FROM assets a LEFT JOIN asset_source_health h ON h.asset_id = a.id WHERE a.project_id = ?1 ORDER BY a.created_at, a.id").map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    let total = rows.len();
    let mut checked = 0usize;
    for (asset_id, source_reference, baseline_size, baseline_modified_ms) in rows {
        let status: String = connection
            .query_row(
                "SELECT status FROM agent_tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if status != "running" {
            return Ok(());
        }
        let (health, size, modified, reason_code) = source_health_observation(
            Path::new(&source_reference),
            baseline_size,
            baseline_modified_ms,
        );
        let baseline_size = baseline_size.or(size);
        let baseline_modified_ms = baseline_modified_ms.or(modified);
        let now = now_millis();
        connection.execute("INSERT INTO asset_source_health (asset_id, project_id, status, baseline_size, baseline_modified_ms, observed_size, observed_modified_ms, reason_code, checked_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9) ON CONFLICT(asset_id) DO UPDATE SET status=excluded.status, baseline_size=excluded.baseline_size, baseline_modified_ms=excluded.baseline_modified_ms, observed_size=excluded.observed_size, observed_modified_ms=excluded.observed_modified_ms, reason_code=excluded.reason_code, checked_at=excluded.checked_at, updated_at=excluded.updated_at", params![asset_id, project_id, health, baseline_size, baseline_modified_ms, size, modified, reason_code, now]).map_err(|error| error.to_string())?;
        checked += 1;
        if checked % 10 == 0 || checked == total {
            connection.execute("UPDATE agent_tasks SET result_json = ?1, updated_at = ?2 WHERE id = ?3 AND status = 'running'", params![serde_json::json!({"checked": checked, "total": total}).to_string(), now, task_id]).map_err(|error| error.to_string())?;
        }
    }
    connection.execute("UPDATE agent_tasks SET status = 'completed', result_json = ?1, updated_at = ?2 WHERE id = ?3 AND status = 'running'", params![serde_json::json!({"checked": checked, "total": total}).to_string(), now_millis(), task_id]).map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn start_asset_health_scan(
    app: AppHandle,
    project_id: String,
) -> Result<AssetHealthScanStart, String> {
    let connection = open_connection(&app)?;
    let existing = connection.query_row("SELECT id FROM agent_tasks WHERE project_id = ?1 AND tool_name = 'scan_asset_health' AND status IN ('queued','running') ORDER BY created_at DESC LIMIT 1", params![project_id], |row| row.get::<_, String>(0)).optional().map_err(|error| error.to_string())?;
    if let Some(task_id) = existing {
        return Ok(AssetHealthScanStart { task_id });
    }
    let task_id = Uuid::new_v4().to_string();
    let timestamp = now_millis();
    connection.execute("INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, result_json, created_at, updated_at) VALUES (?1, ?2, 'scan_asset_health', 'queued', '{}', ?3, ?4, ?4)", params![task_id, project_id, serde_json::json!({"checked":0,"total":0}).to_string(), timestamp]).map_err(|error| error.to_string())?;
    let worker_app = app.clone();
    let worker_project = project_id.clone();
    let worker_task = task_id.clone();
    thread::spawn(move || {
        if let Err(error) =
            run_asset_health_scan(worker_app.clone(), worker_project, worker_task.clone())
        {
            if let Ok(connection) = open_connection(&worker_app) {
                let _ = connection.execute("UPDATE agent_tasks SET status='failed', error_message=?1, updated_at=?2 WHERE id=?3 AND status IN ('queued','running')", params![error, now_millis(), worker_task]);
            }
        }
    });
    Ok(AssetHealthScanStart { task_id })
}

#[tauri::command]
pub fn cancel_asset_health_scan(
    app: AppHandle,
    project_id: String,
    task_id: String,
) -> Result<(), String> {
    open_connection(&app)?.execute("UPDATE agent_tasks SET status='cancelled', updated_at=?1 WHERE id=?2 AND project_id=?3 AND tool_name='scan_asset_health' AND status IN ('queued','running')", params![now_millis(), task_id, project_id]).map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_asset_health_scan_summary(
    app: AppHandle,
    project_id: String,
) -> Result<AssetHealthScanSummary, String> {
    let connection = open_connection(&app)?;
    let total: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let unchecked: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM assets a WHERE a.project_id = ?1 AND NOT EXISTS (SELECT 1 FROM asset_source_health h WHERE h.asset_id = a.id)",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let online: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM asset_source_health WHERE project_id = ?1 AND status = 'online'",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let missing: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM asset_source_health WHERE project_id = ?1 AND status = 'missing'",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let changed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM asset_source_health WHERE project_id = ?1 AND status = 'changed'",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let unreadable: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM asset_source_health WHERE project_id = ?1 AND status = 'unreadable'",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let last_checked_at: Option<i64> = connection
        .query_row(
            "SELECT MAX(checked_at) FROM asset_source_health WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .ok();
    let active_scan: Option<(String, String)> = connection
        .query_row(
            "SELECT id, status FROM agent_tasks WHERE project_id = ?1 AND tool_name = 'scan_asset_health' AND status IN ('queued','running') ORDER BY created_at DESC LIMIT 1",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    let checked = active_scan
        .as_ref()
        .and_then(|(task_id, _)| {
            connection
                .query_row(
                    "SELECT result_json FROM agent_tasks WHERE id = ?1",
                    params![task_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .ok()
                .flatten()
        })
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| value.get("checked")?.as_u64())
        .unwrap_or(0) as usize;
    Ok(AssetHealthScanSummary {
        total: total as usize,
        unchecked: unchecked as usize,
        online: online as usize,
        missing: missing as usize,
        changed: changed as usize,
        unreadable: unreadable as usize,
        checked,
        active_task_id: active_scan.as_ref().map(|(id, _)| id.clone()),
        active_task_status: active_scan.map(|(_, status)| status),
    })
}

pub(crate) fn get_asset_health_summary_for_agent(
    connection: &rusqlite::Connection,
    project_id: &str,
) -> Result<Value, String> {
    let total: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let unchecked: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM assets a WHERE a.project_id = ?1 AND NOT EXISTS (SELECT 1 FROM asset_source_health h WHERE h.asset_id = a.id)",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let online: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM asset_source_health WHERE project_id = ?1 AND status = 'online'",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let missing: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM asset_source_health WHERE project_id = ?1 AND status = 'missing'",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let changed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM asset_source_health WHERE project_id = ?1 AND status = 'changed'",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let unreadable: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM asset_source_health WHERE project_id = ?1 AND status = 'unreadable'",
            params![project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let failure_count = missing + changed + unreadable;
    let last_checked_at: Option<i64> = connection
        .query_row(
            "SELECT MAX(checked_at) FROM asset_source_health WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .ok();
    let active_scan: Option<String> = connection
        .query_row(
            "SELECT status FROM agent_tasks WHERE project_id = ?1 AND tool_name = 'scan_asset_health' AND status IN ('queued','running') ORDER BY created_at DESC LIMIT 1",
            params![project_id],
            |row| row.get(0),
        )
        .ok();
    let reason_counts: Vec<Value> = {
        let mut statement = connection
            .prepare(
                "SELECT reason_code, COUNT(*) as count FROM asset_source_health WHERE project_id = ?1 AND reason_code IS NOT NULL GROUP BY reason_code",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| {
                Ok(serde_json::json!({
                    "code": row.get::<_, String>(0)?,
                    "count": row.get::<_, i64>(1)?,
                }))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    let reasoned_failure_count: i64 = reason_counts
        .iter()
        .filter_map(|value| value.get("count")?.as_i64())
        .sum();
    let unexplained_failure_count = (failure_count - reasoned_failure_count).max(0);
    Ok(serde_json::json!({
        "total": total,
        "unchecked": unchecked,
        "online": online,
        "missing": missing,
        "changed": changed,
        "unreadable": unreadable,
        "lastCheckedAt": last_checked_at,
        "activeScanStatus": active_scan,
        "reasonCounts": reason_counts,
        "reasonedFailureCount": reasoned_failure_count,
        "unexplainedFailureCount": unexplained_failure_count,
        "reasonEvidenceAvailable": failure_count > 0 && unexplained_failure_count == 0,
    }))
}
