//! 本地 CLIP ViT-B/32：beat 文案 ↔ 片段代表帧的图文相似度。
//! 模型随安装包分发，不联网下载；向量写入 asset_segment_embeddings（独立 model 名），缺失时评分回退到 bge/词面。

use crate::db::now_millis;
use crate::models::TechnicalMetadata;
use fastembed::{
    ImageEmbedding, ImageInitOptionsUserDefined, InitOptionsUserDefined, Pooling, TextEmbedding,
    TokenizerFiles, UserDefinedEmbeddingModel, UserDefinedImageEmbeddingModel,
};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

pub(crate) const CLIP_VISION_MODEL: &str = "Qdrant/clip-ViT-B-32-vision";
pub(crate) const CLIP_TEXT_MODEL: &str = "Qdrant/clip-ViT-B-32-text";
pub(crate) const CLIP_DIMENSIONS: usize = 512;
pub(crate) const CLIP_VERSION: u32 = 1;
const VISION_RESOURCE_DIRECTORY: &str = "resources/models/clip-ViT-B-32-vision";
const TEXT_RESOURCE_DIRECTORY: &str = "resources/models/clip-ViT-B-32-text";
/// Qdrant/clip-ViT-B-32-vision model.onnx SHA-256
const VISION_MODEL_SHA256: &str =
    "c68d3d9a200ddd2a8c8a5510b576d4c94d1ae383bf8b36dd8c084f94e1fb4d63";
/// Qdrant/clip-ViT-B-32-text model.onnx SHA-256
const TEXT_MODEL_SHA256: &str = "4dbe762b11e36488304471e439cde89da053ad7acaddbf9e096745d142ec8d8b";
const CLIP_BATCH_SIZE: usize = 8;
const MAX_CLIP_TEXT_CHARS: usize = 500;

static VISION_MODEL: OnceLock<Result<ImageEmbedding, String>> = OnceLock::new();
static TEXT_MODEL: OnceLock<Result<TextEmbedding, String>> = OnceLock::new();

fn bundled_directory(
    app: &AppHandle,
    resource: &str,
    required_file: &str,
) -> Result<PathBuf, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|_| "clip_model_resource_unavailable".to_owned())?;
    let packaged = resource_dir.join(resource);
    if packaged.join(required_file).is_file() {
        return Ok(packaged);
    }
    let leaf = resource.rsplit('/').next().unwrap_or(resource);
    let flattened = resource_dir.join("models").join(leaf);
    if flattened.join(required_file).is_file() {
        return Ok(flattened);
    }
    #[cfg(debug_assertions)]
    {
        let development = Path::new(env!("CARGO_MANIFEST_DIR")).join(resource);
        if development.join(required_file).is_file() {
            return Ok(development);
        }
    }
    Err("clip_model_resource_unavailable".to_owned())
}

pub(crate) fn bundled_models_present(app: &AppHandle) -> Result<bool, String> {
    Ok(
        bundled_directory(app, VISION_RESOURCE_DIRECTORY, "model.onnx").is_ok()
            && bundled_directory(app, TEXT_RESOURCE_DIRECTORY, "model.onnx").is_ok(),
    )
}

