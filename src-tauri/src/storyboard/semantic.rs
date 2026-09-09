//! 使用安装包内置的中文文本向量模型，为 beat 与本地视觉证据提供离线语义召回。
//! 模型不联网下载；向量带模型、版本、维度和来源指纹，失效时回退词面排序。

use crate::db::now_millis;
use crate::models::TechnicalMetadata;
use fastembed::{
    InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

pub(crate) const EMBEDDING_MODEL: &str = "BAAI/bge-small-zh-v1.5";
pub(crate) const EMBEDDING_DIMENSIONS: usize = 512;
pub(crate) const EMBEDDING_VERSION: u32 = 3;
const MODEL_RESOURCE_DIRECTORY: &str = "resources/models/bge-small-zh-v1.5";
const MODEL_SHA256: &str = "69a0b846f4f116b5e6aabf9546ea6754d02264f3211a13a1bd69b31b8040749a";
const EMBEDDING_BATCH_SIZE: usize = 32;
const MAX_EMBEDDING_TEXT_CHARS: usize = 4_000;

static MODEL: OnceLock<Result<TextEmbedding, String>> = OnceLock::new();

fn bundled_model_directory(app: &AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|_| "semantic_model_resource_unavailable".to_owned())?;
    let packaged = resource_dir.join(MODEL_RESOURCE_DIRECTORY);
    if packaged.join("onnx/model.onnx").is_file() {
        return Ok(packaged);
    }
    let flattened = resource_dir.join("models/bge-small-zh-v1.5");
    if flattened.join("onnx/model.onnx").is_file() {
        return Ok(flattened);
    }
    #[cfg(debug_assertions)]
    {
        let development = Path::new(env!("CARGO_MANIFEST_DIR")).join(MODEL_RESOURCE_DIRECTORY);
        if development.join("onnx/model.onnx").is_file() {
            return Ok(development);
        }
    }
    Err("semantic_model_resource_unavailable".to_owned())
}

pub(crate) fn bundled_model_present(app: &AppHandle) -> Result<bool, String> {
    Ok(bundled_model_directory(app).is_ok())
}

fn read_model_file(directory: &Path, relative_path: &str) -> Result<Vec<u8>, String> {
    fs::read(directory.join(relative_path))
        .map_err(|_| "semantic_model_resource_unavailable".to_owned())
}

fn load_model_from_directory(directory: &Path) -> Result<TextEmbedding, String> {
    let tokenizer_files = TokenizerFiles {
        tokenizer_file: read_model_file(directory, "tokenizer.json")?,
        config_file: read_model_file(directory, "config.json")?,
        special_tokens_map_file: read_model_file(directory, "special_tokens_map.json")?,
        tokenizer_config_file: read_model_file(directory, "tokenizer_config.json")?,
    };
    let onnx_file = read_model_file(directory, "onnx/model.onnx")?;
    if hash_bytes(&onnx_file) != MODEL_SHA256 {
        return Err("semantic_model_integrity_failed".to_owned());
    }
    let model =
        UserDefinedEmbeddingModel::new(onnx_file, tokenizer_files).with_pooling(Pooling::Cls);
    TextEmbedding::try_new_from_user_defined(
        model,
        InitOptionsUserDefined::new().with_max_length(512),
    )
    .map_err(|_| "semantic_model_load_failed".to_owned())
}

