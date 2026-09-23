//! 首次分析进度：技术与画面识别合并为互斥状态；选镜后的精修不参与统计。
use rusqlite::{params, Connection};

use crate::models::AssetAnalysisProgress;

pub(super) const ANALYSIS_STATE_SQL: &str = "CASE
    WHEN coalesce(json_extract(metadata_json, '$.analysisCancelled'), 0) = 1 THEN 'queued'
    WHEN analysis_status = 'failed' THEN 'failed'
    WHEN analysis_status = 'analyzing' THEN 'analyzing'
    WHEN analysis_status != 'ready' THEN 'queued'
    WHEN kind NOT IN ('video', 'image') THEN 'ready'
    WHEN json_extract(metadata_json, '$.visualAnalysisStatus') = 'ready' THEN 'ready'
    WHEN json_extract(metadata_json, '$.visualAnalysisStatus') = 'running' THEN 'analyzing'
    WHEN json_extract(metadata_json, '$.visualAnalysisStatus') IN ('failed', 'skipped') THEN 'failed'
    ELSE 'queued' END";

pub(super) fn project_analysis_progress(
    connection: &Connection,
    project_id: &str,
    asset_ids: Option<&[String]>,
) -> Result<AssetAnalysisProgress, String> {
    let sql = format!(
        "SELECT COUNT(*), coalesce(SUM(state = 'ready'), 0),
         coalesce(SUM(state = 'analyzing'), 0), coalesce(SUM(state = 'queued'), 0),
         coalesce(SUM(state = 'failed'), 0),
         coalesce(SUM(state = 'ready' AND kind = 'video' AND excluded = 0
            AND health NOT IN ('missing', 'changed', 'unreadable')), 0),
         coalesce(SUM(cancelled), 0)
         FROM (SELECT kind, ({ANALYSIS_STATE_SQL}) AS state,
            coalesce(json_extract(metadata_json, '$.analysisCancelled'), 0) AS cancelled,
            coalesce((SELECT excluded FROM asset_user_metadata WHERE asset_id = assets.id), 0) AS excluded,
            coalesce((SELECT status FROM asset_source_health WHERE asset_id = assets.id), 'unchecked') AS health
            FROM assets WHERE id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?1)
            AND coalesce(json_extract(metadata_json, '$.libraryRemoved'), 0) = 0
            AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2))))"
    );
    connection
        .query_row(
            &sql,
            params![
                project_id,
                asset_ids.map(|ids| serde_json::json!(ids).to_string())
            ],
            |row| {
                Ok(AssetAnalysisProgress {
                    total: row.get(0)?,
                    ready: row.get(1)?,
                    analyzing: row.get(2)?,
                    queued: row.get(3)?,
                    failed: row.get(4)?,
                    ready_video: row.get(5)?,
                    cancelled: row.get(6)?,
                })
            },
        )
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_partitions_first_pass_states_and_scopes_shared_assets() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE assets (id TEXT PRIMARY KEY, kind TEXT, analysis_status TEXT, metadata_json TEXT);
             CREATE TABLE project_asset_access (project_id TEXT, asset_id TEXT);
             CREATE TABLE asset_user_metadata (asset_id TEXT, excluded INTEGER);
             CREATE TABLE asset_source_health (asset_id TEXT, status TEXT);"
        ).unwrap();
        for (id, kind, technical, visual, expected) in [
            ("ready", "video", "ready", "ready", "ready"),
            ("audio", "audio", "ready", "queued", "ready"),
            ("technical-failed", "video", "failed", "queued", "failed"),
            ("visual-failed", "video", "ready", "failed", "failed"),
            ("skipped", "video", "ready", "skipped", "failed"),
            (
                "technical-running",
                "video",
                "analyzing",
                "queued",
                "analyzing",
            ),
            ("visual-running", "video", "ready", "running", "analyzing"),
            ("technical-queued", "video", "queued", "ready", "queued"),
            ("visual-queued", "video", "ready", "queued", "queued"),
        ] {
            connection
                .execute(
                    "INSERT INTO assets VALUES (?1, ?2, ?3, ?4)",
                    params![
                        id,
                        kind,
                        technical,
                        serde_json::json!({"visualAnalysisStatus": visual}).to_string()
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO project_asset_access VALUES ('project', ?1), ('other', ?1)",
                    [id],
                )
                .unwrap();
            let state: String = connection
                .query_row(
                    &format!("SELECT ({ANALYSIS_STATE_SQL}) FROM assets WHERE id = ?1"),
                    [id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(state, expected, "{id}");
        }
        // 同一共享素材可能由多个库关联，统计仍按素材去重。
        connection
            .execute_batch(
                "INSERT INTO project_asset_access VALUES ('project', 'ready');
            INSERT INTO assets VALUES ('other-only', 'video', 'queued', '{}');
            INSERT INTO project_asset_access VALUES ('other', 'other-only');",
            )
            .unwrap();
        let progress = project_analysis_progress(&connection, "project", None).unwrap();
        assert_eq!(
            (
                progress.total,
                progress.ready,
                progress.analyzing,
                progress.queued,
                progress.failed,
                progress.ready_video
            ),
            (9, 2, 2, 2, 3, 1)
        );
        connection
            .execute("INSERT INTO asset_user_metadata VALUES ('ready', 1)", [])
            .unwrap();
        assert_eq!(
            project_analysis_progress(&connection, "project", None)
                .unwrap()
                .ready_video,
            0
        );
        assert_eq!(
            project_analysis_progress(&connection, "empty", None)
                .unwrap()
                .total,
            0
        );
        connection.execute_batch("UPDATE assets SET metadata_json = json_set(metadata_json, '$.analysisCancelled', json('true')) WHERE id = 'visual-running';
            UPDATE assets SET metadata_json = json_set(metadata_json, '$.libraryRemoved', json('true')) WHERE id = 'ready';").unwrap();
        let progress = project_analysis_progress(&connection, "project", None).unwrap();
        assert_eq!(
            (
                progress.total,
                progress.cancelled,
                progress.analyzing,
                progress.queued
            ),
            (8, 1, 1, 3)
        );
        let batch = project_analysis_progress(
            &connection,
            "project",
            Some(&[
                "visual-running".to_owned(),
                "ready".to_owned(),
                "other-only".to_owned(),
            ]),
        )
        .unwrap();
        assert_eq!((batch.total, batch.cancelled), (1, 1));
    }
}
