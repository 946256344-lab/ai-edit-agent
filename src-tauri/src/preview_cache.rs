//! 预览中间视频缓存：源版本、源区间与画面参数决定复用；按项目限制占用并支持显式清理。
use crate::models::TimelineClip;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tauri::{AppHandle, Manager};

/// 单项目 `previews/cache/<projectId>` 占用上限（成功写入后按修改时间淘汰最旧文件）。
pub(crate) const PROJECT_CACHE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCacheStatus {
    pub project_id: String,
    pub bytes_used: u64,
    pub limit_bytes: u64,
    pub file_count: usize,
}

pub(crate) fn key(value: &impl serde::Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("preview cache key"))
    )
}

pub(crate) fn clip_key(source: &Path, kind: &str, clip: &TimelineClip) -> Result<String, String> {
    let metadata = fs::metadata(source).map_err(|error| error.to_string())?;
    let modified = metadata
        .modified()
        .map_err(|error| error.to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    Ok(key(&json!({
        "renderer": "vertical-540x960-30-x264-v2",
        "source": source, "size": metadata.len(), "modified": modified.to_string(),
        "kind": kind, "clipKind": clip.clip_kind,
        "start": clip.source_start_ms, "end": clip.source_end_ms,
        "duration": clip.timeline_end_ms - clip.timeline_start_ms,
        "cropFocus": clip.crop_focus,
    })))
}

pub(crate) fn project_cache_dir(app: &AppHandle, project_id: &str) -> Result<PathBuf, String> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return Err("Project identifier is required.".to_owned());
    }
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("previews")
        .join("cache")
        .join(project_id))
}

fn cache_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("previews")
        .join("cache"))
}

fn list_cache_files(cache: &Path) -> Result<Vec<(PathBuf, u64, u64)>, String> {
    if !cache.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(cache).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        files.push((path, metadata.len(), modified));
    }
    Ok(files)
}

pub(crate) fn measure_project_cache(cache: &Path) -> Result<(u64, usize), String> {
    let files = list_cache_files(cache)?;
    let bytes = files.iter().map(|(_, size, _)| *size).sum();
    Ok((bytes, files.len()))
}

/// 超出上限时按修改时间从旧到新删除，直到占用不超过 `limit_bytes`。返回释放字节数。
pub(crate) fn enforce_project_cache_limit(cache: &Path, limit_bytes: u64) -> Result<u64, String> {
    let mut files = list_cache_files(cache)?;
    let mut total: u64 = files.iter().map(|(_, size, _)| *size).sum();
    if total <= limit_bytes {
        return Ok(0);
    }
    files.sort_by_key(|(_, _, modified)| *modified);
    let mut freed = 0_u64;
    for (path, size, _) in files {
        if total <= limit_bytes {
            break;
        }
        match fs::remove_file(&path) {
            Ok(()) => {
                total = total.saturating_sub(size);
                freed = freed.saturating_add(size);
            }
            Err(error) => {
                log::warn!(
                    "Could not evict preview cache file {}: {error}",
                    path.display()
                );
            }
        }
    }
    Ok(freed)
}

pub(crate) fn clear_project_cache_dir(cache: &Path) -> Result<u64, String> {
    if !cache.exists() {
        return Ok(0);
    }
    let (bytes, _) = measure_project_cache(cache)?;
    fs::remove_dir_all(cache).map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// 删除已不在项目表中的缓存目录（覆盖“项目已删但缓存残留”）。
pub(crate) fn remove_orphan_project_caches(
    app: &AppHandle,
    known_project_ids: &[String],
) -> Result<u64, String> {
    let root = cache_root(app)?;
    if !root.exists() {
        return Ok(0);
    }
    let known: std::collections::HashSet<&str> =
        known_project_ids.iter().map(String::as_str).collect();
    let mut freed = 0_u64;
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if known.contains(name) {
            continue;
        }
        let (bytes, _) = measure_project_cache(&path).unwrap_or((0, 0));
        match fs::remove_dir_all(&path) {
            Ok(()) => freed = freed.saturating_add(bytes),
            Err(error) => log::warn!(
                "Could not remove orphan preview cache {}: {error}",
                path.display()
            ),
        }
    }
    Ok(freed)
}

#[tauri::command]
pub fn get_preview_cache_status(
    app: AppHandle,
    project_id: String,
) -> Result<PreviewCacheStatus, String> {
    let cache = project_cache_dir(&app, &project_id)?;
    let (bytes_used, file_count) = measure_project_cache(&cache)?;
    Ok(PreviewCacheStatus {
        project_id: project_id.trim().to_owned(),
        bytes_used,
        limit_bytes: PROJECT_CACHE_LIMIT_BYTES,
        file_count,
    })
}

#[tauri::command]
pub fn clear_preview_cache(
    app: AppHandle,
    project_id: String,
    confirmed: bool,
) -> Result<PreviewCacheStatus, String> {
    if !confirmed {
        return Err("Clearing preview cache requires explicit confirmation.".to_owned());
    }
    let cache = project_cache_dir(&app, &project_id)?;
    let _ = clear_project_cache_dir(&cache)?;
    get_preview_cache_status(app, project_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn enforce_limit_reduces_usage_to_budget() {
        let directory =
            std::env::temp_dir().join(format!("preview-cache-limit-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("a.mp4"), vec![0_u8; 80]).unwrap();
        fs::write(directory.join("b.mp4"), vec![0_u8; 80]).unwrap();
        fs::write(directory.join("c.mp4"), vec![0_u8; 80]).unwrap();

        let freed = enforce_project_cache_limit(&directory, 100).unwrap();
        assert!(freed >= 80);
        let (bytes, count) = measure_project_cache(&directory).unwrap();
        assert!(bytes <= 100);
        assert!(count <= 1);
        fs::remove_dir_all(directory).unwrap();
    }
}
