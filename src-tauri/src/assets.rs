//! 素材导入、重链路、媒体收集与 Agent 检索入口。
//! 分析队列、健康检查与库查询由各子模块负责；原始媒体不在这里被删除。
//! 失败对调用方封闭，不得静默修改其他任务或产物。

pub mod analysis;
pub mod health;
pub mod library;
pub mod visual;

// ---- 外部模块需要的 pub(crate) re-exports ----
// library：目录投影与 Agent 列表查询
pub(crate) use library::{
    asset_directory_nodes, asset_public_folder_metadata, asset_safe_directory,
    legacy_asset_directories, list_assets_for_agent, ASSET_PAGE_FILTER_SQL,
};
// analysis：分析队列与恢复
pub(crate) use analysis::{
    drain_pending_analysis, request_asset_analysis, resume_incomplete_analysis,
};
// visual：视觉批次优先级与等待
pub(crate) use visual::{prioritize_pending_visual_batches, wait_for_visual_batch};
// health：Agent 健康摘要
pub(crate) use health::get_asset_health_summary_for_agent;

// ---- 内部辅助：从子模块引用 ----
use analysis::{
    asset_kind, collect_media_files, enqueue_technical_analysis, spawn_technical_analysis_tasks,
    DRAIN_ANALYSIS_BATCH,
};
use health::modified_millis;

use crate::db::{now_millis, open_connection};
use crate::models::{
    Asset, AssetRelinkMatch, AssetRelinkPreview, AssetRelinkResult, CollectProjectMediaPreview,
    CollectProjectMediaResult, SceneSegment, TechnicalMetadata,
};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::AppHandle;
use uuid::Uuid;

/// 将已确认的源文件批量写入 SQLite 并排队技术分析；不删除素材。
/// 事实所有者：assets 表；失败回滚整个事务。
pub(crate) fn store_assets(
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
        let source_reference = source.to_string_lossy().into_owned();
        let folder_reference = folder_reference.map(|path| path.to_string_lossy().into_owned());
        let directory_key = asset_safe_directory(&source_reference, folder_reference.as_deref());
        let (folder_name, relative_path) =
            asset_public_folder_metadata(directory_key.as_deref(), &display_name);
        let asset = Asset {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.to_owned(),
            kind: asset_kind(&source),
            display_name,
            folder_name,
            relative_path,
            directory_key,
            analysis_status: "queued".to_owned(),
            visual_analysis_status: "queued".to_owned(),
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
            favorite: false,
            rating: 0,
            note: String::new(),
            excluded: false,
            user_tags: Vec::new(),
            collection_ids: Vec::new(),
            source_health_status: "online".to_owned(),
            source_health_checked_at: Some(timestamp),
            created_at: timestamp,
            updated_at: timestamp,
        };
        transaction.execute(
            "INSERT INTO assets (id, project_id, kind, display_name, source_reference, folder_reference, analysis_status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![asset.id, asset.project_id, asset.kind, asset.display_name, source_reference, folder_reference, asset.analysis_status, asset.created_at, asset.updated_at],
        ).map_err(|error| error.to_string())?;
        let metadata = fs::metadata(&source).map_err(|error| error.to_string())?;
        transaction.execute(
            "INSERT INTO asset_source_health (asset_id, project_id, status, baseline_size, baseline_modified_ms, observed_size, observed_modified_ms, checked_at, updated_at) VALUES (?1, ?2, 'online', ?3, ?4, ?3, ?4, ?5, ?5)",
            params![asset.id, project_id, i64::try_from(metadata.len()).ok(), modified_millis(&metadata), timestamp],
        ).map_err(|error| error.to_string())?;
        imported.push(asset);
    }
    transaction.commit().map_err(|error| error.to_string())?;
    enqueue_technical_analysis(app, &imported)?;
    Ok(imported)
}

/// 已下载的音频文件直接写入 assets 并用展示名覆盖文件名；music 模块专用。
pub(crate) fn store_downloaded_audio(
    app: &AppHandle,
    project_id: &str,
    source: PathBuf,
    display_name: &str,
) -> Result<Asset, String> {
    let mut assets = store_assets(app, project_id, vec![source], None)?;
    let asset = assets
        .pop()
        .ok_or_else(|| "Could not import downloaded music.".to_owned())?;
    let connection = open_connection(app)?;
    connection
        .execute(
            "UPDATE assets SET display_name = ?1 WHERE id = ?2",
            params![display_name, asset.id],
        )
        .map_err(|error| error.to_string())?;
    Ok(asset)
}