fn model(app: &AppHandle) -> Result<&'static TextEmbedding, String> {
    MODEL
        .get_or_init(|| {
            bundled_model_directory(app).and_then(|path| load_model_from_directory(&path))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn encode_texts(app: &AppHandle, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let embeddings = model(app)?
        .embed(texts, Some(EMBEDDING_BATCH_SIZE))
        .map_err(|_| "semantic_model_inference_failed".to_owned())?;
    if embeddings
        .iter()
        .any(|embedding| embedding.len() != EMBEDDING_DIMENSIONS)
    {
        return Err("semantic_model_dimension_mismatch".to_owned());
    }
    Ok(embeddings)
}

/// OCR 是否足够像可读文本（过滤 Tesseract 乱码，避免污染向量与词面匹配）。
pub(crate) fn ocr_is_meaningful(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let total = trimmed.chars().count().max(1);
    let alnum = trimmed
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(ch))
        .count();
    if (alnum as f64 / total as f64) < 0.6 {
        return false;
    }
    trimmed
        .split(|ch: char| !ch.is_ascii_alphanumeric() && !('\u{4e00}'..='\u{9fff}').contains(&ch))
        .any(|token| {
            let letters = token.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
            letters >= 3
                || token
                    .chars()
                    .filter(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
                    .count()
                    >= 2
        })
}

pub(crate) fn evidence_text(metadata: &TechnicalMetadata) -> String {
    let mut parts = Vec::new();
    for evidence in &metadata.visual_evidence {
        parts.extend(evidence.subjects.iter().cloned());
        parts.extend(evidence.actions.iter().cloned());
        parts.extend(evidence.products.iter().cloned());
        if let Some(scene) = &evidence.scene {
            parts.push(scene.clone());
        }
        if let Some(shot_type) = &evidence.shot_type {
            parts.push(shot_type.clone());
        }
        if let Some(camera_motion) = &evidence.camera_motion {
            parts.push(camera_motion.clone());
        }
    }
    parts.extend(
        metadata
            .ocr_evidence
            .iter()
            .filter(|item| ocr_is_meaningful(&item.text))
            .map(|item| item.text.clone()),
    );
    parts
        .join(" ")
        .chars()
        .take(MAX_EMBEDDING_TEXT_CHARS)
        .collect()
}

pub(crate) fn segment_evidence_text(
    segment: &crate::models::SceneSegment,
    asset_ocr: &[crate::models::OcrEvidence],
) -> String {
    let mut parts = Vec::new();
    if let Some(evidence) = &segment.visual_evidence {
        parts.extend(evidence.subjects.iter().cloned());
        parts.extend(evidence.actions.iter().cloned());
        parts.extend(evidence.products.iter().cloned());
        if let Some(scene) = &evidence.scene {
            parts.push(scene.clone());
        }
        if let Some(shot_type) = &evidence.shot_type {
            parts.push(shot_type.clone());
        }
        if let Some(camera_motion) = &evidence.camera_motion {
            parts.push(camera_motion.clone());
        }
    }
    parts.extend(
        asset_ocr
            .iter()
            .filter(|item| {
                item.time_ms
                    .is_some_and(|time| time >= segment.start_ms && time <= segment.end_ms)
                    && ocr_is_meaningful(&item.text)
            })
            .map(|item| item.text.clone()),
    );
    parts
        .join(" ")
        .chars()
        .take(MAX_EMBEDDING_TEXT_CHARS)
        .collect()
}

fn embedding_blob(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub(crate) fn embedding_from_blob(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.len() % 4 != 0 {
        return None;
    }
    let mut values = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        values.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Some(values)
}

/// 将已有片段视觉证据写入 asset_segment_embeddings（CAS by source_hash）。
pub(crate) fn refresh_segment_embeddings(
    app: &AppHandle,
    asset_id: &str,
    metadata: &TechnicalMetadata,
) -> Result<usize, String> {
    let connection = crate::db::open_connection(app)?;
    let mut pending = Vec::new();
    for segment in &metadata.scene_segments {
        if segment.id.is_empty() || segment.visual_evidence.is_none() {
            continue;
        }
        let text = segment_evidence_text(segment, &metadata.ocr_evidence);
        if text.trim().is_empty() {
            continue;
        }
        let hash = source_hash(&text);
        let current_hash: Option<String> = connection
            .query_row(
                "SELECT source_hash FROM asset_segment_embeddings WHERE asset_id = ?1 AND segment_id = ?2 AND model = ?3 AND version = ?4",
                params![asset_id, segment.id, EMBEDDING_MODEL, EMBEDDING_VERSION as i64],
                |row| row.get(0),
            )
            .ok();
        if current_hash.as_deref() == Some(hash.as_str()) {
            continue;
        }
        pending.push((segment.id.clone(), text, hash));
    }
    if pending.is_empty() {
        return Ok(0);
    }
    let embeddings = encode_texts(
        app,
        pending.iter().map(|(_, text, _)| text.clone()).collect(),
    )?;
    if embeddings.len() != pending.len() {
        return Err("semantic_model_inference_failed".to_owned());
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
                    EMBEDDING_MODEL,
                    EMBEDDING_VERSION as i64,
                    hash,
                    embedding_blob(&embedding),
                    timestamp
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(updated)
}

pub(crate) fn load_segment_embedding(
    connection: &Connection,
    asset_id: &str,
    segment_id: &str,
) -> Option<Vec<f32>> {
    let blob: Vec<u8> = connection
        .query_row(
            "SELECT vector FROM asset_segment_embeddings WHERE asset_id = ?1 AND segment_id = ?2 AND model = ?3 AND version = ?4",
            params![asset_id, segment_id, EMBEDDING_MODEL, EMBEDDING_VERSION as i64],
            |row| row.get(0),
        )
        .ok()?;
    let embedding = embedding_from_blob(&blob)?;
    (embedding.len() == EMBEDDING_DIMENSIONS).then_some(embedding)
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source_hash(text: &str) -> String {
    hash_bytes(text.as_bytes())
}

pub(crate) fn embedding_is_current(metadata: &TechnicalMetadata) -> bool {
    let text = evidence_text(metadata);
    let expected_hash = source_hash(&text);
    !text.trim().is_empty()
        && metadata.embedding_model.as_deref() == Some(EMBEDDING_MODEL)
        && metadata.embedding_dimensions == Some(EMBEDDING_DIMENSIONS)
        && metadata.embedding_version == Some(EMBEDDING_VERSION)
        && metadata.embedding_source_hash.as_deref() == Some(expected_hash.as_str())
        && metadata
            .evidence_embedding
            .as_ref()
            .is_some_and(|embedding| embedding.len() == EMBEDDING_DIMENSIONS)
}

fn clear_embedding(metadata: &mut TechnicalMetadata) {
    metadata.evidence_embedding = None;
    metadata.embedding_model = None;
    metadata.embedding_dimensions = None;
    metadata.embedding_source_hash = None;
    metadata.embedding_version = None;
}

fn apply_embedding(metadata: &mut TechnicalMetadata, text: &str, embedding: Vec<f32>) {
    metadata.evidence_embedding = Some(embedding);
    metadata.embedding_model = Some(EMBEDDING_MODEL.to_owned());
    metadata.embedding_dimensions = Some(EMBEDDING_DIMENSIONS);
    metadata.embedding_source_hash = Some(source_hash(text));
    metadata.embedding_version = Some(EMBEDDING_VERSION);
}

pub(crate) fn refresh_metadata_embedding(
    app: &AppHandle,
    metadata: &mut TechnicalMetadata,
) -> Result<bool, String> {
    let text = evidence_text(metadata);
    if text.trim().is_empty() {
        let changed = metadata.evidence_embedding.is_some();
        clear_embedding(metadata);
        return Ok(changed);
    }
    if embedding_is_current(metadata) {
        return Ok(false);
    }
    let embedding = encode_texts(app, vec![text.clone()])?
        .into_iter()
        .next()
        .ok_or_else(|| "semantic_model_inference_failed".to_owned())?;
    apply_embedding(metadata, &text, embedding);
    Ok(true)
}

/// 为旧素材批量补齐向量。条件更新避免覆盖并发视觉分析刚写入的新证据。
pub(crate) fn backfill_project_embeddings(
    app: &AppHandle,
    connection: &Connection,
    project_id: &str,
) -> Result<usize, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, metadata_json FROM assets WHERE project_id = ?1 AND analysis_status = 'ready' AND kind = 'video'",
        )
        .map_err(|_| "semantic_backfill_query_failed".to_owned())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| "semantic_backfill_query_failed".to_owned())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "semantic_backfill_query_failed".to_owned())?;
    drop(statement);

    let mut pending = Vec::new();
    for (asset_id, original_json) in rows {
        let metadata: TechnicalMetadata = serde_json::from_str(&original_json).unwrap_or_default();
        let text = evidence_text(&metadata);
        if !text.trim().is_empty() && !embedding_is_current(&metadata) {
            pending.push((asset_id, original_json, metadata, text));
        }
    }
    if pending.is_empty() {
        return Ok(0);
    }

    let embeddings = encode_texts(
        app,
        pending.iter().map(|(_, _, _, text)| text.clone()).collect(),
    )?;
    if embeddings.len() != pending.len() {
        return Err("semantic_model_inference_failed".to_owned());
    }

    let transaction = connection
        .unchecked_transaction()
        .map_err(|_| "semantic_backfill_write_failed".to_owned())?;
    let mut updated = 0;
    for ((asset_id, original_json, mut metadata, text), embedding) in
        pending.into_iter().zip(embeddings)
    {
        apply_embedding(&mut metadata, &text, embedding);
        let next_json = serde_json::to_string(&metadata)
            .map_err(|_| "semantic_backfill_write_failed".to_owned())?;
        updated += transaction
            .execute(
                "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3 AND project_id = ?4 AND metadata_json = ?5",
                params![next_json, now_millis(), asset_id, project_id, original_json],
            )
            .map_err(|_| "semantic_backfill_write_failed".to_owned())?;
    }
    transaction
        .commit()
        .map_err(|_| "semantic_backfill_write_failed".to_owned())?;

    // 顺带补齐已有片段证据的片段向量（本地免费）。
    let mut segment_statement = connection
        .prepare(
            "SELECT id, metadata_json FROM assets WHERE project_id = ?1 AND analysis_status = 'ready' AND kind = 'video' AND coalesce(json_extract(metadata_json, '$.visualAnalysisVersion'), 0) >= 2",
        )
        .map_err(|_| "semantic_backfill_query_failed".to_owned())?;
    let segment_rows = segment_statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| "semantic_backfill_query_failed".to_owned())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "semantic_backfill_query_failed".to_owned())?;
    drop(segment_statement);
    for (asset_id, metadata_json) in segment_rows {
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        let _ = refresh_segment_embeddings(app, &asset_id, &metadata);
    }
    Ok(updated)
}

