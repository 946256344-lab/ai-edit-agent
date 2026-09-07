//! 预览中间视频缓存：源版本、源区间与画面参数决定复用，文字和音轨不影响底片。
use crate::models::TimelineClip;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::UNIX_EPOCH};

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
