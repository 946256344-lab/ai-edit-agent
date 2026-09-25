//! 素材分析取消/继续与库内编辑：仅修改本地索引，保留源文件及已有时间线引用。
use super::progress::{project_analysis_progress, ANALYSIS_STATE_SQL};
use crate::db::{now_millis, open_connection};
use crate::models::{AssetAnalysisProgress, BatchAssetActionResult};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use tauri::{AppHandle, Emitter};

pub(super) fn task_running(app: &AppHandle, task_id: &str) -> bool {
    open_connection(app)
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT status = 'running' FROM agent_tasks WHERE id = ?1",
                    [task_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())
        })
        .unwrap_or(false)
}

#[tauri::command(async)]
pub fn get_asset_analysis_progress(
    app: AppHandle,
    project_id: String,
    asset_ids: Option<Vec<String>>,
) -> Result<AssetAnalysisProgress, String> {
    project_analysis_progress(&open_connection(&app)?, &project_id, asset_ids.as_deref())
}

// 取消资产对应的任务；混合批次中的其他素材重新排队，避免影响其他共享素材。
fn cancel_in_scope(
    connection: &Connection,
    project_id: &str,
    asset_ids: Option<&[String]>,
) -> Result<(usize, Vec<String>), String> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let timestamp = now_millis();
    let updated = transaction.execute(&format!(
        "UPDATE assets SET metadata_json = json_set(metadata_json, '$.analysisCancelled', json('true')),
         analysis_status = CASE WHEN analysis_status = 'analyzing' THEN 'queued' ELSE analysis_status END, updated_at = ?3
         WHERE id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?1)
         AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2)))
         AND coalesce(json_extract(metadata_json, '$.analysisCancelled'), 0) = 0
         AND ({ANALYSIS_STATE_SQL}) IN ('queued', 'analyzing')"),
        params![project_id, asset_ids.map(|ids| serde_json::json!(ids).to_string()), timestamp]
    ).map_err(|error| error.to_string())?;
    transaction.execute(
        "UPDATE agent_tasks SET status = 'cancelled', updated_at = ?1
         WHERE tool_name = 'analyze_asset' AND status IN ('queued', 'running')
         AND json_extract(input_json, '$.assetId') IN (SELECT id FROM assets WHERE json_extract(metadata_json, '$.analysisCancelled') = 1)",
        [timestamp]).map_err(|error| error.to_string())?;
    let mut statement = transaction.prepare(
        "SELECT id, input_json FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status IN ('queued', 'running')
         AND EXISTS (SELECT 1 FROM json_each(input_json, '$.assetIds') j JOIN assets a ON a.id = j.value
             WHERE json_extract(a.metadata_json, '$.analysisCancelled') = 1)"
    ).map_err(|error| error.to_string())?;
    let tasks = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    let mut remaining = HashSet::new();
    for (id, input) in tasks {
        transaction
            .execute(
                "UPDATE agent_tasks SET status = 'cancelled', updated_at = ?2 WHERE id = ?1",
                params![id, timestamp],
            )
            .map_err(|error| error.to_string())?;
        let mut statement = transaction
            .prepare(&format!(
                "SELECT id FROM assets WHERE id IN (SELECT value FROM json_each(?1, '$.assetIds'))
             AND coalesce(json_extract(metadata_json, '$.analysisCancelled'), 0) = 0
             AND ({ANALYSIS_STATE_SQL}) IN ('queued', 'analyzing')"
            ))
            .map_err(|error| error.to_string())?;
        for row in statement
            .query_map([input], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
        {
            remaining.insert(row.map_err(|error| error.to_string())?);
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok((updated, remaining.into_iter().collect()))
}

#[tauri::command(async)]
pub fn cancel_asset_analysis(
    app: AppHandle,
    project_id: String,
    asset_ids: Option<Vec<String>>,
) -> Result<usize, String> {
    let (updated, remaining) =
        cancel_in_scope(&open_connection(&app)?, &project_id, asset_ids.as_deref())?;
    super::visual::queue_visual_analysis_batch(&app, &remaining)?;
    let _ = app.emit("assets-changed", &project_id);
    Ok(updated)
}

#[tauri::command(async)]
pub fn resume_asset_analysis(
    app: AppHandle,
    project_id: String,
    asset_ids: Option<Vec<String>>,
) -> Result<usize, String> {
    let connection = open_connection(&app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut statement = transaction.prepare(
        "SELECT id, analysis_status FROM assets WHERE id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?1)
         AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2)))
         AND json_extract(metadata_json, '$.analysisCancelled') = 1 AND coalesce(json_extract(metadata_json, '$.libraryRemoved'), 0) = 0"
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![
                project_id,
                asset_ids.map(|ids| serde_json::json!(ids).to_string())
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    let mut technical = Vec::new();
    let mut visual = Vec::new();
    for (id, status) in &rows {
        transaction.execute("UPDATE assets SET metadata_json = json_set(metadata_json, '$.analysisCancelled', json('false')), updated_at = ?2 WHERE id = ?1", params![id, now_millis()]).map_err(|error| error.to_string())?;
        if status == "ready" {
            visual.push(id.clone());
        } else {
            technical.push(id.clone());
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    if !technical.is_empty() {
        super::analysis::request_asset_analysis(&app, &project_id, &technical)?;
    }
    super::visual::queue_visual_analysis_batch(&app, &visual)?;
    let _ = app.emit("assets-changed", &project_id);
    Ok(rows.len())
}

#[tauri::command(async)]
pub fn rename_library_asset(
    app: AppHandle,
    project_id: String,
    asset_id: String,
    name: String,
) -> Result<(), String> {
    let updated = open_connection(&app)?.execute(
        "UPDATE assets SET display_name = ?3, updated_at = ?4 WHERE id = ?2 AND id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?1)",
        params![project_id, asset_id, name, now_millis()]).map_err(|error| error.to_string())?;
    if updated == 0 {
        return Err("Selected asset is not available in this project.".to_owned());
    }
    let _ = app.emit("assets-changed", &project_id);
    Ok(())
}

#[tauri::command(async)]
pub fn remove_library_assets(
    app: AppHandle,
    project_id: String,
    asset_ids: Vec<String>,
) -> Result<BatchAssetActionResult, String> {
    let connection = open_connection(&app)?;
    let (_, remaining) = cancel_in_scope(&connection, &project_id, Some(&asset_ids))?;
    let updated_count = connection.execute(
        "UPDATE assets SET metadata_json = json_set(metadata_json, '$.libraryRemoved', json('true')), updated_at = ?3
         WHERE id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?1)
         AND id IN (SELECT value FROM json_each(?2)) AND coalesce(json_extract(metadata_json, '$.libraryRemoved'), 0) = 0",
        params![project_id, serde_json::json!(asset_ids).to_string(), now_millis()]).map_err(|error| error.to_string())?;
    super::visual::queue_visual_analysis_batch(&app, &remaining)?;
    let _ = app.emit("assets-changed", &project_id);
    Ok(BatchAssetActionResult {
        requested_count: asset_ids.len(),
        updated_count,
        skipped_count: asset_ids.len().saturating_sub(updated_count),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_scopes_assets_and_preserves_completed_results() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE assets (id TEXT PRIMARY KEY, kind TEXT, analysis_status TEXT, metadata_json TEXT, updated_at INTEGER);
            CREATE TABLE project_asset_access (project_id TEXT, asset_id TEXT);
            CREATE TABLE agent_tasks (id TEXT PRIMARY KEY, tool_name TEXT, status TEXT, input_json TEXT, updated_at INTEGER);
            INSERT INTO assets VALUES ('technical', 'video', 'analyzing', '{}', 0),
                ('visual', 'video', 'ready', '{\"visualAnalysisStatus\":\"running\"}', 0),
                ('other', 'video', 'ready', '{\"visualAnalysisStatus\":\"queued\"}', 0),
                ('ready', 'video', 'ready', '{\"visualAnalysisStatus\":\"ready\"}', 0);
            INSERT INTO project_asset_access VALUES ('p', 'technical'), ('p', 'visual'), ('p', 'ready'), ('q', 'other');
            INSERT INTO agent_tasks VALUES ('t', 'analyze_asset', 'running', '{\"assetId\":\"technical\"}', 0),
                ('v', 'analyze_asset_visual_batch', 'running', '{\"assetIds\":[\"visual\",\"other\"]}', 0);").unwrap();
        let (count, remaining) = cancel_in_scope(&connection, "p", None).unwrap();
        assert_eq!(count, 2);
        assert_eq!(remaining, vec!["other"]);
        let cancelled_tasks: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM agent_tasks WHERE status = 'cancelled'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cancelled_tasks, 2);
        let ready: String = connection
            .query_row(
                "SELECT metadata_json FROM assets WHERE id = 'ready'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ready, r#"{"visualAnalysisStatus":"ready"}"#);
        assert_eq!(cancel_in_scope(&connection, "p", None).unwrap().0, 0);
    }
}