pub(crate) fn encode_beats(
    app: &AppHandle,
    beats: &[crate::models::StoryboardBeat],
) -> Result<Vec<Vec<f32>>, String> {
    encode_texts(
        app,
        beats
            .iter()
            .map(|beat| {
                let keywords = beat.visual_keywords.join(" ");
                format!("{keywords} {} {}", beat.required_visual, beat.purpose)
                    .trim()
                    .to_owned()
            })
            .collect(),
    )
}

pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> Option<f64> {
    if a.is_empty() || a.len() != b.len() {
        return None;
    }
    let dot = a
        .iter()
        .zip(b)
        .map(|(left, right)| *left as f64 * *right as f64)
        .sum::<f64>();
    let left_norm = a
        .iter()
        .map(|value| (*value as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    let right_norm = b
        .iter()
        .map(|value| (*value as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    (left_norm > f64::EPSILON && right_norm > f64::EPSILON)
        .then_some(dot / (left_norm * right_norm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_orders_related_vectors() {
        let query = [1.0, 0.0];
        assert!(
            cosine_similarity(&query, &[0.9, 0.1]).unwrap()
                > cosine_similarity(&query, &[0.0, 1.0]).unwrap()
        );
    }

    #[test]
    fn cosine_similarity_rejects_invalid_vectors() {
        assert_eq!(cosine_similarity(&[], &[]), None);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), None);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), None);
    }

    #[test]
    fn ocr_is_meaningful_rejects_tesseract_garbage() {
        assert!(!ocr_is_meaningful("| ~~ sf mf i ee _ aa wR"));
        assert!(!ocr_is_meaningful("J | :"));
        assert!(ocr_is_meaningful("KRL Power generator"));
        assert!(ocr_is_meaningful("电池模组"));
    }

    #[test]
    fn evidence_text_drops_garbage_ocr() {
        let metadata = TechnicalMetadata {
            visual_evidence: vec![crate::models::VisualEvidence {
                time_ms: Some(0),
                subjects: vec!["forklift".to_owned()],
                scene: Some("loading dock".to_owned()),
                actions: vec![],
                products: vec![],
                quality_notes: vec![],
                shot_type: None,

                camera_motion: None,

                segment_id: None,
            }],
            ocr_evidence: vec![
                crate::models::OcrEvidence {
                    time_ms: Some(0),
                    text: "| ~~ sf mf i ee _ aa wR".to_owned(),
                },
                crate::models::OcrEvidence {
                    time_ms: Some(1),
                    text: "KRL Power".to_owned(),
                },
            ],
            ..Default::default()
        };
        let text = evidence_text(&metadata);
        assert!(text.contains("forklift"));
        assert!(text.contains("KRL Power"));
        assert!(!text.contains("sf mf"));
    }

    #[test]
    fn bundled_model_recalls_vehicle_synonym_without_lexical_overlap() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(MODEL_RESOURCE_DIRECTORY);
        let model = load_model_from_directory(&directory).expect("load bundled semantic model");
        let embeddings = model
            .embed(
                vec!["汽车驶过城市街道", "城市道路上的车辆", "厨房里有人切菜"],
                Some(3),
            )
            .expect("encode semantic fixture");

        let vehicle_similarity = cosine_similarity(&embeddings[0], &embeddings[1]).unwrap();
        let kitchen_similarity = cosine_similarity(&embeddings[0], &embeddings[2]).unwrap();
        assert!(vehicle_similarity > kitchen_similarity + 0.1);
    }
}