fn read_model_file(directory: &Path, relative_path: &str) -> Result<Vec<u8>, String> {
    fs::read(directory.join(relative_path))
        .map_err(|_| "clip_model_resource_unavailable".to_owned())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn load_vision_from_directory(directory: &Path) -> Result<ImageEmbedding, String> {
    let onnx_file = read_model_file(directory, "model.onnx")?;
    if hash_bytes(&onnx_file) != VISION_MODEL_SHA256 {
        return Err("clip_vision_model_integrity_failed".to_owned());
    }
    let preprocessor_file = read_model_file(directory, "preprocessor_config.json")?;
    let model = UserDefinedImageEmbeddingModel::new(onnx_file, preprocessor_file);
    ImageEmbedding::try_new_from_user_defined(model, ImageInitOptionsUserDefined::new())
        .map_err(|_| "clip_vision_model_load_failed".to_owned())
}

fn load_text_from_directory(directory: &Path) -> Result<TextEmbedding, String> {
    let onnx_file = read_model_file(directory, "model.onnx")?;
    if hash_bytes(&onnx_file) != TEXT_MODEL_SHA256 {
        return Err("clip_text_model_integrity_failed".to_owned());
    }
    let tokenizer_files = TokenizerFiles {
        tokenizer_file: read_model_file(directory, "tokenizer.json")?,
        config_file: read_model_file(directory, "config.json")?,
        special_tokens_map_file: read_model_file(directory, "special_tokens_map.json")?,
        tokenizer_config_file: read_model_file(directory, "tokenizer_config.json")?,
    };
    let model =
        UserDefinedEmbeddingModel::new(onnx_file, tokenizer_files).with_pooling(Pooling::Mean);
    TextEmbedding::try_new_from_user_defined(
        model,
        InitOptionsUserDefined::new().with_max_length(77),
    )
    .map_err(|_| "clip_text_model_load_failed".to_owned())
}

fn vision_model(app: &AppHandle) -> Result<&'static ImageEmbedding, String> {
    VISION_MODEL
        .get_or_init(|| {
            bundled_directory(app, VISION_RESOURCE_DIRECTORY, "model.onnx")
                .and_then(|path| load_vision_from_directory(&path))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn text_model(app: &AppHandle) -> Result<&'static TextEmbedding, String> {
    TEXT_MODEL
        .get_or_init(|| {
            bundled_directory(app, TEXT_RESOURCE_DIRECTORY, "model.onnx")
                .and_then(|path| load_text_from_directory(&path))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn beat_clip_query(beat: &crate::models::StoryboardBeat) -> String {
    let keywords = beat.visual_keywords.join(" ");
    format!("{keywords} {}", beat.required_visual)
        .chars()
        .take(MAX_CLIP_TEXT_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

pub(crate) fn encode_beats(
    app: &AppHandle,
    beats: &[crate::models::StoryboardBeat],
) -> Result<Vec<Vec<f32>>, String> {
    crate::execution_deadline::check()?;
    if beats.is_empty() {
        return Ok(Vec::new());
    }
    let texts = beats
        .iter()
        .map(|beat| {
            let query = beat_clip_query(beat);
            if query.is_empty() {
                beat.purpose
                    .chars()
                    .take(MAX_CLIP_TEXT_CHARS)
                    .collect::<String>()
            } else {
                query
            }
        })
        .collect::<Vec<_>>();
    let embeddings = text_model(app)?
        .embed(texts, Some(CLIP_BATCH_SIZE))
        .map_err(|_| "clip_text_inference_failed".to_owned())?;
    crate::execution_deadline::check()?;
    if embeddings
        .iter()
        .any(|embedding| embedding.len() != CLIP_DIMENSIONS)
    {
        return Err("clip_text_dimension_mismatch".to_owned());
    }
    Ok(embeddings)
}

fn embedding_blob(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn embedding_from_blob(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.len() % 4 != 0 {
        return None;
    }
    let mut values = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        values.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Some(values)
}

fn representative_frame_path(segment: &crate::models::SceneSegment) -> Option<&str> {
    if segment.frames.is_empty() {
        return None;
    }
    let mid = segment.start_ms + (segment.end_ms - segment.start_ms).max(0) / 2;
    segment
        .frames
        .iter()
        .min_by_key(|frame| (frame.time_ms - mid).abs())
        .map(|frame| frame.image_path.as_str())
}

/// 为有代表帧的片段写入 CLIP 图像向量（CAS by 帧内容 hash）。
pub(crate) fn refresh_segment_clip_embeddings(
    app: &AppHandle,
    asset_id: &str,
    metadata: &TechnicalMetadata,
) -> Result<usize, String> {
    let connection = crate::db::open_connection(app)?;
    let mut pending: Vec<(String, Vec<u8>, String)> = Vec::new();
    for segment in &metadata.scene_segments {
        if segment.id.is_empty() {
            continue;
        }
        let Some(path) = representative_frame_path(segment) else {
            continue;
        };
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }
        let hash = hash_bytes(&bytes);
        let current_hash: Option<String> = connection
            .query_row(
                "SELECT source_hash FROM asset_segment_embeddings WHERE asset_id = ?1 AND segment_id = ?2 AND model = ?3 AND version = ?4",
                params![asset_id, segment.id, CLIP_VISION_MODEL, CLIP_VERSION as i64],
                |row| row.get(0),
            )
            .ok();
        if current_hash.as_deref() == Some(hash.as_str()) {
            continue;
        }
        pending.push((segment.id.clone(), bytes, hash));
    }
    if pending.is_empty() {
        return Ok(0);
    }

    crate::execution_deadline::check()?;
    let image_refs = pending
        .iter()
        .map(|(_, bytes, _)| bytes.as_slice())
        .collect::<Vec<_>>();
    let embeddings = vision_model(app)?
        .embed_bytes(&image_refs, Some(CLIP_BATCH_SIZE))
        .map_err(|_| "clip_vision_inference_failed".to_owned())?;
    crate::execution_deadline::check()?;
    if embeddings.len() != pending.len()
        || embeddings
            .iter()
            .any(|embedding| embedding.len() != CLIP_DIMENSIONS)
    {
        return Err("clip_vision_dimension_mismatch".to_owned());
    }

    let timestamp = now_millis();
    let mut updated = 0;
    for ((segment_id, _, hash), embedding) in pending.into_iter().zip(embeddings) {
        updated += connection
            .execute(
                "INSERT INTO asset_segment_embeddings (asset_id, segment_id, model, version, source_hash, vector, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(asset_id, segment_id, model, version) DO UPDATE SET
                   source_hash = excluded.source_hash,
                   vector = excluded.vector,
                   updated_at = excluded.updated_at
                 WHERE asset_segment_embeddings.source_hash != excluded.source_hash",
                params![
                    asset_id,
                    segment_id,
                    CLIP_VISION_MODEL,
                    CLIP_VERSION as i64,
                    hash,
                    embedding_blob(&embedding),
                    timestamp
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(updated)
}

pub(crate) fn load_segment_clip_embedding(
    connection: &Connection,
    asset_id: &str,
    segment_id: &str,
) -> Option<Vec<f32>> {
    let blob: Vec<u8> = connection
        .query_row(
            "SELECT vector FROM asset_segment_embeddings WHERE asset_id = ?1 AND segment_id = ?2 AND model = ?3 AND version = ?4",
            params![asset_id, segment_id, CLIP_VISION_MODEL, CLIP_VERSION as i64],
            |row| row.get(0),
        )
        .ok()?;
    let embedding = embedding_from_blob(&blob)?;
    (embedding.len() == CLIP_DIMENSIONS).then_some(embedding)
}

/// 对给定素材列表补齐片段 CLIP 图像向量；帧缺失的片段跳过。
pub(crate) fn refresh_assets_clip_embeddings(
    app: &AppHandle,
    asset_ids: &[String],
) -> Result<usize, String> {
    if asset_ids.is_empty() {
        return Ok(0);
    }
    // 探测模型是否可用；不可用时整段跳过，不打断选镜。
    let _ = vision_model(app)?;
    let connection = crate::db::open_connection(app)?;
    let mut updated = 0;
    for asset_id in asset_ids {
        let metadata_json: String = match connection.query_row(
            "SELECT metadata_json FROM assets WHERE id = ?1",
            params![asset_id],
            |row| row.get(0),
        ) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        match refresh_segment_clip_embeddings(app, asset_id, &metadata) {
            Ok(count) => updated += count,
            Err(error) => log::warn!("CLIP embed skipped for {asset_id}: {error}"),
        }
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_clip_query_prefers_keywords_and_required_visual() {
        let beat = crate::models::StoryboardBeat {
            id: "b1".to_owned(),
            purpose: "说明质保".to_owned(),
            required_visual: "framed certificates on wall".to_owned(),
            visual_keywords: vec!["warranty plaque".to_owned(), "certificate wall".to_owned()],
            narration: String::new(),
            on_screen_text: String::new(),
        };
        let query = beat_clip_query(&beat);
        assert!(query.contains("certificate wall"));
        assert!(query.contains("framed certificates"));
        assert!(!query.contains("说明质保"));
    }
}
