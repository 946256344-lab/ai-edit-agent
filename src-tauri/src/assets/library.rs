//! 素材库查询：分页、搜索、目录投影、collection/tag/metadata 管理。
//!
//! 职责：
//! - 分页查询与筛选（`list_asset_page`）
//! - 目录投影与安全相对路径（`project_asset_directories`）
//! - 素材证据检视（`get_asset_evidence`）
//! - 用户整理：收藏、评分、备注、标签、集合（batch 操作）
//! - 旧版导入路径兼容（legacy_* 函数）
//!
//! 不拥有：素材导入、技术分析、视觉分析、健康扫描（仍在 `assets.rs`）。

use rusqlite::{params, Connection, OptionalExtension};
use serde_json;
use std::collections::HashMap;
use std::path::Path;
use tauri::AppHandle;
use uuid::Uuid;

use crate::db::{now_millis, open_connection};
use crate::models::{
    Asset, AssetCollection, AssetDirectory, AssetEvidence, AssetPage, AssetStatusCounts,
    BatchAssetActionResult, TechnicalMetadata,
};

pub(crate) const ASSET_PAGE_FILTER_SQL: &str = "
    project_id = ?1
    AND (?2 IS NULL OR display_name LIKE '%' || ?2 || '%' OR id IN (SELECT asset_id FROM asset_user_metadata WHERE note LIKE '%' || ?2 || '%' OR asset_id IN (SELECT ata.asset_id FROM asset_tag_assignments ata JOIN asset_tags t ON t.id = ata.tag_id WHERE t.name LIKE '%' || ?2 || '%')))
    AND (?3 IS NULL OR kind = ?3)
    AND (?4 IS NULL OR analysis_status = ?4)
    AND (?5 IS NULL OR (
        (?5 = 'storyboard-ready' AND analysis_status = 'ready' AND json_extract(metadata_json, '$.visualAnalysisStatus') = 'ready' AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 0 AND coalesce((SELECT status FROM asset_source_health ash WHERE ash.asset_id = assets.id), 'unchecked') NOT IN ('missing', 'changed', 'unreadable'))
        OR (?5 != 'storyboard-ready' AND json_extract(metadata_json, '$.visualAnalysisStatus') = ?5)
    ))
    AND (?6 IS NULL OR id IN (SELECT value FROM json_each(?6)))
    AND (?7 IS NULL OR (
        (?7 = 'favorite' AND coalesce((SELECT favorite FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 1)
        OR (?7 = 'excluded' AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 1)
        OR (?7 = 'available' AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 0 AND coalesce((SELECT status FROM asset_source_health ash WHERE ash.asset_id = assets.id), 'unchecked') NOT IN ('missing', 'changed', 'unreadable'))
    ))
    AND (?8 IS NULL OR id IN (SELECT asset_id FROM asset_collection_items WHERE collection_id = ?8))
";

pub(crate) fn asset_safe_directory(
    source_reference: &str,
    folder_reference: Option<&str>,
) -> Option<String> {
    let folder_reference = folder_reference?;
    let root = Path::new(folder_reference).file_name()?.to_str()?;
    let relative = Path::new(source_reference)
        .strip_prefix(folder_reference)
        .ok()?;
    let parent = relative
        .parent()
        .filter(|path| !path.as_os_str().is_empty());
    Some(match parent.and_then(Path::to_str) {
        Some(value) => format!("{}\\{}", root, value.replace('/', "\\")),
        None => root.to_owned(),
    })
}

pub(crate) fn asset_public_folder_metadata(
    directory_key: Option<&str>,
    display_name: &str,
) -> (Option<String>, Option<String>) {
    let Some(directory_key) = directory_key else {
        return (None, None);
    };
    let (folder_name, relative_directory) = directory_key
        .split_once('\\')
        .map_or((directory_key, None), |(root, relative)| {
            (root, Some(relative))
        });
    let relative_path = relative_directory.map_or_else(
        || display_name.to_owned(),
        |directory| format!("{directory}\\{display_name}"),
    );
    (Some(folder_name.to_owned()), Some(relative_path))
}

pub(crate) fn asset_directory_nodes(
    asset_directories: &HashMap<String, String>,
) -> Vec<AssetDirectory> {
    let mut nodes = HashMap::<String, (String, usize)>::new();
    for directory in asset_directories.values() {
        let normalized = directory.replace('/', "\\").trim_matches('\\').to_owned();
        let segments = normalized
            .split('\\')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            continue;
        }
        let mut key = String::new();
        for segment in segments {
            if !key.is_empty() {
                key.push('\\');
            }
            key.push_str(segment);
            nodes
                .entry(key.to_lowercase())
                .or_insert_with(|| (key.clone(), 0));
        }
        if let Some((_, direct_asset_count)) = nodes.get_mut(&key.to_lowercase()) {
            *direct_asset_count += 1;
        }
    }
    let mut directories = nodes
        .into_values()
        .filter_map(|(key, direct_asset_count)| {
            let name = key.rsplit('\\').next()?.to_owned();
            let parent_key = key.rsplit_once('\\').map(|(parent, _)| parent.to_owned());
            Some(AssetDirectory {
                key,
                name,
                parent_key,
                direct_asset_count,
            })
        })
        .collect::<Vec<_>>();
    directories.sort_by(|left, right| left.key.to_lowercase().cmp(&right.key.to_lowercase()));
    directories
}

pub(crate) fn project_asset_directories(
    connection: &Connection,
    project_id: &str,
) -> Result<HashMap<String, String>, String> {
    let mut statement = connection
        .prepare("SELECT id, source_reference, folder_reference FROM assets WHERE project_id = ?1")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    let mut directories = HashMap::new();
    let has_folder_reference = rows.iter().any(|(_, _, folder)| folder.is_some());
    if has_folder_reference {
        for (id, source, folder) in &rows {
            if let Some(directory) = asset_safe_directory(source, folder.as_deref()) {
                directories.insert(id.clone(), directory);
            }
        }
    } else {
        let legacy_rows = rows
            .iter()
            .map(|(id, source, _)| (id.clone(), source.clone()))
            .collect::<Vec<_>>();
        directories = legacy_asset_directories(&legacy_rows);
    }
    Ok(directories)
}

fn list_assets_snapshot(
    app: AppHandle,
    project_id: String,
    schedule_pending_analysis: bool,
) -> Result<Vec<Asset>, String> {
    let connection = open_connection(&app)?;
    let asset_directories = project_asset_directories(&connection, &project_id)?;
    let mut statement = connection.prepare(
        "SELECT id, project_id, kind, display_name, source_reference, folder_reference, analysis_status, metadata_json, created_at, updated_at,
         coalesce((SELECT favorite FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0),
         coalesce((SELECT rating FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0),
         coalesce((SELECT note FROM asset_user_metadata um WHERE um.asset_id = assets.id), ''),
         coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0),
         coalesce((SELECT json_group_array(t.name) FROM asset_tag_assignments ata JOIN asset_tags t ON t.id = ata.tag_id WHERE ata.asset_id = assets.id), '[]'),
         coalesce((SELECT json_group_array(aci.collection_id) FROM asset_collection_items aci WHERE aci.asset_id = assets.id), '[]'),
         coalesce((SELECT status FROM asset_source_health ash WHERE ash.asset_id = assets.id), 'unchecked'),
         (SELECT checked_at FROM asset_source_health ash WHERE ash.asset_id = assets.id)
         FROM assets WHERE project_id = ?1 ORDER BY created_at DESC",
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            let id: String = row.get(0)?;
            let display_name: String = row.get(3)?;
            let directory_key = asset_directories.get(&id).cloned();
            let (folder_name, relative_path) =
                asset_public_folder_metadata(directory_key.as_deref(), &display_name);
            let metadata: TechnicalMetadata =
                serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default();
            Ok(Asset {
                id,
                project_id: row.get(1)?,
                kind: row.get(2)?,
                display_name,
                folder_name,
                relative_path,
                directory_key,
                analysis_status: row.get(6)?,
                visual_analysis_status: metadata.visual_analysis_status.clone(),
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
                favorite: row.get::<_, i64>(10)? != 0,
                rating: row.get(11)?,
                note: row.get(12)?,
                excluded: row.get::<_, i64>(13)? != 0,
                user_tags: serde_json::from_str(&row.get::<_, String>(14)?).unwrap_or_default(),
                collection_ids: serde_json::from_str(&row.get::<_, String>(15)?)
                    .unwrap_or_default(),
                source_health_status: row.get(16)?,
                source_health_checked_at: row.get(17)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let assets = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    if schedule_pending_analysis {
        crate::assets::drain_pending_analysis(&app, &project_id)?;
    }
    Ok(assets)
}

#[tauri::command]
pub fn list_assets(app: AppHandle, project_id: String) -> Result<Vec<Asset>, String> {
    list_assets_snapshot(app, project_id, true)
}

/// Agent 观察不得唤醒分析；桌面列表保留旧调度行为，Agent 只读同一持久化快照。
pub(crate) fn list_assets_for_agent(
    app: AppHandle,
    project_id: String,
) -> Result<Vec<Asset>, String> {
    list_assets_snapshot(app, project_id, false)
}

#[tauri::command]
/// 返回一个有界素材页和目录投影；目录筛选按"直属素材"语义执行，而不是递归混入后代。
pub fn list_asset_page(
    app: AppHandle,
    project_id: String,
    search: Option<String>,
    kind: Option<String>,
    analysis_status: Option<String>,
    visual_status: Option<String>,
    directory_key: Option<String>,
    user_filter: Option<String>,
    collection_id: Option<String>,
    offset: usize,
    limit: usize,
) -> Result<AssetPage, String> {
    let limit = limit.clamp(1, 200);
    let search = search
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let kind = kind.filter(|value| matches!(value.as_str(), "video" | "image" | "audio" | "other"));
    let analysis_status = analysis_status
        .filter(|value| matches!(value.as_str(), "queued" | "analyzing" | "ready" | "failed"));
    let visual_status = visual_status.filter(|value| {
        matches!(
            value.as_str(),
            "queued" | "running" | "ready" | "failed" | "skipped" | "storyboard-ready"
        )
    });
    let directory_key = directory_key
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let user_filter =
        user_filter.filter(|value| matches!(value.as_str(), "favorite" | "excluded" | "available"));
    let collection_id = collection_id
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let connection = open_connection(&app)?;
    let asset_directories = project_asset_directories(&connection, &project_id)?;
    let folder_asset_ids = if let Some(folder) = directory_key.as_deref() {
        let ids = if folder == "__unfiled__" {
            let mut statement = connection
                .prepare("SELECT id FROM assets WHERE project_id = ?1")
                .map_err(|error| error.to_string())?;
            let ids = statement
                .query_map(params![project_id], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
                .filter(|id| !asset_directories.contains_key(id))
                .collect::<Vec<_>>();
            ids
        } else {
            asset_directories
                .iter()
                .filter_map(|(id, directory)| {
                    directory.eq_ignore_ascii_case(folder).then_some(id.clone())
                })
                .collect::<Vec<_>>()
        };
        Some(serde_json::to_string(&ids).map_err(|error| error.to_string())?)
    } else {
        None
    };
    let filter_sql = ASSET_PAGE_FILTER_SQL;
    let query_params = params![
        project_id,
        search,
        kind,
        analysis_status,
        visual_status,
        folder_asset_ids,
        user_filter,
        collection_id,
    ];
    let total: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM assets WHERE {filter_sql}"),
            query_params,
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mut statement = connection
        .prepare(&format!(
            "SELECT id, project_id, kind, display_name, source_reference, folder_reference, analysis_status, metadata_json, created_at, updated_at,
             coalesce((SELECT favorite FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0),
             coalesce((SELECT rating FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0),
             coalesce((SELECT note FROM asset_user_metadata um WHERE um.asset_id = assets.id), ''),
             coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0),
             coalesce((SELECT json_group_array(t.name) FROM asset_tag_assignments ata JOIN asset_tags t ON t.id = ata.tag_id WHERE ata.asset_id = assets.id), '[]'),
             coalesce((SELECT json_group_array(aci.collection_id) FROM asset_collection_items aci WHERE aci.asset_id = assets.id), '[]'),
             coalesce((SELECT status FROM asset_source_health ash WHERE ash.asset_id = assets.id), 'unchecked'),
             (SELECT checked_at FROM asset_source_health ash WHERE ash.asset_id = assets.id)
             FROM assets WHERE {filter_sql} ORDER BY created_at DESC, id DESC LIMIT ?9 OFFSET ?10"
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![
                project_id,
                search,
                kind,
                analysis_status,
                visual_status,
                folder_asset_ids,
                user_filter,
                collection_id,
                limit as i64,
                offset as i64,
            ],
            |row| {
                let id: String = row.get(0)?;
                let display_name: String = row.get(3)?;
                let directory_key = asset_directories.get(&id).cloned();
                let (folder_name, relative_path) =
                    asset_public_folder_metadata(directory_key.as_deref(), &display_name);
                let metadata: TechnicalMetadata =
                    serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default();
                Ok(Asset {
                    id,
                    project_id: row.get(1)?,
                    kind: row.get(2)?,
                    display_name,
                    folder_name,
                    relative_path,
                    directory_key,
                    analysis_status: row.get(6)?,
                    visual_analysis_status: metadata.visual_analysis_status.clone(),
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
                    favorite: row.get::<_, i64>(10)? != 0,
                    rating: row.get(11)?,
                    note: row.get(12)?,
                    excluded: row.get::<_, i64>(13)? != 0,
                    user_tags: serde_json::from_str(&row.get::<_, String>(14)?).unwrap_or_default(),
                    collection_ids: serde_json::from_str(&row.get::<_, String>(15)?)
                        .unwrap_or_default(),
                    source_health_status: row.get(16)?,
                    source_health_checked_at: row.get(17)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    let items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);

    let counts = connection
        .query_row(
            "SELECT COUNT(*), SUM(analysis_status = 'ready'), SUM(analysis_status = 'analyzing'), SUM(analysis_status = 'queued'), SUM(analysis_status = 'failed') FROM assets WHERE project_id = ?1",
            params![project_id],
            |row| Ok(AssetStatusCounts { total: row.get::<_, i64>(0)? as usize, ready: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as usize, analyzing: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as usize, queued: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as usize, failed: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as usize }),
        )
        .map_err(|error| error.to_string())?;
    let directories = asset_directory_nodes(&asset_directories);
    let unfiled_count = counts.total.saturating_sub(asset_directories.len());
    crate::assets::drain_pending_analysis(&app, &project_id)?;
    Ok(AssetPage {
        items,
        total: total as usize,
        offset,
        limit,
        directories,
        unfiled_count,
        counts,
    })
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
                    duration_ms: metadata.duration_ms,
                    visual_analysis_status: metadata.visual_analysis_status,
                    keyframes: metadata.keyframes,
                    ocr_evidence: metadata.ocr_evidence,
                    visual_evidence: metadata.visual_evidence,
                    visual_analysis_note: metadata.visual_analysis_note,
                })
            },
        )
        .map_err(|_| "Asset evidence is unavailable.".to_owned())
}

fn normalized_asset_label(value: String, kind: &str) -> Result<String, String> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > 64 {
        return Err(format!("{kind} must contain between 1 and 64 characters."));
    }
    Ok(value)
}

fn validate_batch_asset_ids(
    transaction: &Connection,
    project_id: &str,
    asset_ids: Vec<String>,
) -> Result<Vec<String>, String> {
    if asset_ids.is_empty() {
        return Err("Select at least one asset.".to_owned());
    }
    if asset_ids.len() > 200 {
        return Err("Cannot update more than 200 assets at once.".to_owned());
    }
    let unique_ids = asset_ids
        .into_iter()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let placeholders = unique_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut statement = transaction
        .prepare(&format!(
            "SELECT id FROM assets WHERE project_id = ?1 AND id IN ({placeholders})"
        ))
        .map_err(|error| error.to_string())?;
    let mut params_vec: Vec<&dyn rusqlite::ToSql> = vec![&project_id];
    for id in &unique_ids {
        params_vec.push(id);
    }
    let validated = statement
        .query_map(params_vec.as_slice(), |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if validated.len() != unique_ids.len() {
        return Err("One or more selected assets are not available in this project.".to_owned());
    }
    Ok(validated)
}

#[tauri::command]
pub fn update_asset_user_metadata_batch(
    app: AppHandle,
    project_id: String,
    asset_ids: Vec<String>,
    favorite: Option<bool>,
    rating: Option<i64>,
    note: Option<String>,
    excluded: Option<bool>,
) -> Result<BatchAssetActionResult, String> {
    if favorite.is_none() && rating.is_none() && note.is_none() && excluded.is_none() {
        return Err("Choose at least one user metadata field to update.".to_owned());
    }
    if !matches!(rating, None | Some(0..=5)) {
        return Err("Asset rating must be between 0 and 5.".to_owned());
    }
    let note = note.map(|value| value.trim().to_owned());
    if note
        .as_ref()
        .is_some_and(|value| value.chars().count() > 2000)
    {
        return Err("Asset note is too long.".to_owned());
    }
    let connection = open_connection(&app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let asset_ids = validate_batch_asset_ids(&transaction, &project_id, asset_ids)?;
    let timestamp = now_millis();
    for asset_id in &asset_ids {
        transaction.execute("INSERT INTO asset_user_metadata (asset_id, project_id, favorite, rating, note, excluded, updated_at) VALUES (?1, ?2, coalesce(?3, 0), coalesce(?4, 0), coalesce(?5, ''), coalesce(?6, 0), ?7) ON CONFLICT(asset_id) DO UPDATE SET favorite = coalesce(?3, favorite), rating = coalesce(?4, rating), note = coalesce(?5, note), excluded = coalesce(?6, excluded), updated_at = ?7", params![asset_id, project_id, favorite.map(i64::from), rating, note, excluded.map(i64::from), timestamp]).map_err(|error| error.to_string())?;
    }
    transaction.execute("INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, after_json, created_at) VALUES (?1, ?2, 'user', 'update_asset_user_metadata_batch', 'project_assets', ?2, ?3, ?4)", params![Uuid::new_v4().to_string(), project_id, serde_json::json!({ "updatedCount": asset_ids.len(), "favoriteChanged": favorite.is_some(), "ratingChanged": rating.is_some(), "noteChanged": note.is_some(), "excludedChanged": excluded.is_some() }).to_string(), timestamp]).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(BatchAssetActionResult {
        requested_count: asset_ids.len(),
        updated_count: asset_ids.len(),
        skipped_count: 0,
    })
}

#[tauri::command]
pub fn add_asset_tag_batch(
    app: AppHandle,
    project_id: String,
    asset_ids: Vec<String>,
    tag: String,
) -> Result<BatchAssetActionResult, String> {
    let tag = normalized_asset_label(tag, "Asset tag")?;
    let connection = open_connection(&app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let asset_ids = validate_batch_asset_ids(&transaction, &project_id, asset_ids)?;
    let timestamp = now_millis();
    let tag_id = transaction
        .query_row(
            "SELECT id FROM asset_tags WHERE project_id = ?1 AND name = ?2 COLLATE NOCASE",
            params![project_id, tag],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    transaction.execute("INSERT OR IGNORE INTO asset_tags (id, project_id, name, created_at) VALUES (?1, ?2, ?3, ?4)", params![tag_id, project_id, tag, timestamp]).map_err(|error| error.to_string())?;
    let mut updated_count = 0usize;
    for asset_id in &asset_ids {
        updated_count += transaction.execute("INSERT OR IGNORE INTO asset_tag_assignments (asset_id, tag_id, created_at) VALUES (?1, ?2, ?3)", params![asset_id, tag_id, timestamp]).map_err(|error| error.to_string())?;
    }
    transaction.execute("INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, after_json, created_at) VALUES (?1, ?2, 'user', 'add_asset_tag_batch', 'project_assets', ?2, ?3, ?4)", params![Uuid::new_v4().to_string(), project_id, serde_json::json!({ "updatedCount": updated_count }).to_string(), timestamp]).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(BatchAssetActionResult {
        requested_count: asset_ids.len(),
        updated_count,
        skipped_count: asset_ids.len().saturating_sub(updated_count),
    })
}

#[tauri::command]
pub fn remove_asset_tag_batch(
    app: AppHandle,
    project_id: String,
    asset_ids: Vec<String>,
    tag: String,
) -> Result<BatchAssetActionResult, String> {
    let tag = normalized_asset_label(tag, "Asset tag")?;
    let connection = open_connection(&app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let asset_ids = validate_batch_asset_ids(&transaction, &project_id, asset_ids)?;
    let mut updated_count = 0usize;
    for asset_id in &asset_ids {
        updated_count += transaction.execute("DELETE FROM asset_tag_assignments WHERE asset_id = ?1 AND tag_id IN (SELECT id FROM asset_tags WHERE project_id = ?2 AND name = ?3 COLLATE NOCASE)", params![asset_id, project_id, tag]).map_err(|error| error.to_string())?;
    }
    transaction.execute("INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, after_json, created_at) VALUES (?1, ?2, 'user', 'remove_asset_tag_batch', 'project_assets', ?2, ?3, ?4)", params![Uuid::new_v4().to_string(), project_id, serde_json::json!({ "updatedCount": updated_count }).to_string(), now_millis()]).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(BatchAssetActionResult {
        requested_count: asset_ids.len(),
        updated_count,
        skipped_count: asset_ids.len().saturating_sub(updated_count),
    })
}

#[tauri::command]
pub fn create_asset_collection(
    app: AppHandle,
    project_id: String,
    name: String,
) -> Result<AssetCollection, String> {
    let name = normalized_asset_label(name, "Asset collection name")?;
    let connection = open_connection(&app)?;
    let timestamp = now_millis();
    let id = Uuid::new_v4().to_string();
    connection.execute("INSERT INTO asset_collections (id, project_id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)", params![id, project_id, name, timestamp]).map_err(|_| "An asset collection with this name already exists.".to_owned())?;
    connection.execute("INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, after_json, created_at) VALUES (?1, ?2, 'user', 'create_asset_collection', 'asset_collection', ?3, ?4, ?5)", params![Uuid::new_v4().to_string(), project_id, id, serde_json::json!({ "created": true }).to_string(), timestamp]).map_err(|error| error.to_string())?;
    Ok(AssetCollection {
        id,
        project_id,
        name,
        asset_count: 0,
        created_at: timestamp,
        updated_at: timestamp,
    })
}

#[tauri::command]
pub fn list_asset_collections(
    app: AppHandle,
    project_id: String,
) -> Result<Vec<AssetCollection>, String> {
    let connection = open_connection(&app)?;
    let mut statement = connection.prepare("SELECT c.id, c.project_id, c.name, COUNT(i.asset_id), c.created_at, c.updated_at FROM asset_collections c LEFT JOIN asset_collection_items i ON i.collection_id = c.id WHERE c.project_id = ?1 GROUP BY c.id ORDER BY c.updated_at DESC, c.name COLLATE NOCASE").map_err(|error| error.to_string())?;
    let collections = statement
        .query_map(params![project_id], |row| {
            Ok(AssetCollection {
                id: row.get(0)?,
                project_id: row.get(1)?,
                name: row.get(2)?,
                asset_count: row.get::<_, i64>(3)? as usize,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(collections)
}

#[tauri::command]
pub fn add_assets_to_collection(
    app: AppHandle,
    project_id: String,
    collection_id: String,
    asset_ids: Vec<String>,
) -> Result<BatchAssetActionResult, String> {
    let connection = open_connection(&app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let asset_ids = validate_batch_asset_ids(&transaction, &project_id, asset_ids)?;
    let collection_exists: i64 = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM asset_collections WHERE id = ?1 AND project_id = ?2)",
            params![collection_id, project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if collection_exists == 0 {
        return Err("Selected asset collection is not available in this project.".to_owned());
    }
    let timestamp = now_millis();
    let mut updated_count = 0usize;
    for asset_id in &asset_ids {
        updated_count += transaction.execute("INSERT OR IGNORE INTO asset_collection_items (collection_id, asset_id, created_at) VALUES (?1, ?2, ?3)", params![collection_id, asset_id, timestamp]).map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "UPDATE asset_collections SET updated_at = ?1 WHERE id = ?2",
            params![timestamp, collection_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.execute("INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, after_json, created_at) VALUES (?1, ?2, 'user', 'add_assets_to_collection', 'asset_collection', ?3, ?4, ?5)", params![Uuid::new_v4().to_string(), project_id, collection_id, serde_json::json!({ "updatedCount": updated_count }).to_string(), timestamp]).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(BatchAssetActionResult {
        requested_count: asset_ids.len(),
        updated_count,
        skipped_count: asset_ids.len().saturating_sub(updated_count),
    })
}

// ============================================================================
// Legacy path compatibility
// ============================================================================

fn safe_legacy_parent_parts(parts: &[&str]) -> Option<Vec<String>> {
    let mut parts = parts
        .iter()
        .filter(|part| !part.is_empty())
        .map(|part| (*part).to_owned())
        .collect::<Vec<_>>();
    parts.pop()?;
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| part == "." || part == ".." || part.contains(':'))
    {
        return None;
    }
    Some(parts)
}

fn legacy_source_parent(source: &str) -> Option<(String, Vec<String>)> {
    let normalized = source.replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with(r"\\?\unc\") {
        let parts = normalized[8..].split('\\').collect::<Vec<_>>();
        let (server, share) = (*parts.first()?, *parts.get(1)?);
        if server.is_empty()
            || share.is_empty()
            || [server, share]
                .iter()
                .any(|part| *part == "." || *part == ".." || part.contains(':'))
        {
            return None;
        }
        return Some((
            format!(
                "unc:{}\\{}",
                server.to_ascii_lowercase(),
                share.to_ascii_lowercase()
            ),
            safe_legacy_parent_parts(&parts[2..])?,
        ));
    }
    if lower.starts_with(r"\\?\") {
        return legacy_drive_parent(&normalized[4..]);
    }
    if normalized.starts_with(r"\\") {
        let parts = normalized[2..].split('\\').collect::<Vec<_>>();
        let (server, share) = (*parts.first()?, *parts.get(1)?);
        if server.is_empty()
            || share.is_empty()
            || [server, share]
                .iter()
                .any(|part| *part == "." || *part == ".." || part.contains(':'))
        {
            return None;
        }
        return Some((
            format!(
                "unc:{}\\{}",
                server.to_ascii_lowercase(),
                share.to_ascii_lowercase()
            ),
            safe_legacy_parent_parts(&parts[2..])?,
        ));
    }
    legacy_drive_parent(&normalized)
}

fn legacy_drive_parent(normalized: &str) -> Option<(String, Vec<String>)> {
    let (drive, relative) = normalized.split_once('\\')?;
    if drive.len() != 2 || !drive.ends_with(':') {
        return None;
    }
    Some((
        format!("drive:{}", drive.to_ascii_lowercase()),
        safe_legacy_parent_parts(&relative.split('\\').collect::<Vec<_>>())?,
    ))
}

pub(crate) fn legacy_asset_directories(rows: &[(String, String)]) -> HashMap<String, String> {
    // 旧数据只在同一安全卷根内恢复相对树；多根加稳定命名空间，响应绝不暴露盘符或 UNC 根。
    let mut root_groups = HashMap::<String, Vec<(String, Vec<String>)>>::new();
    for (id, source) in rows {
        if let Some((root, parts)) = legacy_source_parent(source) {
            root_groups
                .entry(root)
                .or_default()
                .push((id.clone(), parts));
        }
    }
    let mut directories = HashMap::new();
    let mut groups = root_groups
        .into_iter()
        .filter(|(_, group)| group.len() >= 2)
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| left.0.cmp(&right.0));
    let group_count = groups.len();
    for (group_index, (_, group)) in groups.into_iter().enumerate() {
        let namespace = (group_count > 1).then(|| format!("导入素材 {}", group_index + 1));
        let mut common = group[0].1.len();
        for (_, parts) in &group[1..] {
            common = common.min(parts.len());
            common = (0..common)
                .take_while(|index| group[0].1[*index].eq_ignore_ascii_case(&parts[*index]))
                .count();
        }
        if common == 0 {
            let root = namespace.unwrap_or_else(|| "导入素材".to_owned());
            directories.extend(
                group
                    .into_iter()
                    .map(|(id, parts)| (id, format!("{root}\\{}", parts.join("\\")))),
            );
            continue;
        }
        let base = common.saturating_sub(1);
        directories.extend(group.into_iter().filter_map(|(id, parts)| {
            (parts.len() > base).then(|| {
                let relative = parts[base..].join("\\");
                let directory = namespace
                    .as_deref()
                    .map_or(relative.clone(), |root| format!("{root}\\{relative}"));
                (id, directory)
            })
        }));
    }
    directories
}