/// 仅用于后台 Agent 等待已排队的下载音乐完成分析；失败不重试，ready 门不变。
pub(crate) fn wait_for_asset_ready(
    app: &AppHandle,
    project_id: &str,
    asset_id: &str,
) -> Result<Asset, String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    loop {
        let assets = library::list_assets(app.clone(), project_id.to_owned())?;
        let asset = assets
            .into_iter()
            .find(|asset| asset.id == asset_id)
            .ok_or_else(|| "Downloaded music is no longer available in this project.".to_owned())?;
        match asset.analysis_status.as_str() {
            "ready" => return Ok(asset),
            "failed" => {
                return Err(
                    "Downloaded music could not be analyzed and was not added to the timeline."
                        .to_owned(),
                )
            }
            _ if std::time::Instant::now() >= deadline => return Err(
                "Downloaded music analysis is still running; it was not added to the timeline yet."
                    .to_owned(),
            ),
            _ => std::thread::sleep(Duration::from_millis(250)),
        }
    }
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

/// 从候选目录中找出与本项目素材的相对路径和媒体类型唯一匹配的源文件。
fn relink_candidates(
    connection: &rusqlite::Connection,
    project_id: &str,
    source_directory: &Path,
) -> Result<Vec<(String, String, PathBuf)>, String> {
    if !source_directory.is_dir() {
        return Err("The selected folder is no longer available.".to_owned());
    }
    let mut sources = Vec::new();
    collect_media_files(source_directory, &mut sources)?;
    let available: HashSet<String> = sources
        .iter()
        .filter_map(|source| {
            source
                .strip_prefix(source_directory)
                .ok()?
                .to_str()
                .map(|relative| relative.to_ascii_lowercase())
        })
        .collect();
    let mut statement = connection
        .prepare(
            "SELECT id, display_name, kind, source_reference, folder_reference FROM assets WHERE project_id = ?1",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    let mut relative_counts = HashMap::new();
    let mut candidates = Vec::new();
    for (asset_id, display_name, kind, source_reference, folder_reference) in rows {
        let Some(folder_reference) = folder_reference else {
            continue;
        };
        let Some(relative) = Path::new(&source_reference)
            .strip_prefix(Path::new(&folder_reference))
            .ok()
            .and_then(|path| path.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        let key = relative.to_ascii_lowercase();
        *relative_counts.entry(key.clone()).or_insert(0usize) += 1;
        candidates.push((
            asset_id,
            display_name,
            kind,
            key,
            source_directory.join(relative),
        ));
    }
    Ok(candidates
        .into_iter()
        .filter(|(_, _, kind, relative, candidate)| {
            relative_counts.get(relative) == Some(&1)
                && available.contains(relative)
                && asset_kind(candidate) == kind.as_str()
        })
        .map(|(asset_id, display_name, _, _, candidate)| (asset_id, display_name, candidate))
        .collect())
}

#[tauri::command]
pub fn preview_asset_relink(
    app: AppHandle,
    project_id: String,
    source_directory: String,
) -> Result<AssetRelinkPreview, String> {
    let connection = open_connection(&app)?;
    let candidates = relink_candidates(&connection, &project_id, Path::new(&source_directory))?;
    let asset_count: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let matches = candidates
        .into_iter()
        .map(|(asset_id, display_name, _)| AssetRelinkMatch {
            asset_id,
            display_name,
        })
        .collect::<Vec<_>>();
    Ok(AssetRelinkPreview {
        unmatched_count: asset_count.saturating_sub(matches.len()),
        matches,
    })
}

#[tauri::command]
pub fn confirm_asset_relink(
    app: AppHandle,
    project_id: String,
    source_directory: String,
    asset_ids: Vec<String>,
    preserve_analysis: bool,
) -> Result<AssetRelinkResult, String> {
    if asset_ids.is_empty() {
        return Err("No verified source matches were selected.".to_owned());
    }
    let connection = open_connection(&app)?;
    let selected: HashSet<String> = asset_ids.into_iter().collect();
    let candidates = relink_candidates(&connection, &project_id, Path::new(&source_directory))?;
    let timestamp = now_millis();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut relinked_count = 0usize;
    let mut tasks = Vec::new();
    for (asset_id, _, source) in candidates {
        if !selected.contains(&asset_id) {
            continue;
        }
        let new_kind = asset_kind(&source);
        if preserve_analysis {
            transaction.execute(
                "UPDATE assets SET source_reference = ?1, folder_reference = ?2, kind = ?3, updated_at = ?4 WHERE id = ?5 AND project_id = ?6",
                params![source.to_string_lossy(), source_directory.as_str(), new_kind, timestamp, asset_id, project_id],
            ).map_err(|error| error.to_string())?;
        } else {
            transaction.execute(
                "UPDATE agent_tasks SET status = 'cancelled', error_message = 'Superseded after the source file was relinked.', updated_at = ?1 WHERE project_id = ?2 AND tool_name = 'analyze_asset' AND status IN ('queued', 'running') AND input_json LIKE ?3",
                params![timestamp, project_id, format!("%\"assetId\":\"{asset_id}\"%")],
            ).map_err(|error| error.to_string())?;
            transaction.execute(
                "UPDATE assets SET source_reference = ?1, folder_reference = ?2, kind = ?3, analysis_status = 'queued', metadata_json = '{}', updated_at = ?4 WHERE id = ?5 AND project_id = ?6",
                params![source.to_string_lossy(), source_directory.as_str(), new_kind, timestamp, asset_id, project_id],
            ).map_err(|error| error.to_string())?;
            let task_id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset', 'queued', ?3, ?4, ?5)",
                params![task_id, project_id, serde_json::json!({ "assetId": asset_id }).to_string(), timestamp, timestamp],
            ).map_err(|error| error.to_string())?;
            tasks.push((asset_id.clone(), task_id));
        }
        let source_metadata = fs::metadata(&source).map_err(|error| error.to_string())?;
        transaction.execute(
            "INSERT INTO asset_source_health (asset_id, project_id, status, baseline_size, baseline_modified_ms, observed_size, observed_modified_ms, reason_code, checked_at, updated_at) VALUES (?1, ?2, 'online', ?3, ?4, ?3, ?4, NULL, ?5, ?5) ON CONFLICT(asset_id) DO UPDATE SET status = 'online', baseline_size = excluded.baseline_size, baseline_modified_ms = excluded.baseline_modified_ms, observed_size = excluded.observed_size, observed_modified_ms = excluded.observed_modified_ms, reason_code = NULL, checked_at = excluded.checked_at, updated_at = excluded.updated_at",
            params![asset_id, project_id, i64::try_from(source_metadata.len()).ok(), modified_millis(&source_metadata), timestamp],
        ).map_err(|error| error.to_string())?;
        relinked_count += 1;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    if relinked_count == 0 {
        return Err(
            "The selected folder no longer contains the verified source matches.".to_owned(),
        );
    }
    tasks.truncate(DRAIN_ANALYSIS_BATCH);
    spawn_technical_analysis_tasks(app.clone(), tasks);
    Ok(AssetRelinkResult { relinked_count })
}

fn collectable_project_sources(
    connection: &rusqlite::Connection,
    project_id: &str,
) -> Result<Vec<(String, String, String)>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, display_name, source_reference FROM assets WHERE project_id=?1 ORDER BY created_at, id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub fn preview_collect_project_media(
    app: AppHandle,
    project_id: String,
) -> Result<CollectProjectMediaPreview, String> {
    let connection = open_connection(&app)?;
    let mut collectable_count = 0usize;
    let mut unavailable_count = 0usize;
    let mut total_bytes = 0i64;
    for (_, _, source) in collectable_project_sources(&connection, &project_id)? {
        match fs::metadata(source) {
            Ok(metadata) if metadata.is_file() => {
                collectable_count += 1;
                total_bytes =
                    total_bytes.saturating_add(i64::try_from(metadata.len()).unwrap_or(i64::MAX));
            }
            _ => unavailable_count += 1,
        }
    }
    Ok(CollectProjectMediaPreview {
        collectable_count,
        unavailable_count,
        total_bytes,
    })
}

fn safe_collected_name(display_name: &str, asset_id: &str) -> String {
    let path = Path::new(display_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("media")
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let suffix = asset_id.chars().take(8).collect::<String>();
    match path.extension().and_then(|value| value.to_str()) {
        Some(extension) => format!("{stem}-{suffix}.{extension}"),
        None => format!("{stem}-{suffix}"),
    }
}

#[tauri::command]
pub fn collect_project_media(
    app: AppHandle,
    project_id: String,
    destination_directory: String,
) -> Result<CollectProjectMediaResult, String> {
    let destination = PathBuf::from(destination_directory);
    if !destination.is_dir() {
        return Err("The selected collection destination is unavailable.".to_owned());
    }
    let connection = open_connection(&app)?;
    let package = destination.join(format!("assembly-media-{}", Uuid::new_v4().simple()));
    let media_directory = package.join("media");
    fs::create_dir(&package).map_err(|error| error.to_string())?;
    fs::create_dir(&media_directory).map_err(|error| error.to_string())?;
    let mut copied = Vec::new();
    let mut unavailable_count = 0usize;
    for (asset_id, display_name, source) in collectable_project_sources(&connection, &project_id)? {
        if !Path::new(&source).is_file() {
            unavailable_count += 1;
            continue;
        }
        let collected_name = safe_collected_name(&display_name, &asset_id);
        let target = media_directory.join(&collected_name);
        match fs::copy(&source, &target) {
            Ok(bytes) => copied.push(serde_json::json!({
                "assetId": asset_id,
                "displayName": display_name,
                "collectedFile": format!("media/{collected_name}"),
                "bytes": bytes
            })),
            Err(_) => unavailable_count += 1,
        }
    }
    let manifest = serde_json::json!({
        "format": "assembly-video-agent-media-collection-v1",
        "projectId": project_id,
        "createdAt": now_millis(),
        "assets": copied,
        "unavailableCount": unavailable_count
    });
    fs::write(
        package.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let timestamp = now_millis();
    connection
        .execute(
            "INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, after_json, created_at) VALUES (?1, ?2, 'user', 'collect_project_media', 'project', ?2, ?3, ?4)",
            params![
                Uuid::new_v4().to_string(),
                project_id,
                serde_json::json!({"copiedCount": copied.len(), "unavailableCount": unavailable_count}).to_string(),
                timestamp
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(CollectProjectMediaResult {
        copied_count: copied.len(),
        unavailable_count,
        output_directory: package.to_string_lossy().into_owned(),
    })
}

/// Agent 受限只读素材检索；排除禁用素材，最多 20 条，不返回绝对路径或备注正文。
pub(crate) fn search_assets_for_agent(
    connection: &rusqlite::Connection,
    project_id: &str,
    query: Option<&str>,
    kind: Option<&str>,
    min_duration_ms: Option<i64>,
    max_duration_ms: Option<i64>,
    min_rating: Option<i64>,
    favorite_only: bool,
    tag: Option<&str>,
    collection_id: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<Value, String> {
    let query = query.map(str::trim).filter(|value| !value.is_empty());
    if query.is_some_and(|value| value.chars().count() > 200)
        || !matches!(kind, None | Some("video" | "image" | "audio" | "other"))
        || !matches!(min_rating, None | Some(0..=5))
        || min_duration_ms.is_some_and(|value| value < 0)
        || max_duration_ms.is_some_and(|value| value < 0)
    {
        return Err("Asset search arguments are outside the allowed range.".to_owned());
    }
    let limit = limit.clamp(1, 20);
    let offset = offset.min(10_000);
    let search_sql = "a.project_id = ?1
        AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = a.id), 0) = 0
        AND (?2 IS NULL OR a.kind = ?2)
        AND (?3 IS NULL OR coalesce(json_extract(a.metadata_json, '$.durationMs'), 0) >= ?3)
        AND (?4 IS NULL OR coalesce(json_extract(a.metadata_json, '$.durationMs'), 0) <= ?4)
        AND (?5 IS NULL OR coalesce((SELECT rating FROM asset_user_metadata um WHERE um.asset_id = a.id), 0) >= ?5)
        AND (?6 = 0 OR coalesce((SELECT favorite FROM asset_user_metadata um WHERE um.asset_id = a.id), 0) = 1)
        AND (?7 IS NULL OR EXISTS (SELECT 1 FROM asset_tag_assignments ata JOIN asset_tags t ON t.id = ata.tag_id WHERE ata.asset_id = a.id AND t.name = ?7 COLLATE NOCASE))
        AND (?8 IS NULL OR EXISTS (SELECT 1 FROM asset_collection_items aci JOIN asset_collections c ON c.id = aci.collection_id WHERE aci.asset_id = a.id AND c.id = ?8 AND c.project_id = ?1))
        AND (?9 IS NULL OR instr(lower(a.display_name || ' ' || a.metadata_json || ' ' || coalesce((SELECT group_concat(t.name, ' ') FROM asset_tag_assignments ata JOIN asset_tags t ON t.id = ata.tag_id WHERE ata.asset_id = a.id), '') || ' ' || coalesce((SELECT note FROM asset_user_metadata um WHERE um.asset_id = a.id), '')), lower(?9)) > 0)";
    let total: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM assets a WHERE {search_sql}"),
            params![
                project_id,
                kind,
                min_duration_ms,
                max_duration_ms,
                min_rating,
                i64::from(favorite_only),
                tag,
                collection_id,
                query
            ],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mut statement = connection.prepare(&format!(
        "SELECT a.id, a.display_name, a.kind, a.analysis_status, a.metadata_json,
         coalesce((SELECT rating FROM asset_user_metadata um WHERE um.asset_id = a.id), 0),
         coalesce((SELECT favorite FROM asset_user_metadata um WHERE um.asset_id = a.id), 0),
         coalesce((SELECT json_group_array(t.name) FROM asset_tag_assignments ata JOIN asset_tags t ON t.id = ata.tag_id WHERE ata.asset_id = a.id), '[]')
         FROM assets a WHERE {search_sql}
         ORDER BY coalesce((SELECT favorite FROM asset_user_metadata um WHERE um.asset_id = a.id), 0) DESC,
         coalesce((SELECT rating FROM asset_user_metadata um WHERE um.asset_id = a.id), 0) DESC,
         a.updated_at DESC, a.id DESC LIMIT ?10 OFFSET ?11"
    )).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![
                project_id,
                kind,
                min_duration_ms,
                max_duration_ms,
                min_rating,
                i64::from(favorite_only),
                tag,
                collection_id,
                query,
                limit as i64,
                offset as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)? != 0,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let query_lower = query.map(|value| value.to_lowercase());
    let mut candidates = Vec::new();
    for row in rows {
        let (id, name, kind, analysis_status, metadata_json, rating, favorite, tags_json) =
            row.map_err(|error| error.to_string())?;
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        let mut reasons = Vec::new();
        if let Some(query) = query_lower.as_deref() {
            if name.to_lowercase().contains(query) {
                reasons.push("display_name_match");
            }
            if tags
                .iter()
                .any(|value| value.to_lowercase().contains(query))
            {
                reasons.push("user_tag_match");
            }
            if metadata
                .ocr_evidence
                .iter()
                .any(|value| value.text.to_lowercase().contains(query))
            {
                reasons.push("ocr_match");
            }
            if metadata.visual_evidence.iter().any(|value| {
                value
                    .scene
                    .as_deref()
                    .is_some_and(|scene| scene.to_lowercase().contains(query))
                    || value
                        .subjects
                        .iter()
                        .chain(&value.actions)
                        .chain(&value.products)
                        .any(|item| item.to_lowercase().contains(query))
            }) {
                reasons.push("visual_evidence_match");
            }
            if reasons.is_empty() {
                reasons.push("local_note_match");
            }
        }
        if favorite {
            reasons.push("favorite");
        }
        if rating > 0 {
            reasons.push("rated");
        }
        candidates.push(serde_json::json!({
            "assetId": id, "displayName": name, "kind": kind,
            "analysisStatus": analysis_status, "visualAnalysisStatus": metadata.visual_analysis_status,
            "durationMs": metadata.duration_ms, "sceneCount": metadata.scene_segments.len(),
            "rating": rating, "favorite": favorite, "userTags": tags,
            "matchReasons": reasons
        }));
    }
    let next_offset =
        (offset + candidates.len() < total as usize).then_some(offset + candidates.len());
    Ok(serde_json::json!({
        "candidates": candidates, "total": total,
        "nextOffset": next_offset, "limit": limit
    }))
}

/// Agent 受限只读片段检索；基于真实场景段和时间点证据，排除禁用及健康异常素材。
pub(crate) fn search_asset_segments_for_agent(
    connection: &rusqlite::Connection,
    project_id: &str,
    query: &str,
    asset_id: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<Value, String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() || query.chars().count() > 200 {
        return Err("Segment search needs a bounded query.".to_owned());
    }
    let limit = limit.clamp(1, 20);
    let offset = offset.min(10_000);
    let mut statement = connection.prepare(
        "SELECT a.id, a.display_name, a.kind, a.metadata_json FROM assets a
         WHERE a.project_id=?1 AND a.analysis_status='ready' AND a.kind IN ('video','image')
         AND (?2 IS NULL OR a.id=?2)
         AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id=a.id),0)=0
         AND coalesce((SELECT status FROM asset_source_health h WHERE h.asset_id=a.id),'unchecked') NOT IN ('missing','changed','unreadable')
         ORDER BY a.updated_at DESC, a.id DESC",
    ).map_err(|error| error.to_string())?;
    let assets = statement
        .query_map(params![project_id, asset_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut matches = Vec::new();
    for (id, name, kind, metadata_json) in assets {
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        let segments = if kind == "image" {
            vec![SceneSegment {
                start_ms: 0,
                end_ms: 0,
                scene_duration_ms: None,
                visual_quality_score: None,
            }]
        } else {
            metadata.scene_segments.clone()
        };
        for segment in segments {
            let contains_time = |time: Option<i64>| {
                time.is_some_and(|value| value >= segment.start_ms && value <= segment.end_ms)
            };
            let ocr_match = metadata.ocr_evidence.iter().any(|evidence| {
                contains_time(evidence.time_ms) && evidence.text.to_lowercase().contains(&query)
            });
            let mut labels = Vec::new();
            for evidence in metadata
                .visual_evidence
                .iter()
                .filter(|evidence| contains_time(evidence.time_ms))
            {
                for label in evidence
                    .subjects
                    .iter()
                    .chain(&evidence.actions)
                    .chain(&evidence.products)
                    .chain(evidence.scene.iter())
                {
                    if label.to_lowercase().contains(&query) && !labels.contains(label) {
                        labels.push(label.clone());
                    }
                }
            }
            if ocr_match || !labels.is_empty() || name.to_lowercase().contains(&query) {
                matches.push(serde_json::json!({
                    "assetId": id, "displayName": name, "kind": kind,
                    "sourceStartMs": segment.start_ms, "sourceEndMs": segment.end_ms,
                    "matchReasons": [if ocr_match { "ocr_match" } else if !labels.is_empty() { "visual_evidence_match" } else { "name_match" }],
                    "matchedVisualLabels": labels.into_iter().take(8).collect::<Vec<_>>()
                }));
            }
        }
    }
    let total = matches.len();
    let results = matches
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let next_offset = (offset + results.len() < total).then_some(offset + results.len());
    Ok(serde_json::json!({
        "segments": results, "total": total,
        "nextOffset": next_offset, "limit": limit
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_collected_name_sanitizes_forbidden_characters() {
        let name = safe_collected_name("my:file<name>.mp4", "abcdef12");
        assert!(!name.contains(':'));
        assert!(!name.contains('<'));
        assert!(name.ends_with(".mp4"));
        assert!(name.contains("abcdef12"));
    }

    #[test]
    fn safe_collected_name_appends_asset_id_suffix() {
        let name = safe_collected_name("clip.mov", "1234567890");
        assert!(name.contains("12345678"));
        assert!(name.ends_with(".mov"));
    }

    #[test]
    fn search_assets_for_agent_rejects_out_of_range_arguments() {
        // No DB needed — validation fires before any query.
        let result = search_assets_for_agent(
            // We can't construct a real connection without a DB path, so we test via
            // the argument validation path by providing an invalid kind.
            &rusqlite::Connection::open_in_memory().unwrap(),
            "proj",
            None,
            Some("invalid_kind"),
            None,
            None,
            None,
            false,
            None,
            None,
            0,
            20,
        );
        assert!(result.is_err());
    }
}
