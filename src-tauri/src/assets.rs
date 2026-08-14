use crate::db::{now_millis, open_connection};
use crate::models::{
    Asset, AssetCollection, AssetEvidence, AssetHealthScanStart, AssetHealthScanSummary, AssetPage,
    AssetRelinkMatch, AssetRelinkPreview, AssetRelinkResult, AssetStatusCounts, AssetTaskCenter,
    AssetTaskFailure, AssetTaskStageCounts, BatchAssetActionResult, CollectProjectMediaPreview,
    CollectProjectMediaResult, KeyframeMetadata, OcrEvidence, SceneSegment, TechnicalMetadata,
    VisualEvidence,
};
use crate::process::{hidden_command, run_hidden_command_with_timeout, HiddenCommandError};
use crate::provider::{
    complete_visual_model_request, model_response_json_text, post_visual_model_payload,
    visual_model_retry_after, ModelAccess,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

fn modified_millis(metadata: &fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|value| value.as_millis() as i64)
}

fn source_health_observation(
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

const VISUAL_ANALYSIS_TIMEOUT: Duration = Duration::from_secs(30);
const PRIORITY_VISUAL_WAIT_TIMEOUT: Duration = Duration::from_secs(65);

// Import needs enough evidence to start editing promptly without long scans.
const SCENE_SCAN_CAP_SECONDS: f64 = 30.0;
const SCENE_SCAN_FPS: usize = 4;
const MAX_INITIAL_SCENE_KEYFRAMES: usize = 4;
const MAX_INITIAL_OCR_FRAMES: usize = 2;
const VISUAL_ANALYSIS_BATCH_SIZE: usize = 6;
const STARTUP_ANALYSIS_BATCH: usize = 4;
const DRAIN_ANALYSIS_BATCH: usize = 4;
const MAX_TECHNICAL_ANALYSIS_WORKERS: usize = 2;
const FFPROBE_TIMEOUT: Duration = Duration::from_secs(20);
const THUMBNAIL_FFMPEG_TIMEOUT: Duration = Duration::from_secs(30);
const SCENE_SCAN_FFMPEG_TIMEOUT: Duration = Duration::from_secs(45);
const FALLBACK_FRAME_FFMPEG_TIMEOUT: Duration = Duration::from_secs(20);
const TESSERACT_TIMEOUT: Duration = Duration::from_secs(20);
const ASSET_PAGE_FILTER_SQL: &str = "project_id = ?1
        AND (?2 IS NULL OR instr(lower(display_name || ' ' || source_reference || ' ' || coalesce((SELECT note FROM asset_user_metadata um WHERE um.asset_id = assets.id), '')), lower(?2)) > 0 OR EXISTS (SELECT 1 FROM asset_tag_assignments ata JOIN asset_tags t ON t.id = ata.tag_id WHERE ata.asset_id = assets.id AND instr(lower(t.name), lower(?2)) > 0))
        AND (?3 IS NULL OR kind = ?3)
        AND (?4 IS NULL OR analysis_status = ?4)
        AND (?5 IS NULL OR (
            (?5 = 'storyboard-ready' AND kind IN ('video', 'image') AND analysis_status = 'ready' AND json_extract(metadata_json, '$.visualAnalysisStatus') = 'ready' AND json_array_length(coalesce(json_extract(metadata_json, '$.visualEvidence'), json('[]'))) > 0)
            OR (?5 <> 'storyboard-ready' AND coalesce(json_extract(metadata_json, '$.visualAnalysisStatus'), 'queued') = ?5)
        ))
        AND (?6 IS NULL OR id IN (SELECT value FROM json_each(?6)))
        AND (?7 IS NULL OR (?7 = 'favorite' AND coalesce((SELECT favorite FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 1) OR (?7 = 'excluded' AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 1) OR (?7 = 'available' AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 0))
        AND (?8 IS NULL OR EXISTS (SELECT 1 FROM asset_collection_items aci WHERE aci.asset_id = assets.id AND aci.collection_id = ?8))";

static ANALYSIS_WORKER_COUNT: AtomicUsize = AtomicUsize::new(0);
static VISUAL_ANALYSIS_WORKER_ACTIVE: AtomicBool = AtomicBool::new(false);
static VISUAL_ANALYSIS_WAKE_SCHEDULED: AtomicBool = AtomicBool::new(false);

struct TechnicalAnalysisWorkerSlot;

impl Drop for TechnicalAnalysisWorkerSlot {
    fn drop(&mut self) {
        let _ = ANALYSIS_WORKER_COUNT.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            count.checked_sub(1)
        });
    }
}

fn reserve_technical_analysis_worker() -> Option<TechnicalAnalysisWorkerSlot> {
    let mut count = ANALYSIS_WORKER_COUNT.load(Ordering::Acquire);
    loop {
        if count >= MAX_TECHNICAL_ANALYSIS_WORKERS {
            return None;
        }
        match ANALYSIS_WORKER_COUNT.compare_exchange_weak(
            count,
            count + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Some(TechnicalAnalysisWorkerSlot),
            Err(current) => count = current,
        }
    }
}

#[cfg(test)]
fn release_all_technical_analysis_workers() {
    ANALYSIS_WORKER_COUNT.store(0, Ordering::Release);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualBatchResponse {
    #[serde(default)]
    assets: Vec<VisualBatchAsset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualBatchAsset {
    asset_id: String,
    #[serde(default)]
    time_ms: Option<i64>,
    #[serde(default)]
    subjects: Vec<String>,
    #[serde(default)]
    scene: Option<String>,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    products: Vec<String>,
    #[serde(default)]
    quality_notes: Vec<String>,
}

#[derive(Clone)]
struct VisualBatchRanking {
    task_id: String,
    created_at: i64,
    priority: usize,
}

#[derive(Deserialize)]
struct FfprobeOutput {
    format: Option<FfprobeFormat>,
    #[serde(default)]
    streams: Vec<FfprobeStream>,
}

#[derive(Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    r_frame_rate: Option<String>,
}

fn asset_kind(path: &Path) -> String {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => "video",
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" => "image",
        "mp3" | "wav" | "aac" | "m4a" | "flac" | "ogg" => "audio",
        _ => "other",
    }
    .to_owned()
}

fn supported_media_file(path: &Path) -> bool {
    asset_kind(path) != "other"
}

fn collect_media_files(directory: &Path, sources: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        fs::read_dir(directory).map_err(|_| "The selected folder could not be read.".to_owned())?
    {
        let entry = entry.map_err(|_| "The selected folder could not be read.".to_owned())?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|_| "The selected folder could not be read.".to_owned())?;
        if file_type.is_dir() && !file_type.is_symlink() {
            collect_media_files(&path, sources)?;
        } else if file_type.is_file() && supported_media_file(&path) {
            sources.push(path);
        }
    }
    Ok(())
}

fn parse_frame_rate(value: Option<&str>) -> Option<f64> {
    let value = value?;
    let mut parts = value.split('/');
    let numerator = parts.next()?.parse::<f64>().ok()?;
    let denominator = parts
        .next()
        .and_then(|part| part.parse::<f64>().ok())
        .unwrap_or(1.0);
    (denominator > 0.0).then_some(numerator / denominator)
}

fn probe_media(source: &Path) -> Result<TechnicalMetadata, String> {
    let mut command = hidden_command("ffprobe");
    command
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,width,height,r_frame_rate",
            "-of",
            "json",
        ])
        .arg(source);
    let output =
        run_hidden_command_with_timeout(&mut command, FFPROBE_TIMEOUT).map_err(|error| {
            match error {
                HiddenCommandError::TimedOut => "FFprobe timed out while reading this media file.",
                HiddenCommandError::Failed => "FFprobe is not available on this computer.",
            }
            .to_owned()
        })?;
    if !output.status.success() {
        return Err("FFprobe could not read this media file.".to_owned());
    }
    let probe: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|_| "FFprobe returned invalid media metadata.".to_owned())?;
    let video_stream = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"));
    Ok(TechnicalMetadata {
        duration_ms: probe
            .format
            .and_then(|format| format.duration)
            .and_then(|duration| duration.parse::<f64>().ok())
            .map(|duration| (duration * 1000.0).round() as i64),
        width: video_stream.and_then(|stream| stream.width),
        height: video_stream.and_then(|stream| stream.height),
        fps: parse_frame_rate(video_stream.and_then(|stream| stream.r_frame_rate.as_deref())),
        has_audio: probe
            .streams
            .iter()
            .any(|stream| stream.codec_type.as_deref() == Some("audio")),
        thumbnail_path: None,
        keyframes: Vec::new(),
        scene_segments: Vec::new(),
        ocr_evidence: Vec::new(),
        visual_evidence: Vec::new(),
        visual_analysis_note: None,
        visual_analysis_status: "queued".to_owned(),
    })
}

fn derived_directory(app: &AppHandle, asset_id: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("derived")
        .join(asset_id);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

fn thumbnail_destination(app: &AppHandle, asset_id: &str) -> Result<PathBuf, String> {
    Ok(derived_directory(app, asset_id)?.join("thumbnail.jpg"))
}

fn generate_thumbnail(
    app: &AppHandle,
    asset_id: &str,
    source: &Path,
    kind: &str,
) -> Result<Option<String>, String> {
    if !matches!(kind, "video" | "image") {
        return Ok(None);
    }
    let destination = thumbnail_destination(app, asset_id)?;
    let mut command = hidden_command("ffmpeg");
    command.args(["-y", "-hide_banner", "-loglevel", "error"]);
    if kind == "video" {
        command.args(["-ss", "0.5"]);
    }
    command
        .arg("-i")
        .arg(source)
        .args(["-frames:v", "1", "-vf", "scale=320:-2"])
        .arg(&destination);
    let output = run_hidden_command_with_timeout(&mut command, THUMBNAIL_FFMPEG_TIMEOUT).map_err(
        |error| match error {
            HiddenCommandError::TimedOut => "Thumbnail generation timed out.".to_owned(),
            HiddenCommandError::Failed => "Thumbnail generation could not start.".to_owned(),
        },
    )?;
    Ok((output.status.success() && destination.is_file())
        .then(|| destination.to_string_lossy().into_owned()))
}

fn extract_scene_times(output: &[u8]) -> Vec<f64> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| line.split("pts_time:").nth(1))
        .filter_map(|value| value.split_whitespace().next())
        .filter_map(|value| value.parse::<f64>().ok())
        .collect()
}

fn scene_scan_filter() -> String {
    format!(
        "fps={SCENE_SCAN_FPS},scale=320:-2:flags=fast_bilinear,select=gt(scene\\,0.30),showinfo"
    )
}

fn generate_video_keyframes(
    app: &AppHandle,
    asset_id: &str,
    source: &Path,
    duration_ms: Option<i64>,
) -> Result<(Vec<KeyframeMetadata>, Vec<SceneSegment>), String> {
    let directory = derived_directory(app, asset_id)?;
    let pattern = directory.join("keyframe_%03d.jpg");
    let cap_seconds = SCENE_SCAN_CAP_SECONDS.to_string();
    let max_keyframes = MAX_INITIAL_SCENE_KEYFRAMES.to_string();
    let scene_filter = scene_scan_filter();
    let mut command = hidden_command("ffmpeg");
    command
        .args(["-y", "-hide_banner", "-loglevel", "info", "-i"])
        .arg(source)
        .args([
            "-t",
            &cap_seconds,
            "-vf",
            &scene_filter,
            "-frames:v",
            &max_keyframes,
            "-fps_mode",
            "vfr",
        ])
        .arg(&pattern);
    let output = run_hidden_command_with_timeout(&mut command, SCENE_SCAN_FFMPEG_TIMEOUT);
    let output = output.map_err(|error| match error {
        HiddenCommandError::TimedOut => "Scene scan timed out.".to_owned(),
        HiddenCommandError::Failed => "Scene scan could not start.".to_owned(),
    })?;
    let mut times = output
        .status
        .success()
        .then(|| extract_scene_times(&output.stderr))
        .unwrap_or_default();
    if times.is_empty() {
        let duration_seconds = duration_ms.unwrap_or(0) as f64 / 1000.0;
        times = [
            0.0,
            duration_seconds * 0.5,
            (duration_seconds - 0.1).max(0.0),
        ]
        .into_iter()
        .filter(|time| duration_seconds > 0.0 || *time == 0.0)
        .collect();
        for (index, time) in times.iter().enumerate() {
            let destination = directory.join(format!("keyframe_{:03}.jpg", index + 1));
            let mut command = hidden_command("ffmpeg");
            command
                .args([
                    "-y",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-ss",
                    &format!("{time:.3}"),
                    "-i",
                ])
                .arg(source)
                .args(["-frames:v", "1", "-vf", "scale=320:-2"])
                .arg(destination);
            run_hidden_command_with_timeout(&mut command, FALLBACK_FRAME_FFMPEG_TIMEOUT).map_err(
                |error| match error {
                    HiddenCommandError::TimedOut => {
                        "Fallback frame extraction timed out.".to_owned()
                    }
                    HiddenCommandError::Failed => {
                        "Fallback frame extraction could not start.".to_owned()
                    }
                },
            )?;
        }
    }
    let keyframes = times
        .into_iter()
        .enumerate()
        .filter_map(|(index, time)| {
            let image_path = directory.join(format!("keyframe_{:03}.jpg", index + 1));
            image_path.is_file().then(|| KeyframeMetadata {
                time_ms: (time * 1000.0).round() as i64,
                image_path: image_path.to_string_lossy().into_owned(),
            })
        })
        .collect::<Vec<_>>();
    let mut boundaries = keyframes
        .iter()
        .map(|frame| frame.time_ms)
        .collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();
    if boundaries.first().copied() != Some(0) {
        boundaries.insert(0, 0);
    }
    if let Some(duration_ms) = duration_ms.filter(|duration| *duration > 0) {
        if boundaries.last().copied() != Some(duration_ms) {
            boundaries.push(duration_ms);
        }
    }
    let scenes = boundaries
        .windows(2)
        .filter(|pair| pair[1] > pair[0])
        .map(|pair| SceneSegment {
            start_ms: pair[0],
            end_ms: pair[1],
        })
        .collect();
    Ok((keyframes, scenes))
}

fn tesseract_program() -> PathBuf {
    if let Some(configured) = env::var_os("TESSERACT_PATH") {
        return PathBuf::from(configured);
    }
    if let Some(program_files) = env::var_os("ProgramFiles") {
        let installed = PathBuf::from(program_files)
            .join("Tesseract-OCR")
            .join("tesseract.exe");
        if installed.is_file() {
            return installed;
        }
    }
    PathBuf::from("tesseract")
}

fn extract_ocr(image_path: &Path, time_ms: Option<i64>) -> Result<Option<OcrEvidence>, String> {
    let mut command = hidden_command(tesseract_program());
    command
        .arg(image_path)
        .arg("stdout")
        .args(["-l", "eng", "--psm", "6"]);
    let output =
        run_hidden_command_with_timeout(&mut command, TESSERACT_TIMEOUT).map_err(|error| {
            match error {
                HiddenCommandError::TimedOut => "OCR timed out.".to_owned(),
                HiddenCommandError::Failed => "OCR could not start.".to_owned(),
            }
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok((!text.is_empty()).then_some(OcrEvidence { time_ms, text }))
}

fn extract_ocr_evidence(
    kind: &str,
    source: &Path,
    keyframes: &[KeyframeMetadata],
) -> Result<Vec<OcrEvidence>, String> {
    if kind == "image" {
        return Ok(extract_ocr(source, None)?.into_iter().collect());
    }
    if kind == "video" {
        return Ok(keyframes
            .iter()
            .take(MAX_INITIAL_OCR_FRAMES)
            .map(|frame| extract_ocr(Path::new(&frame.image_path), Some(frame.time_ms)))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect());
    }
    Ok(Vec::new())
}

fn representative_frame(metadata: &TechnicalMetadata, kind: &str) -> Option<(String, Option<i64>)> {
    if kind == "video" {
        metadata
            .keyframes
            // The middle candidate is less likely than the opening frame to be
            // an intro card, slate, or transition.
            .get(metadata.keyframes.len() / 2)
            .map(|frame| (frame.image_path.clone(), Some(frame.time_ms)))
    } else if kind == "image" {
        metadata.thumbnail_path.clone().map(|path| (path, None))
    } else {
        None
    }
}

fn update_analysis_status(
    app: &AppHandle,
    asset_id: &str,
    task_id: &str,
    source_reference: &str,
    status: &str,
    metadata: Option<&TechnicalMetadata>,
    error_message: Option<&str>,
) -> Result<bool, String> {
    let timestamp = now_millis();
    let task_status = match status {
        "analyzing" => "running",
        "ready" => "completed",
        "failed" => "failed",
        _ => status,
    };
    let metadata_json = metadata
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| error.to_string())?;
    let connection = open_connection(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let updated = if let Some(metadata_json) = metadata_json {
        transaction.execute(
            "UPDATE assets SET analysis_status = ?1, metadata_json = ?2, updated_at = ?3 WHERE id = ?4 AND source_reference = ?5 AND EXISTS (SELECT 1 FROM agent_tasks WHERE id = ?6 AND tool_name = 'analyze_asset' AND status = 'running' AND json_extract(input_json, '$.assetId') = ?4)",
            params![status, metadata_json, timestamp, asset_id, source_reference, task_id],
        ).map_err(|error| error.to_string())?
    } else {
        transaction
            .execute(
                "UPDATE assets SET analysis_status = ?1, updated_at = ?2 WHERE id = ?3 AND source_reference = ?4 AND EXISTS (SELECT 1 FROM agent_tasks WHERE id = ?5 AND tool_name = 'analyze_asset' AND status = 'running' AND json_extract(input_json, '$.assetId') = ?3)",
                params![status, timestamp, asset_id, source_reference, task_id],
            )
            .map_err(|error| error.to_string())?
    };
    if updated == 0 {
        transaction.commit().map_err(|error| error.to_string())?;
        return Ok(false);
    }
    transaction.execute(
        "UPDATE agent_tasks SET status = ?1, result_json = ?2, error_message = ?3, updated_at = ?4 WHERE id = ?5 AND tool_name = 'analyze_asset' AND status = 'running' AND json_extract(input_json, '$.assetId') = ?6 AND EXISTS (SELECT 1 FROM assets WHERE id = ?6 AND source_reference = ?7)",
        params![task_status, metadata.map(serde_json::to_string).transpose().map_err(|error| error.to_string())?, error_message, timestamp, task_id, asset_id, source_reference],
    ).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(true)
}

fn update_visual_batch_task(
    app: &AppHandle,
    task_id: &str,
    status: &str,
    requested_count: usize,
    ready_count: usize,
    skipped_count: usize,
    failed_count: usize,
    error_code: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(app)?;
    let created_at = connection
        .query_row(
            "SELECT created_at FROM agent_tasks WHERE id = ?1",
            params![task_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or_else(|_| now_millis());
    let timestamp = now_millis();
    connection
        .execute(
            "UPDATE agent_tasks SET status = ?1, result_json = ?2, error_message = ?3, updated_at = ?4 WHERE id = ?5",
            params![
                status,
                serde_json::json!({
                    "requestedCount": requested_count,
                    "readyCount": ready_count,
                    "skippedCount": skipped_count,
                    "failedCount": failed_count,
                    "durationMs": timestamp.saturating_sub(created_at),
                })
                .to_string(),
                error_code,
                timestamp,
                task_id,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn update_visual_metadata(
    app: &AppHandle,
    asset_ids: &[String],
    status: &str,
    evidence: &HashMap<String, VisualEvidence>,
    note: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    for asset_id in asset_ids {
        let metadata_json: String = transaction
            .query_row(
                "SELECT metadata_json FROM assets WHERE id = ?1",
                params![asset_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| "{}".to_owned());
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        if preserves_explicit_visual_skip(&metadata, status) {
            continue;
        }
        metadata.visual_analysis_status = status.to_owned();
        metadata.visual_analysis_note = note.map(str::to_owned);
        if let Some(item) = evidence.get(asset_id) {
            metadata.visual_evidence = vec![item.clone()];
        }
        transaction
            .execute(
                "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3 AND analysis_status = 'ready'",
                params![
                    serde_json::to_string(&metadata).map_err(|error| error.to_string())?,
                    now_millis(),
                    asset_id
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

fn preserves_explicit_visual_skip(metadata: &TechnicalMetadata, next_status: &str) -> bool {
    metadata.visual_analysis_status == "skipped"
        && metadata.visual_analysis_note.as_deref() == Some("visual_analysis_skipped_by_user")
        && next_status != "skipped"
}

fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{2fa1f}'
    )
}

fn lexical_tokens(text: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut ascii = String::new();
    let mut cjk = String::new();
    let flush_ascii = |value: &mut String, tokens: &mut HashSet<String>| {
        if !value.is_empty() {
            tokens.insert(std::mem::take(value));
        }
    };
    let flush_cjk = |value: &mut String, tokens: &mut HashSet<String>| {
        if value.is_empty() {
            return;
        }
        let characters = value.chars().collect::<Vec<_>>();
        tokens.insert(std::mem::take(value));
        if characters.len() > 1 {
            tokens.extend(
                characters
                    .windows(2)
                    .map(|pair| pair.iter().collect::<String>()),
            );
        }
    };
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk, &mut tokens);
            ascii.push(character.to_ascii_lowercase());
        } else if is_cjk(character) {
            flush_ascii(&mut ascii, &mut tokens);
            cjk.push(character);
        } else {
            flush_ascii(&mut ascii, &mut tokens);
            flush_cjk(&mut cjk, &mut tokens);
        }
    }
    flush_ascii(&mut ascii, &mut tokens);
    flush_cjk(&mut cjk, &mut tokens);
    tokens
}

fn visual_asset_ranking_text(
    display_name: &str,
    source_reference: &str,
    folder_reference: Option<&str>,
    metadata: &TechnicalMetadata,
) -> String {
    let mut hints = vec![display_name.to_owned()];
    if let Some(folder_reference) = folder_reference {
        let folder = Path::new(folder_reference);
        if let Some(folder_name) = folder.file_name().and_then(|name| name.to_str()) {
            hints.push(folder_name.to_owned());
        }
        if let Ok(relative) = Path::new(source_reference).strip_prefix(folder) {
            if let Some(parent) = relative.parent() {
                hints.extend(
                    parent
                        .components()
                        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned)),
                );
            }
        }
    }
    hints.extend(metadata.ocr_evidence.iter().map(|item| item.text.clone()));
    hints.join(" ")
}

fn lexical_overlap_score(brief_tokens: &HashSet<String>, hints: &str) -> usize {
    lexical_tokens(hints).intersection(brief_tokens).count()
}

fn rank_visual_batches(mut batches: Vec<VisualBatchRanking>) -> Vec<VisualBatchRanking> {
    batches.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    batches
}

pub(crate) fn prioritize_pending_visual_batches(
    app: &AppHandle,
    project_id: &str,
    brief: &str,
) -> Result<Option<String>, String> {
    let connection = open_connection(app)?;
    let brief_tokens = lexical_tokens(brief);
    let mut asset_scores = HashMap::new();
    let mut assets = connection
        .prepare(
            "SELECT id, display_name, source_reference, folder_reference, metadata_json FROM assets WHERE project_id = ?1",
        )
        .map_err(|error| error.to_string())?;
    let rows = assets
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(assets);
    for (asset_id, display_name, source_reference, folder_reference, metadata_json) in rows {
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        let hints = visual_asset_ranking_text(
            &display_name,
            &source_reference,
            folder_reference.as_deref(),
            &metadata,
        );
        asset_scores.insert(asset_id, lexical_overlap_score(&brief_tokens, &hints));
    }

    let mut tasks = connection
        .prepare(
            "SELECT id, input_json, created_at, status FROM agent_tasks WHERE project_id = ?1 AND tool_name = 'analyze_asset_visual_batch' AND status IN ('queued', 'running') ORDER BY created_at ASC, id ASC",
        )
        .map_err(|error| error.to_string())?;
    let queued = tasks
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(tasks);
    let rankings_with_status = queued
        .into_iter()
        .map(|(task_id, input_json, created_at, status)| {
            let scores = serde_json::from_str::<Value>(&input_json)
                .ok()
                .and_then(|value| value.get("assetIds").and_then(Value::as_array).cloned())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|id| id.as_str().and_then(|id| asset_scores.get(id)).copied())
                .collect::<Vec<_>>();
            let maximum = scores.iter().copied().max().unwrap_or(0);
            (
                VisualBatchRanking {
                    task_id,
                    created_at,
                    priority: maximum.saturating_mul(1_000) + scores.iter().sum::<usize>(),
                },
                status,
            )
        })
        .collect::<Vec<_>>();
    let status_by_task = rankings_with_status
        .iter()
        .map(|(ranking, status)| (ranking.task_id.clone(), status.clone()))
        .collect::<HashMap<_, _>>();
    let rankings = rank_visual_batches(
        rankings_with_status
            .iter()
            .map(|(ranking, _)| ranking.clone())
            .collect(),
    );
    let highest_relevant_task = rankings
        .first()
        .filter(|batch| batch.priority > 0)
        .map(|batch| batch.task_id.clone());
    for batch in rankings {
        let is_queued = status_by_task
            .get(&batch.task_id)
            .is_some_and(|status| status == "queued");
        if !is_queued {
            continue;
        }
        connection
            .execute(
                "UPDATE agent_tasks SET result_json = json_set(COALESCE(result_json, '{}'), '$.priority', ?1) WHERE id = ?2 AND status = 'queued'",
                params![batch.priority as i64, batch.task_id],
            )
            .map_err(|error| error.to_string())?;
    }
    spawn_visual_analysis_worker(app.clone());
    Ok(highest_relevant_task)
}

pub(crate) fn wait_for_visual_batch(app: &AppHandle, task_id: Option<&str>) -> Result<(), String> {
    let Some(task_id) = task_id else {
        return Ok(());
    };
    let deadline = Instant::now() + PRIORITY_VISUAL_WAIT_TIMEOUT;
    loop {
        let status = open_connection(app)?
            .query_row(
                "SELECT status FROM agent_tasks WHERE id = ?1 AND tool_name = 'analyze_asset_visual_batch'",
                params![task_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if !matches!(status.as_deref(), Some("queued" | "running")) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn visual_model_content(frames: &[(String, Option<i64>, Vec<u8>)]) -> Vec<Value> {
    let mut content = vec![
        serde_json::json!({ "type": "input_text", "text": "Analyze only visible evidence. Return JSON {assets:[{assetId,timeMs,subjects,scene,actions,products,qualityNotes}]}. Each assetId must be one supplied label. Do not infer facts not visible." }),
    ];
    for (asset_id, time_ms, image) in frames {
        content.push(serde_json::json!({ "type": "input_text", "text": format!("assetId={asset_id}; sourceTimeMs={}", time_ms.map_or("image".to_owned(), |value| value.to_string())) }));
        content.push(serde_json::json!({ "type": "input_image", "image_url": format!("data:image/jpeg;base64,{}", STANDARD.encode(image)) }));
    }
    content
}

fn queue_visual_analysis_batch(app: &AppHandle, asset_ids: &[String]) -> Result<(), String> {
    if asset_ids.is_empty() {
        return Ok(());
    }
    let connection = open_connection(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut visual_asset_ids = Vec::new();
    let mut skipped_asset_ids = Vec::new();
    let mut project_id = None;
    for asset_id in asset_ids {
        let row = transaction
            .query_row(
                "SELECT project_id, kind, metadata_json FROM assets WHERE id = ?1 AND analysis_status = 'ready'",
                params![asset_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((row_project_id, kind, metadata_json)) = row else {
            continue;
        };
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        project_id.get_or_insert(row_project_id);
        if representative_frame(&metadata, &kind).is_some() {
            metadata.visual_analysis_status = "queued".to_owned();
            metadata.visual_analysis_note = None;
            visual_asset_ids.push(asset_id.clone());
        } else {
            metadata.visual_analysis_status = "skipped".to_owned();
            metadata.visual_analysis_note = Some("visual_analysis_not_applicable".to_owned());
            skipped_asset_ids.push(asset_id.clone());
        }
        transaction
            .execute(
                "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    serde_json::to_string(&metadata).map_err(|error| error.to_string())?,
                    now_millis(),
                    asset_id
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    if let Some(project_id) = project_id.filter(|_| !visual_asset_ids.is_empty()) {
        transaction.execute(
            "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, result_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset_visual_batch', 'queued', ?3, ?4, ?5, ?5)",
            params![
                Uuid::new_v4().to_string(),
                project_id,
                serde_json::json!({ "assetIds": visual_asset_ids }).to_string(),
                serde_json::json!({ "requestedCount": visual_asset_ids.len(), "readyCount": 0, "skippedCount": skipped_asset_ids.len(), "failedCount": 0 }).to_string(),
                now_millis(),
            ],
        ).map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    spawn_visual_analysis_worker(app.clone());
    Ok(())
}

fn run_visual_analysis_batch(app: AppHandle, task_id: String, asset_ids: Vec<String>) {
    let requested_count = asset_ids.len();
    let _ = update_visual_batch_task(&app, &task_id, "running", requested_count, 0, 0, 0, None);
    let _ = update_visual_metadata(&app, &asset_ids, "running", &HashMap::new(), None);

    let assets = (|| -> Result<Vec<(String, String, TechnicalMetadata)>, &'static str> {
        let connection = open_connection(&app).map_err(|_| "visual_storage_failed")?;
        asset_ids
            .iter()
            .map(|asset_id| {
                connection.query_row(
                    "SELECT id, kind, metadata_json FROM assets WHERE id = ?1 AND analysis_status = 'ready'",
                    params![asset_id],
                    |row| Ok((row.get(0)?, row.get(1)?, serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_default())),
                ).map_err(|_| "visual_asset_unavailable")
            })
            .collect()
    })();
    let Ok(assets) = assets else {
        let _ = update_visual_metadata(
            &app,
            &asset_ids,
            "failed",
            &HashMap::new(),
            Some("visual_asset_unavailable"),
        );
        let _ = update_visual_batch_task(
            &app,
            &task_id,
            "failed",
            requested_count,
            0,
            0,
            requested_count,
            Some("visual_asset_unavailable"),
        );
        return;
    };
    let mut source_times = HashMap::new();
    let mut frames = Vec::new();
    for (asset_id, kind, metadata) in &assets {
        let Some((image_path, time_ms)) = representative_frame(metadata, kind) else {
            continue;
        };
        source_times.insert(asset_id.clone(), time_ms);
        let Ok(image) = fs::read(image_path) else {
            let _ = update_visual_metadata(
                &app,
                &asset_ids,
                "failed",
                &HashMap::new(),
                Some("visual_frame_unavailable"),
            );
            let _ = update_visual_batch_task(
                &app,
                &task_id,
                "failed",
                requested_count,
                0,
                0,
                requested_count,
                Some("visual_frame_unavailable"),
            );
            return;
        };
        frames.push((asset_id.clone(), time_ms, image));
    }
    let content = visual_model_content(&frames);
    let access = match ModelAccess::resolve() {
        Ok(access) => access,
        Err(_) => {
            let _ = update_visual_metadata(
                &app,
                &asset_ids,
                "skipped",
                &HashMap::new(),
                Some("visual_provider_unavailable"),
            );
            let _ = update_visual_batch_task(
                &app,
                &task_id,
                "completed",
                requested_count,
                0,
                requested_count,
                0,
                Some("visual_provider_unavailable"),
            );
            return;
        }
    };
    let request = serde_json::json!({ "model": "gpt-5.4", "store": false, "stream": true, "input": [{ "role": "user", "content": content }], "text": { "format": { "type": "json_object" } } });
    let response_body =
        match post_visual_model_payload(&access, &request, Some(VISUAL_ANALYSIS_TIMEOUT)) {
            Ok(body) => body,
            Err(error) if error == "visual_provider_circuit_open" => {
                let _ = update_visual_metadata(
                    &app,
                    &asset_ids,
                    "queued",
                    &HashMap::new(),
                    Some("visual_provider_cooldown"),
                );
                let _ = update_visual_batch_task(
                    &app,
                    &task_id,
                    "queued",
                    requested_count,
                    0,
                    0,
                    0,
                    Some("visual_provider_cooldown"),
                );
                return;
            }
            Err(_) => String::new(),
        };
    let response = (!response_body.is_empty())
        .then_some(response_body)
        .and_then(|body| model_response_json_text(&access, &body))
        .and_then(|text| serde_json::from_str::<VisualBatchResponse>(&text).ok());
    let Some(response) = response else {
        complete_visual_model_request(false);
        let _ = update_visual_metadata(
            &app,
            &asset_ids,
            "failed",
            &HashMap::new(),
            Some("visual_request_failed"),
        );
        let _ = update_visual_batch_task(
            &app,
            &task_id,
            "failed",
            requested_count,
            0,
            0,
            requested_count,
            Some("visual_request_failed"),
        );
        return;
    };
    let allowed: HashSet<&str> = asset_ids.iter().map(String::as_str).collect();
    if response.assets.iter().any(|item| {
        !allowed.contains(item.asset_id.as_str())
            || source_times.get(&item.asset_id).copied().flatten() != item.time_ms
    }) {
        complete_visual_model_request(false);
        let _ = update_visual_metadata(
            &app,
            &asset_ids,
            "failed",
            &HashMap::new(),
            Some("visual_response_invalid"),
        );
        let _ = update_visual_batch_task(
            &app,
            &task_id,
            "failed",
            requested_count,
            0,
            0,
            requested_count,
            Some("visual_response_invalid"),
        );
        return;
    }
    let evidence = response
        .assets
        .into_iter()
        .fold(HashMap::new(), |mut evidence, item| {
            let time_ms = source_times.get(&item.asset_id).copied().flatten();
            evidence.entry(item.asset_id).or_insert(VisualEvidence {
                time_ms,
                subjects: item.subjects,
                scene: item.scene,
                actions: item.actions,
                products: item.products,
                quality_notes: item.quality_notes,
            });
            evidence
        });
    let ready_ids = asset_ids
        .iter()
        .filter(|asset_id| evidence.contains_key(*asset_id))
        .cloned()
        .collect::<Vec<_>>();
    let failed_ids = asset_ids
        .iter()
        .filter(|asset_id| !evidence.contains_key(*asset_id))
        .cloned()
        .collect::<Vec<_>>();
    complete_visual_model_request(failed_ids.is_empty());
    let _ = update_visual_metadata(&app, &ready_ids, "ready", &evidence, None);
    let _ = update_visual_metadata(
        &app,
        &failed_ids,
        "failed",
        &HashMap::new(),
        Some("visual_response_incomplete"),
    );
    let _ = update_visual_batch_task(
        &app,
        &task_id,
        if failed_ids.is_empty() {
            "completed"
        } else {
            "failed"
        },
        requested_count,
        ready_ids.len(),
        0,
        failed_ids.len(),
        (!failed_ids.is_empty()).then_some("visual_response_incomplete"),
    );
}

fn spawn_visual_analysis_worker(app: AppHandle) {
    if VISUAL_ANALYSIS_WORKER_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        loop {
            if visual_model_retry_after().is_some() {
                break;
            }
            let task = (|| -> Result<Option<(String, Vec<String>)>, String> {
                let connection = open_connection(&app)?;
                let transaction = connection
                    .unchecked_transaction()
                    .map_err(|error| error.to_string())?;
                let row = transaction.query_row("SELECT id, input_json FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status = 'queued' ORDER BY COALESCE(json_extract(result_json, '$.priority'), 0) DESC, created_at ASC, id ASC LIMIT 1", [], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).optional().map_err(|error| error.to_string())?;
                let Some((task_id, input_json)) = row else {
                    return Ok(None);
                };
                let asset_ids = serde_json::from_str::<serde_json::Value>(&input_json)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("assetIds")
                            .and_then(serde_json::Value::as_array)
                            .map(|ids| {
                                ids.iter()
                                    .filter_map(|id| id.as_str().map(str::to_owned))
                                    .collect::<Vec<_>>()
                            })
                    })
                    .filter(|ids| !ids.is_empty() && ids.len() <= VISUAL_ANALYSIS_BATCH_SIZE);
                let Some(asset_ids) = asset_ids else {
                    update_visual_batch_task(
                        &app,
                        &task_id,
                        "failed",
                        0,
                        0,
                        0,
                        0,
                        Some("visual_task_input_invalid"),
                    )?;
                    return Ok(Some((task_id, Vec::new())));
                };
                let claimed = transaction.execute(
                    "UPDATE agent_tasks SET status = 'running', updated_at = ?1 WHERE id = ?2 AND status = 'queued'",
                    params![now_millis(), task_id],
                ).map_err(|error| error.to_string())?;
                transaction.commit().map_err(|error| error.to_string())?;
                if claimed == 1 {
                    Ok(Some((task_id, asset_ids)))
                } else {
                    Ok(Some((task_id, Vec::new())))
                }
            })();
            match task {
                Ok(Some((task_id, asset_ids))) => {
                    if !asset_ids.is_empty() {
                        run_visual_analysis_batch(app.clone(), task_id, asset_ids);
                    }
                }
                Ok(None) | Err(_) => break,
            }
        }
        VISUAL_ANALYSIS_WORKER_ACTIVE.store(false, Ordering::Release);
        if let Some(retry_after) = visual_model_retry_after() {
            schedule_visual_analysis_wake(app.clone(), retry_after);
            return;
        }
        let has_pending = open_connection(&app)
            .ok()
            .and_then(|connection| {
                connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status = 'queued')",
                        [],
                        |row| row.get::<_, bool>(0),
                    )
                    .ok()
            })
            .unwrap_or(false);
        if has_pending {
            spawn_visual_analysis_worker(app.clone());
        }
    });
}

fn schedule_visual_analysis_wake(app: AppHandle, retry_after: Duration) {
    if VISUAL_ANALYSIS_WAKE_SCHEDULED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        thread::sleep(retry_after + Duration::from_millis(100));
        VISUAL_ANALYSIS_WAKE_SCHEDULED.store(false, Ordering::Release);
        spawn_visual_analysis_worker(app);
    });
}

fn recover_interrupted_visual_batches(app: &AppHandle) -> Result<(), String> {
    let connection = open_connection(app)?;
    let rows = connection
        .prepare(
            "SELECT id, input_json FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status = 'running'",
        )
        .map_err(|error| error.to_string())?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut asset_ids = Vec::new();
    let mut invalid_asset_ids = Vec::new();
    for (task_id, input_json) in rows {
        let parsed = serde_json::from_str::<serde_json::Value>(&input_json)
            .ok()
            .and_then(|value| {
                value
                    .get("assetIds")
                    .and_then(serde_json::Value::as_array)
                    .map(|ids| {
                        ids.iter()
                            .filter_map(|id| id.as_str().map(str::to_owned))
                            .collect::<Vec<_>>()
                    })
            })
            .filter(|ids| !ids.is_empty());
        if let Some(ids) = parsed {
            if ids.len() <= VISUAL_ANALYSIS_BATCH_SIZE {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'queued', error_message = NULL, updated_at = ?1 WHERE id = ?2",
                        params![now_millis(), task_id],
                    )
                    .map_err(|error| error.to_string())?;
                asset_ids.extend(ids);
            } else {
                connection.execute(
                    "UPDATE agent_tasks SET status = 'failed', error_message = 'visual_task_input_invalid', updated_at = ?1 WHERE id = ?2",
                    params![now_millis(), task_id],
                ).map_err(|error| error.to_string())?;
                invalid_asset_ids.extend(ids);
            }
        } else {
            update_visual_batch_task(
                app,
                &task_id,
                "failed",
                0,
                0,
                0,
                0,
                Some("visual_task_input_invalid"),
            )?;
        }
    }
    drop(connection);
    if !invalid_asset_ids.is_empty() {
        update_visual_metadata(
            app,
            &invalid_asset_ids,
            "failed",
            &HashMap::new(),
            Some("visual_task_input_invalid"),
        )?;
    }
    asset_ids.sort_unstable();
    asset_ids.dedup();
    if !asset_ids.is_empty() {
        update_visual_metadata(app, &asset_ids, "queued", &HashMap::new(), None)?;
    }
    Ok(())
}

fn backfill_queued_visual_batches(app: &AppHandle) -> Result<(), String> {
    let connection = open_connection(app)?;
    let mut active_ids = HashSet::new();
    let mut tasks = connection
        .prepare("SELECT input_json FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status IN ('queued', 'running')")
        .map_err(|error| error.to_string())?;
    let active_rows = tasks
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(tasks);
    for input_json in active_rows {
        if let Some(ids) = serde_json::from_str::<serde_json::Value>(&input_json)
            .ok()
            .and_then(|value| {
                value
                    .get("assetIds")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
            })
        {
            active_ids.extend(ids.iter().filter_map(|id| id.as_str().map(str::to_owned)));
        }
    }
    let mut statement = connection
        .prepare("SELECT id, project_id, kind, metadata_json FROM assets WHERE analysis_status = 'ready'")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
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
    drop(statement);
    drop(connection);
    let mut by_project = HashMap::<String, Vec<String>>::new();
    for (asset_id, project_id, kind, metadata_json) in rows {
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.visual_analysis_status == "queued"
            && !active_ids.contains(&asset_id)
            && representative_frame(&metadata, &kind).is_some()
        {
            by_project.entry(project_id).or_default().push(asset_id);
        }
    }
    for asset_ids in by_project.into_values() {
        for batch in asset_ids.chunks(VISUAL_ANALYSIS_BATCH_SIZE) {
            queue_visual_analysis_batch(app, batch)?;
        }
    }
    Ok(())
}

fn run_technical_analysis(app: AppHandle, asset_id: String, task_id: String) {
    log::info!("Starting local media analysis for asset {asset_id}.");
    let source = (|| -> Result<(String, String), String> {
        let connection = open_connection(&app)?;
        let (source_reference, kind): (String, String) = connection
            .query_row(
                "SELECT source_reference, kind FROM assets WHERE id = ?1 AND EXISTS (SELECT 1 FROM agent_tasks WHERE id = ?2 AND tool_name = 'analyze_asset' AND status = 'running' AND json_extract(input_json, '$.assetId') = ?1)",
                params![asset_id, task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "The media asset is no longer available.".to_owned())?;
        Ok((source_reference, kind))
    })();
    let terminal_source_reference = source
        .as_ref()
        .ok()
        .map(|(source_reference, _)| source_reference.clone());
    let result = source.and_then(|(source_reference, kind)| {
        if !update_analysis_status(
            &app,
            &asset_id,
            &task_id,
            &source_reference,
            "analyzing",
            None,
            None,
        )? {
            return Ok(None);
        }
        let source = PathBuf::from(&source_reference);
        let mut metadata = probe_media(&source)?;
        metadata.thumbnail_path = generate_thumbnail(&app, &asset_id, &source, &kind)?;
        if kind == "video" {
            (metadata.keyframes, metadata.scene_segments) =
                generate_video_keyframes(&app, &asset_id, &source, metadata.duration_ms)?;
        }
        metadata.ocr_evidence = extract_ocr_evidence(&kind, &source, &metadata.keyframes)?;
        Ok(Some((source_reference, metadata)))
    });
    match result {
        Ok(Some((source_reference, metadata))) => {
            if update_analysis_status(
                &app,
                &asset_id,
                &task_id,
                &source_reference,
                "ready",
                Some(&metadata),
                None,
            )
            .unwrap_or(false)
            {
                log::info!("Completed local media analysis for asset {asset_id}.");
            }
        }
        Ok(None) => {}
        Err(error) => {
            log::warn!("Local media analysis failed for asset {asset_id}: {error}");
            if let Some(source_reference) = terminal_source_reference {
                let _ = update_analysis_status(
                    &app,
                    &asset_id,
                    &task_id,
                    &source_reference,
                    "failed",
                    None,
                    Some(&error),
                );
            }
        }
    }
}

fn spawn_technical_analysis_tasks(app: AppHandle, tasks: Vec<(String, String)>) {
    if tasks.is_empty() {
        return;
    }
    let claimed_tasks = open_connection(&app)
        .ok()
        .map(|connection| {
            tasks
                .into_iter()
                .filter(|(_, task_id)| {
                    connection
                        .execute(
                            "UPDATE agent_tasks SET status = 'running', updated_at = ?1 WHERE id = ?2 AND tool_name = 'analyze_asset' AND status = 'queued'",
                            params![now_millis(), task_id],
                        )
                        .map(|updated| updated == 1)
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if claimed_tasks.is_empty() {
        return;
    }
    let slots = (0..claimed_tasks.len().min(MAX_TECHNICAL_ANALYSIS_WORKERS))
        .filter_map(|_| reserve_technical_analysis_worker())
        .collect::<Vec<_>>();
    if slots.is_empty() {
        if let Ok(connection) = open_connection(&app) {
            for (_, task_id) in &claimed_tasks {
                let _ = connection.execute(
                    "UPDATE agent_tasks SET status = 'queued', updated_at = ?1 WHERE id = ?2 AND tool_name = 'analyze_asset' AND status = 'running'",
                    params![now_millis(), task_id],
                );
            }
        }
        return;
    }
    let mut assignments = (0..slots.len()).map(|_| Vec::new()).collect::<Vec<_>>();
    let worker_count = assignments.len();
    for (index, task) in claimed_tasks.into_iter().enumerate() {
        assignments[index % worker_count].push(task);
    }
    for (slot, tasks) in slots.into_iter().zip(assignments) {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _slot = slot;
            for batch in tasks.chunks(VISUAL_ANALYSIS_BATCH_SIZE) {
                let mut completed_asset_ids_by_project = HashMap::<String, Vec<String>>::new();
                for (asset_id, task_id) in batch {
                    run_technical_analysis(app.clone(), asset_id.clone(), task_id.clone());
                    let ready_project_id = open_connection(&app).ok().and_then(|connection| {
                        connection
                            .query_row(
                                "SELECT project_id FROM assets WHERE id = ?1 AND analysis_status = 'ready'",
                                params![asset_id],
                                |row| row.get::<_, String>(0),
                            )
                            .ok()
                    });
                    if let Some(project_id) = ready_project_id {
                        completed_asset_ids_by_project
                            .entry(project_id)
                            .or_default()
                            .push(asset_id.clone());
                    }
                }
                for asset_ids in completed_asset_ids_by_project.into_values() {
                    let _ = queue_visual_analysis_batch(&app, &asset_ids);
                }
            }
        });
    }
}

pub(crate) fn resume_incomplete_analysis(app: &AppHandle) -> Result<(), String> {
    static RECOVERY_STARTED: AtomicBool = AtomicBool::new(false);
    if RECOVERY_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }
    if let Err(error) = recover_interrupted_visual_batches(app) {
        RECOVERY_STARTED.store(false, Ordering::Release);
        return Err(error);
    }
    if let Err(error) = backfill_queued_visual_batches(app) {
        RECOVERY_STARTED.store(false, Ordering::Release);
        return Err(error);
    }
    spawn_visual_analysis_worker(app.clone());

    let result = (|| {
        let connection = open_connection(app)?;
        connection
            .execute(
                "UPDATE agent_tasks SET status = 'queued', updated_at = ?1 WHERE tool_name = 'analyze_asset' AND status = 'running'",
                params![now_millis()],
            )
            .map_err(|error| error.to_string())?;
        let mut statement = connection
            .prepare(
                "
                SELECT id, input_json
                FROM agent_tasks
                WHERE tool_name = 'analyze_asset' AND status = 'queued'
                ORDER BY created_at ASC
                ",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        drop(statement);

        // Every asset that already has any analyze_asset task (queued, running,
        // completed, failed, or cancelled) must never be enqueued again.
        let mut tasked_asset_ids: HashSet<String> = HashSet::new();
        let mut pending_asset_ids: HashSet<String> = HashSet::new();
        let mut tasks = Vec::new();
        for (task_id, input_json) in rows {
            let asset_id = serde_json::from_str::<serde_json::Value>(&input_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("assetId")
                        .and_then(|asset_id| asset_id.as_str())
                        .map(str::to_owned)
                });
            let Some(asset_id) = asset_id else {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'failed', error_message = 'Stored analysis input is invalid.', updated_at = ?1 WHERE id = ?2",
                        params![now_millis(), task_id],
                    )
                    .map_err(|error| error.to_string())?;
                continue;
            };
            tasked_asset_ids.insert(asset_id.clone());
            if !pending_asset_ids.insert(asset_id.clone()) {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'cancelled', error_message = 'Superseded duplicate analysis task.', updated_at = ?1 WHERE id = ?2",
                        params![now_millis(), task_id],
                    )
                    .map_err(|error| error.to_string())?;
                continue;
            }
            tasks.push((asset_id.clone(), task_id));
        }

        // Collect assets that already settled (completed/failed/cancelled) so the
        // orphan sweep below never re-enqueues them.
        let mut settled = connection
            .prepare(
                "
                SELECT input_json
                FROM agent_tasks
                WHERE tool_name = 'analyze_asset' AND status IN ('completed', 'failed', 'cancelled')
                ",
            )
            .map_err(|error| error.to_string())?;
        let settled_rows = settled
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        drop(settled);
        for input_json in settled_rows {
            if let Some(asset_id) = serde_json::from_str::<serde_json::Value>(&input_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("assetId")
                        .and_then(|asset_id| asset_id.as_str())
                        .map(str::to_owned)
                })
            {
                tasked_asset_ids.insert(asset_id);
            }
        }

        // Re-enqueue analysis for assets that are queued or in progress but had
        // never had an analyze_asset task persisted (for example after an import
        // was interrupted), so they no longer sit in "正在分析媒体" forever.
        let mut orphan_statement = connection
            .prepare(
                "
                SELECT id, project_id
                FROM assets
                WHERE analysis_status IN ('queued', 'analyzing')
                ",
            )
            .map_err(|error| error.to_string())?;
        let orphan_assets = orphan_statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        drop(orphan_statement);
        let orphan_timestamp = now_millis();
        for (asset_id, project_id) in orphan_assets {
            if tasked_asset_ids.contains(&asset_id) {
                continue;
            }
            let task_id = Uuid::new_v4().to_string();
            connection
                .execute(
                    "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset', 'queued', ?3, ?4, ?5)",
                    params![task_id, project_id, serde_json::json!({ "assetId": asset_id }).to_string(), orphan_timestamp, orphan_timestamp],
                )
                .map_err(|error| error.to_string())?;
            tasks.push((asset_id.clone(), task_id));
        }
        tasks.truncate(STARTUP_ANALYSIS_BATCH);
        spawn_technical_analysis_tasks(app.clone(), tasks);
        Ok(())
    })();

    if result.is_err() {
        RECOVERY_STARTED.store(false, Ordering::Release);
    }
    result
}

fn enqueue_technical_analysis(app: &AppHandle, assets: &[Asset]) -> Result<(), String> {
    let timestamp = now_millis();
    let connection = open_connection(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let tasks = assets
        .iter()
        .map(|asset| {
            (
                asset.id.clone(),
                asset.project_id.clone(),
                Uuid::new_v4().to_string(),
            )
        })
        .collect::<Vec<_>>();
    for (asset_id, project_id, task_id) in &tasks {
        transaction.execute(
            "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset', 'queued', ?3, ?4, ?5)",
            params![task_id, project_id, serde_json::json!({ "assetId": asset_id }).to_string(), timestamp, timestamp],
        ).map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    spawn_technical_analysis_tasks(
        app.clone(),
        tasks
            .into_iter()
            .map(|(asset_id, _, task_id)| (asset_id, task_id))
            .collect(),
    );
    Ok(())
}

/// Queues a fresh local analysis only for assets already owned by this project.
/// It is deliberately a controlled tool boundary: the model chooses *which*
/// imported evidence to inspect, while Rust owns filesystem access and task
/// persistence. Active analyses are never duplicated.
pub(crate) fn request_asset_analysis(
    app: &AppHandle,
    project_id: &str,
    asset_ids: &[String],
) -> Result<usize, String> {
    if asset_ids.is_empty() {
        return Err("Select one or more imported assets to analyze.".to_owned());
    }
    let connection = open_connection(app)?;
    let timestamp = now_millis();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut tasks = Vec::new();
    for asset_id in asset_ids {
        let row = transaction.query_row(
            "SELECT source_reference, analysis_status FROM assets WHERE id = ?1 AND project_id = ?2",
            params![asset_id, project_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).optional().map_err(|error| error.to_string())?;
        let Some((source_reference, status)) = row else {
            return Err("Selected asset is not available in this project.".to_owned());
        };
        if !Path::new(&source_reference).is_file() {
            return Err("Selected asset source is no longer available.".to_owned());
        }
        if status == "ready" {
            continue;
        }
        let active: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM agent_tasks WHERE project_id = ?1 AND tool_name = 'analyze_asset' AND status IN ('queued', 'running') AND input_json LIKE ?2",
            params![project_id, format!("%\"assetId\":\"{asset_id}\"%")],
            |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        if active > 0 {
            continue;
        }
        let task_id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset', 'queued', ?3, ?4, ?5)",
            params![task_id, project_id, serde_json::json!({ "assetId": asset_id }).to_string(), timestamp, timestamp],
        ).map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE assets SET analysis_status = 'queued', updated_at = ?1 WHERE id = ?2",
                params![timestamp, asset_id],
            )
            .map_err(|error| error.to_string())?;
        tasks.push((asset_id.clone(), task_id));
    }
    transaction.commit().map_err(|error| error.to_string())?;
    let queued = tasks.len();
    spawn_technical_analysis_tasks(app.clone(), tasks);
    Ok(queued)
}

#[tauri::command]
pub fn retry_asset_analysis_batch(
    app: AppHandle,
    project_id: String,
    asset_ids: Vec<String>,
) -> Result<BatchAssetActionResult, String> {
    if asset_ids.len() > 200 {
        return Err("Select no more than 200 assets for one batch action.".to_owned());
    }
    let requested_count = asset_ids.len();
    let updated_count = request_asset_analysis(&app, &project_id, &asset_ids)?;
    let connection = open_connection(&app)?;
    connection.execute(
        "INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, after_json, created_at) VALUES (?1, ?2, 'user', 'retry_asset_analysis_batch', 'project_assets', ?2, ?3, ?4)",
        params![Uuid::new_v4().to_string(), project_id, serde_json::json!({ "requestedCount": requested_count, "queuedCount": updated_count }).to_string(), now_millis()],
    ).map_err(|error| error.to_string())?;
    Ok(BatchAssetActionResult {
        requested_count,
        updated_count,
        skipped_count: requested_count.saturating_sub(updated_count),
    })
}

#[tauri::command]
pub fn skip_asset_visual_analysis_batch(
    app: AppHandle,
    project_id: String,
    asset_ids: Vec<String>,
) -> Result<BatchAssetActionResult, String> {
    if asset_ids.is_empty() {
        return Err("Select one or more imported assets to skip visual analysis.".to_owned());
    }
    if asset_ids.len() > 200 {
        return Err("Select no more than 200 assets for one batch action.".to_owned());
    }
    let requested_count = asset_ids.len();
    let connection = open_connection(&app)?;
    let timestamp = now_millis();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut updated_count = 0usize;
    for asset_id in &asset_ids {
        let row = transaction.query_row(
            "SELECT kind, analysis_status, metadata_json FROM assets WHERE id = ?1 AND project_id = ?2",
            params![asset_id, project_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        ).optional().map_err(|error| error.to_string())?;
        let Some((kind, analysis_status, metadata_json)) = row else {
            return Err("Selected asset is not available in this project.".to_owned());
        };
        if analysis_status != "ready" || !matches!(kind.as_str(), "video" | "image") {
            continue;
        }
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.visual_analysis_status == "skipped" {
            continue;
        }
        metadata.visual_analysis_status = "skipped".to_owned();
        metadata.visual_analysis_note = Some("visual_analysis_skipped_by_user".to_owned());
        metadata.visual_evidence.clear();
        transaction.execute(
            "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3 AND project_id = ?4",
            params![serde_json::to_string(&metadata).map_err(|error| error.to_string())?, timestamp, asset_id, project_id],
        ).map_err(|error| error.to_string())?;
        updated_count += 1;
    }
    transaction.execute(
        "INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, after_json, created_at) VALUES (?1, ?2, 'user', 'skip_asset_visual_analysis_batch', 'project_assets', ?2, ?3, ?4)",
        params![Uuid::new_v4().to_string(), project_id, serde_json::json!({ "requestedCount": requested_count, "skippedCount": updated_count }).to_string(), timestamp],
    ).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(BatchAssetActionResult {
        requested_count,
        updated_count,
        skipped_count: requested_count.saturating_sub(updated_count),
    })
}

#[tauri::command]
pub fn get_asset_task_center(
    app: AppHandle,
    project_id: String,
) -> Result<AssetTaskCenter, String> {
    let connection = open_connection(&app)?;
    let technical = connection.query_row(
        "SELECT SUM(analysis_status = 'queued'), SUM(analysis_status = 'analyzing'), SUM(analysis_status = 'failed') FROM assets WHERE project_id = ?1",
        params![project_id],
        |row| Ok(AssetTaskStageCounts { queued: row.get::<_, Option<i64>>(0)?.unwrap_or(0) as usize, running: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as usize, failed: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as usize, skipped: 0 }),
    ).map_err(|error| error.to_string())?;
    let visual = connection.query_row(
        "SELECT SUM(coalesce(json_extract(metadata_json, '$.visualAnalysisStatus'), 'queued') = 'queued'), SUM(json_extract(metadata_json, '$.visualAnalysisStatus') = 'running'), SUM(json_extract(metadata_json, '$.visualAnalysisStatus') = 'failed'), SUM(json_extract(metadata_json, '$.visualAnalysisStatus') = 'skipped') FROM assets WHERE project_id = ?1 AND kind IN ('video', 'image') AND analysis_status = 'ready'",
        params![project_id],
        |row| Ok(AssetTaskStageCounts { queued: row.get::<_, Option<i64>>(0)?.unwrap_or(0) as usize, running: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as usize, failed: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as usize, skipped: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as usize }),
    ).map_err(|error| error.to_string())?;
    let mut statement = connection.prepare(
        "SELECT id, display_name, 'technical', 'technical_analysis_failed', updated_at FROM assets WHERE project_id = ?1 AND analysis_status = 'failed'
         UNION ALL
         SELECT id, display_name, 'visual', 'visual_analysis_failed', updated_at FROM assets WHERE project_id = ?1 AND json_extract(metadata_json, '$.visualAnalysisStatus') = 'failed'
         ORDER BY updated_at DESC LIMIT 50",
    ).map_err(|error| error.to_string())?;
    let recent_failures = statement
        .query_map(params![project_id], |row| {
            Ok(AssetTaskFailure {
                asset_id: row.get(0)?,
                display_name: row.get(1)?,
                stage: row.get(2)?,
                reason_code: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(AssetTaskCenter {
        technical,
        visual,
        recent_failures,
    })
}

fn validate_batch_asset_ids(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    asset_ids: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut asset_ids = asset_ids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    asset_ids.sort();
    if asset_ids.is_empty() || asset_ids.len() > 200 {
        return Err("Select between 1 and 200 assets for one batch action.".to_owned());
    }
    for asset_id in &asset_ids {
        let exists: i64 = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM assets WHERE id = ?1 AND project_id = ?2)",
                params![asset_id, project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if exists == 0 {
            return Err("Selected asset is not available in this project.".to_owned());
        }
    }
    Ok(asset_ids)
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

fn normalized_asset_label(value: String, kind: &str) -> Result<String, String> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > 64 {
        return Err(format!("{kind} must contain between 1 and 64 characters."));
    }
    Ok(value)
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

fn drain_pending_analysis(app: &AppHandle, project_id: &str) -> Result<(), String> {
    if ANALYSIS_WORKER_COUNT.load(Ordering::Acquire) >= MAX_TECHNICAL_ANALYSIS_WORKERS {
        return Ok(());
    }
    let connection = open_connection(app)?;
    let mut statement = connection
        .prepare(
            "
            SELECT id, input_json
            FROM agent_tasks
            WHERE project_id = ?1 AND tool_name = 'analyze_asset' AND status = 'queued'
            ORDER BY created_at ASC
            LIMIT ?2
            ",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id, DRAIN_ANALYSIS_BATCH as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    if rows.is_empty() {
        return Ok(());
    }
    let mut tasks = Vec::with_capacity(rows.len());
    for (task_id, input_json) in rows {
        let asset_id = serde_json::from_str::<serde_json::Value>(&input_json)
            .ok()
            .and_then(|value| {
                value
                    .get("assetId")
                    .and_then(|asset_id| asset_id.as_str())
                    .map(str::to_owned)
            });
        if let Some(asset_id) = asset_id {
            tasks.push((asset_id, task_id));
        } else {
            connection
                .execute(
                    "UPDATE agent_tasks SET status = 'failed', error_message = 'Stored analysis input is invalid.', updated_at = ?1 WHERE id = ?2",
                    params![now_millis(), task_id],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    spawn_technical_analysis_tasks(app.clone(), tasks);
    Ok(())
}

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
        let asset = Asset {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.to_owned(),
            kind: asset_kind(&source),
            display_name,
            folder_name: None,
            relative_path: None,
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
            params![asset.id, asset.project_id, asset.kind, asset.display_name, source.to_string_lossy(), folder_reference.map(|path| path.to_string_lossy().into_owned()), asset.analysis_status, asset.created_at, asset.updated_at],
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

/// Wait only in a background Agent action for the already-queued analysis of a
/// freshly downloaded track. This never retries a failed analysis and keeps the
/// ready gate that protects timelines and delivery tools.
pub(crate) fn wait_for_asset_ready(
    app: &AppHandle,
    project_id: &str,
    asset_id: &str,
) -> Result<Asset, String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    loop {
        let assets = list_assets(app.clone(), project_id.to_owned())?;
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
        if preserve_analysis {
            transaction.execute(
                "UPDATE assets SET source_reference = ?1, folder_reference = ?2, updated_at = ?3 WHERE id = ?4 AND project_id = ?5",
                params![source.to_string_lossy(), source_directory.as_str(), timestamp, asset_id, project_id],
            ).map_err(|error| error.to_string())?;
        } else {
            transaction.execute(
                "UPDATE agent_tasks SET status = 'cancelled', error_message = 'Superseded after the source file was relinked.', updated_at = ?1 WHERE project_id = ?2 AND tool_name = 'analyze_asset' AND status IN ('queued', 'running') AND input_json LIKE ?3",
                params![timestamp, project_id, format!("%\"assetId\":\"{asset_id}\"%")],
            ).map_err(|error| error.to_string())?;
            transaction.execute(
                "UPDATE assets SET source_reference = ?1, folder_reference = ?2, analysis_status = 'queued', metadata_json = '{}', updated_at = ?3 WHERE id = ?4 AND project_id = ?5",
                params![source.to_string_lossy(), source_directory.as_str(), timestamp, asset_id, project_id],
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
    let mut statement = connection.prepare("SELECT id, display_name, source_reference FROM assets WHERE project_id=?1 ORDER BY created_at, id").map_err(|error| error.to_string())?;
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
            Ok(bytes) => copied.push(serde_json::json!({"assetId":asset_id,"displayName":display_name,"collectedFile":format!("media/{collected_name}"),"bytes":bytes})),
            Err(_) => unavailable_count += 1,
        }
    }
    let manifest = serde_json::json!({"format":"assembly-video-agent-media-collection-v1","projectId":project_id,"createdAt":now_millis(),"assets":copied,"unavailableCount":unavailable_count});
    fs::write(
        package.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let timestamp = now_millis();
    connection.execute("INSERT INTO operation_logs (id,project_id,actor,operation_type,entity_type,entity_id,after_json,created_at) VALUES (?1,?2,'user','collect_project_media','project',?2,?3,?4)", params![Uuid::new_v4().to_string(), project_id, serde_json::json!({"copiedCount":copied.len(),"unavailableCount":unavailable_count}).to_string(), timestamp]).map_err(|error| error.to_string())?;
    Ok(CollectProjectMediaResult {
        copied_count: copied.len(),
        unavailable_count,
        output_directory: package.to_string_lossy().into_owned(),
    })
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
    let (total, unchecked, online, missing, changed, unreadable): (i64,i64,i64,i64,i64,i64) = connection.query_row("SELECT COUNT(*), coalesce(SUM(CASE WHEN coalesce(h.status,'unchecked')='unchecked' THEN 1 ELSE 0 END),0), coalesce(SUM(CASE WHEN h.status='online' THEN 1 ELSE 0 END),0), coalesce(SUM(CASE WHEN h.status='missing' THEN 1 ELSE 0 END),0), coalesce(SUM(CASE WHEN h.status='changed' THEN 1 ELSE 0 END),0), coalesce(SUM(CASE WHEN h.status='unreadable' THEN 1 ELSE 0 END),0) FROM assets a LEFT JOIN asset_source_health h ON h.asset_id=a.id WHERE a.project_id=?1", params![project_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).map_err(|error| error.to_string())?;
    let active = connection.query_row("SELECT id,status,result_json FROM agent_tasks WHERE project_id=?1 AND tool_name='scan_asset_health' AND status IN ('queued','running') ORDER BY created_at DESC LIMIT 1", params![project_id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,Option<String>>(2)?))).optional().map_err(|error| error.to_string())?;
    let checked = active
        .as_ref()
        .and_then(|(_, _, json)| json.as_ref())
        .and_then(|json| serde_json::from_str::<Value>(json).ok())
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
        active_task_id: active.as_ref().map(|v| v.0.clone()),
        active_task_status: active.map(|v| v.1),
    })
}

pub(crate) fn get_asset_health_summary_for_agent(
    connection: &rusqlite::Connection,
    project_id: &str,
) -> Result<Value, String> {
    let (total, unchecked, online, missing, changed, unreadable, last_checked_at):
        (i64, i64, i64, i64, i64, i64, Option<i64>) = connection
        .query_row(
            "SELECT COUNT(*),
             coalesce(SUM(CASE WHEN coalesce(h.status,'unchecked')='unchecked' THEN 1 ELSE 0 END),0),
             coalesce(SUM(CASE WHEN h.status='online' THEN 1 ELSE 0 END),0),
             coalesce(SUM(CASE WHEN h.status='missing' THEN 1 ELSE 0 END),0),
             coalesce(SUM(CASE WHEN h.status='changed' THEN 1 ELSE 0 END),0),
             coalesce(SUM(CASE WHEN h.status='unreadable' THEN 1 ELSE 0 END),0),
             MAX(h.checked_at)
             FROM assets a LEFT JOIN asset_source_health h ON h.asset_id=a.id
             WHERE a.project_id=?1",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        )
        .map_err(|error| error.to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT reason_code, COUNT(*) FROM asset_source_health
             WHERE project_id=?1 AND status IN ('missing','unreadable') AND reason_code IS NOT NULL
             GROUP BY reason_code ORDER BY COUNT(*) DESC, reason_code ASC",
        )
        .map_err(|error| error.to_string())?;
    let reason_counts = statement
        .query_map(params![project_id], |row| {
            Ok(serde_json::json!({
                "code": row.get::<_, String>(0)?,
                "count": row.get::<_, i64>(1)?,
            }))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let active_scan = connection
        .query_row(
            "SELECT status FROM agent_tasks WHERE project_id=?1 AND tool_name='scan_asset_health' AND status IN ('queued','running') ORDER BY created_at DESC LIMIT 1",
            params![project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let failure_count = missing + unreadable;
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

fn asset_folder_metadata(
    source_reference: &str,
    folder_reference: Option<String>,
) -> (Option<String>, Option<String>) {
    let Some(folder_reference) = folder_reference else {
        return (None, None);
    };
    let folder = Path::new(&folder_reference);
    let folder_name = folder
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    let relative_path = Path::new(source_reference)
        .strip_prefix(folder)
        .ok()
        .and_then(|path| path.to_str())
        .map(str::to_owned);
    (folder_name, relative_path)
}

fn asset_safe_directory(source_reference: &str, folder_reference: Option<&str>) -> Option<String> {
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

fn legacy_asset_directories(rows: &[(String, String)]) -> HashMap<String, String> {
    let parents = rows
        .iter()
        .map(|(_, source)| {
            source
                .replace('/', "\\")
                .split('\\')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|parts| parts.len() > 1)
        .map(|mut parts| {
            parts.pop();
            parts
        })
        .collect::<Vec<_>>();
    if parents.len() < 2 {
        return HashMap::new();
    }
    let mut common = parents[0].len();
    for parts in &parents[1..] {
        common = common.min(parts.len());
        common = (0..common)
            .take_while(|index| parents[0][*index].eq_ignore_ascii_case(&parts[*index]))
            .count();
    }
    let base = common.saturating_sub(1);
    rows.iter()
        .filter_map(|(id, source)| {
            let mut parts = source
                .replace('/', "\\")
                .split('\\')
                .map(str::to_owned)
                .collect::<Vec<_>>();
            parts.pop()?;
            (parts.len() > base).then(|| (id.clone(), parts[base..].join("\\")))
        })
        .collect()
}

fn project_asset_directories(
    connection: &rusqlite::Connection,
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
    let legacy = rows
        .iter()
        .filter(|(_, _, root)| root.is_none())
        .map(|(id, source, _)| (id.clone(), source.clone()))
        .collect::<Vec<_>>();
    let mut directories = legacy_asset_directories(&legacy);
    for (id, source, root) in rows {
        if let Some(directory) = asset_safe_directory(&source, root.as_deref()) {
            directories.insert(id, directory);
        }
    }
    Ok(directories)
}

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
    Ok(
        serde_json::json!({ "candidates": candidates, "total": total, "nextOffset": next_offset, "limit": limit }),
    )
}

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
    Ok(serde_json::json!({"segments":results,"total":total,"nextOffset":next_offset,"limit":limit}))
}

#[tauri::command]
pub fn list_assets(app: AppHandle, project_id: String) -> Result<Vec<Asset>, String> {
    let connection = open_connection(&app)?;
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
            let source_reference: String = row.get(4)?;
            let folder_reference: Option<String> = row.get(5)?;
            let (folder_name, relative_path) =
                asset_folder_metadata(&source_reference, folder_reference);
            let metadata: TechnicalMetadata =
                serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default();
            Ok(Asset {
                id: row.get(0)?,
                project_id: row.get(1)?,
                kind: row.get(2)?,
                display_name: row.get(3)?,
                folder_name,
                relative_path,
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
    drain_pending_analysis(&app, &project_id)?;
    Ok(assets)
}

#[tauri::command]
pub fn list_asset_page(
    app: AppHandle,
    project_id: String,
    search: Option<String>,
    kind: Option<String>,
    analysis_status: Option<String>,
    visual_status: Option<String>,
    folder_name: Option<String>,
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
    let folder_name = folder_name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let user_filter =
        user_filter.filter(|value| matches!(value.as_str(), "favorite" | "excluded" | "available"));
    let collection_id = collection_id
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let connection = open_connection(&app)?;
    let asset_directories = project_asset_directories(&connection, &project_id)?;
    let folder_asset_ids = if let Some(folder) = folder_name.as_deref() {
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
                let source_reference: String = row.get(4)?;
                let folder_reference: Option<String> = row.get(5)?;
                let (folder_name, relative_path) =
                    asset_folder_metadata(&source_reference, folder_reference);
                let metadata: TechnicalMetadata =
                    serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default();
                Ok(Asset {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    kind: row.get(2)?,
                    display_name: row.get(3)?,
                    folder_name,
                    relative_path,
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
    let mut folders = asset_directories.values().cloned().collect::<Vec<_>>();
    folders.sort_by(|left, right| left.to_lowercase().cmp(&right.to_lowercase()));
    folders.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    let unfiled_count = counts.total.saturating_sub(asset_directories.len());
    if unfiled_count > 0 {
        folders.push("__unfiled__".to_owned());
    }
    drain_pending_analysis(&app, &project_id)?;
    Ok(AssetPage {
        items,
        total: total as usize,
        offset,
        limit,
        folders,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_scoring_handles_ascii_and_cjk_and_preserves_fifo_ties() {
        let brief = lexical_tokens("Coffee launch 上海咖啡");
        assert!(lexical_overlap_score(&brief, "coffee product") > 0);
        assert!(lexical_overlap_score(&brief, "上海街景 咖啡店") > 0);
        assert_eq!(lexical_overlap_score(&brief, "mountain hiking"), 0);

        let ranked = rank_visual_batches(vec![
            VisualBatchRanking {
                task_id: "first".to_owned(),
                created_at: 10,
                priority: 0,
            },
            VisualBatchRanking {
                task_id: "second".to_owned(),
                created_at: 20,
                priority: 0,
            },
            VisualBatchRanking {
                task_id: "relevant".to_owned(),
                created_at: 30,
                priority: 1,
            },
        ]);
        assert_eq!(
            ranked
                .iter()
                .map(|batch| batch.task_id.as_str())
                .collect::<Vec<_>>(),
            vec!["relevant", "first", "second"]
        );
    }

    #[test]
    fn visual_provider_payload_contains_no_local_path_hints() {
        let local_path = r"D:\private\客户项目\coffee-launch.mp4";
        let content = visual_model_content(&[("asset-1".to_owned(), Some(1200), vec![1, 2, 3])]);
        let payload = serde_json::to_string(&content).expect("visual content should serialize");

        assert!(!payload.contains(local_path));
        assert!(!payload.contains("coffee-launch.mp4"));
        assert!(payload.contains("asset-1"));
        assert!(payload.contains("sourceTimeMs=1200"));
    }

    #[test]
    fn technical_worker_slots_are_bounded_and_released() {
        release_all_technical_analysis_workers();
        let first = reserve_technical_analysis_worker();
        let second = reserve_technical_analysis_worker();
        assert!(first.is_some());
        assert!(second.is_some());
        assert!(reserve_technical_analysis_worker().is_none());

        drop(first);
        assert!(reserve_technical_analysis_worker().is_some());
        drop(second);
        release_all_technical_analysis_workers();
    }

    #[test]
    fn scene_scan_reduces_frames_and_resolution_before_comparison() {
        let filter = scene_scan_filter();
        let fps = filter.find("fps=4").expect("scene scan must limit fps");
        let scale = filter
            .find("scale=320")
            .expect("scene scan must reduce resolution");
        let scene = filter
            .find("select=gt(scene")
            .expect("scene scan must compare frames");

        assert!(fps < scale && scale < scene);
    }

    #[test]
    fn asset_page_filter_combines_search_folder_and_storyboard_readiness() {
        let connection = rusqlite::Connection::open_in_memory().expect("open test database");
        connection.execute_batch("CREATE TABLE assets (id TEXT, project_id TEXT, kind TEXT, display_name TEXT, source_reference TEXT, folder_reference TEXT, analysis_status TEXT, metadata_json TEXT, created_at INTEGER, updated_at INTEGER); CREATE TABLE asset_user_metadata (asset_id TEXT, note TEXT, favorite INTEGER, excluded INTEGER); CREATE TABLE asset_tags (id TEXT, name TEXT); CREATE TABLE asset_tag_assignments (asset_id TEXT, tag_id TEXT); CREATE TABLE asset_collection_items (asset_id TEXT, collection_id TEXT);").expect("create assets table");
        connection.execute(
            "INSERT INTO assets VALUES (?1, 'project', 'video', ?2, ?3, ?4, 'ready', ?5, ?6, ?6)",
            params!["ready", "Coffee closeup", r"D:\media\campaign\coffee.mp4", r"D:\media\campaign", r#"{"visualAnalysisStatus":"ready","visualEvidence":[{"subjects":["coffee"]}]}"#, 2],
        ).expect("insert ready asset");
        connection.execute(
            "INSERT INTO assets VALUES (?1, 'project', 'video', ?2, ?3, ?4, 'ready', ?5, ?6, ?6)",
            params!["waiting", "Coffee wide", r"D:\media\campaign\wide.mp4", r"D:\media\campaign", r#"{"visualAnalysisStatus":"queued","visualEvidence":[]}"#, 1],
        ).expect("insert queued visual asset");
        let count: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM assets WHERE {ASSET_PAGE_FILTER_SQL}"),
                params![
                    "project",
                    "coffee",
                    Option::<String>::None,
                    Option::<String>::None,
                    "storyboard-ready",
                    r#"["ready"]"#,
                    Option::<String>::None,
                    Option::<String>::None
                ],
                |row| row.get(0),
            )
            .expect("query filtered assets");
        assert_eq!(count, 1);
        let selected: String = connection.query_row(
            &format!("SELECT id FROM assets WHERE {ASSET_PAGE_FILTER_SQL} ORDER BY created_at DESC LIMIT 1 OFFSET 0"),
            params!["project", "coffee", "video", "ready", "storyboard-ready", r#"["ready"]"#, Option::<String>::None, Option::<String>::None],
            |row| row.get(0),
        ).expect("query first page");
        assert_eq!(selected, "ready");
    }

    #[test]
    fn an_inflight_visual_batch_cannot_overwrite_an_explicit_user_skip() {
        let explicit_skip = TechnicalMetadata {
            visual_analysis_status: "skipped".to_owned(),
            visual_analysis_note: Some("visual_analysis_skipped_by_user".to_owned()),
            ..TechnicalMetadata::default()
        };
        assert!(preserves_explicit_visual_skip(&explicit_skip, "ready"));
        let automatic_skip = TechnicalMetadata {
            visual_analysis_status: "skipped".to_owned(),
            visual_analysis_note: Some("visual_analysis_not_applicable".to_owned()),
            ..TechnicalMetadata::default()
        };
        assert!(!preserves_explicit_visual_skip(&automatic_skip, "queued"));
    }

    #[test]
    fn agent_asset_search_is_bounded_scoped_and_redacts_private_text() {
        let connection = rusqlite::Connection::open_in_memory().expect("open test database");
        crate::db::migrate(&connection).expect("migrate test database");
        connection.execute("INSERT INTO projects (id, name, created_at, updated_at) VALUES ('project', 'Test', 1, 1)", []).expect("insert project");
        let metadata = serde_json::json!({
            "durationMs": 5000,
            "width": 1920,
            "height": 1080,
            "fps": 30.0,
            "hasAudio": true,
            "thumbnailPath": null,
            "keyframes": [],
            "sceneSegments": [{"startMs":0,"endMs":5000}],
            "visualAnalysisStatus": "ready",
            "visualEvidence": [{"timeMs":1000,"subjects":["coffee"],"scene":"studio","actions":[],"products":[],"qualityNotes":[]}],
            "ocrEvidence": [{"timeMs":1000,"text":"PRIVATE OCR coffee"}]
        });
        connection.execute("INSERT INTO assets (id, project_id, kind, display_name, source_reference, analysis_status, metadata_json, created_at, updated_at) VALUES ('asset', 'project', 'video', 'Coffee shot', 'D:\\private\\coffee.mp4', 'ready', ?1, 1, 1)", params![metadata.to_string()]).expect("insert asset");
        connection.execute("INSERT INTO asset_user_metadata (asset_id, project_id, favorite, rating, note, excluded, updated_at) VALUES ('asset', 'project', 1, 5, 'PRIVATE NOTE coffee', 0, 1)", []).expect("insert metadata");
        let result = search_assets_for_agent(
            &connection,
            "project",
            Some("studio"),
            Some("video"),
            Some(1000),
            Some(6000),
            Some(4),
            true,
            None,
            None,
            0,
            100,
        )
        .expect("search assets");
        let serialized = result.to_string();
        assert_eq!(result["limit"], 20);
        assert_eq!(result["total"], 1);
        assert!(serialized.contains("visual_evidence_match"));
        assert!(!serialized.contains("PRIVATE"));
        assert!(!serialized.contains("D:\\\\private"));
        connection
            .execute(
                "UPDATE asset_user_metadata SET excluded = 1 WHERE asset_id = 'asset'",
                [],
            )
            .expect("exclude asset");
        let excluded = search_assets_for_agent(
            &connection,
            "project",
            Some("studio"),
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            0,
            12,
        )
        .expect("search excluded");
        assert_eq!(excluded["total"], 0);
    }

    #[test]
    fn segment_search_returns_exact_ranges_without_private_ocr_text() {
        let connection = rusqlite::Connection::open_in_memory().expect("open database");
        connection.execute_batch("CREATE TABLE assets (id TEXT, project_id TEXT, kind TEXT, display_name TEXT, analysis_status TEXT, metadata_json TEXT, updated_at INTEGER); CREATE TABLE asset_user_metadata (asset_id TEXT, excluded INTEGER); CREATE TABLE asset_source_health (asset_id TEXT, status TEXT);").expect("create tables");
        let metadata = serde_json::json!({"durationMs":6000,"width":1920,"height":1080,"fps":30.0,"hasAudio":true,"thumbnailPath":null,"keyframes":[],"sceneSegments":[{"startMs":0,"endMs":3000},{"startMs":3000,"endMs":6000}],"ocrEvidence":[{"timeMs":4200,"text":"PRIVATE launch offer"}],"visualEvidence":[{"timeMs":4500,"subjects":["product"],"scene":"showroom","actions":[],"products":[],"qualityNotes":[]}],"visualAnalysisStatus":"ready"});
        connection
            .execute(
                "INSERT INTO assets VALUES ('asset','project','video','Launch reel','ready',?1,1)",
                params![metadata.to_string()],
            )
            .expect("insert asset");
        let result = search_asset_segments_for_agent(&connection, "project", "launch", None, 0, 20)
            .expect("search segments");
        assert_eq!(result["total"], 2);
        assert_eq!(result["segments"][1]["sourceStartMs"], 3000);
        assert!(!result.to_string().contains("PRIVATE"));
    }

    #[test]
    fn collected_media_names_are_collision_safe_and_windows_safe() {
        let first = safe_collected_name("launch:final?.mp4", "12345678-one");
        let second = safe_collected_name("launch:final?.mp4", "87654321-two");
        assert_eq!(first, "launch_final_-12345678.mp4");
        assert_ne!(first, second);
        assert!(!first.contains(':') && !first.contains('?'));
    }

    #[test]
    fn agent_health_summary_returns_safe_reason_counts_without_paths() {
        let connection = rusqlite::Connection::open_in_memory().expect("open database");
        connection.execute_batch(
            "CREATE TABLE assets (id TEXT, project_id TEXT);
             CREATE TABLE asset_source_health (asset_id TEXT, project_id TEXT, status TEXT, reason_code TEXT, checked_at INTEGER);
             CREATE TABLE agent_tasks (project_id TEXT, tool_name TEXT, status TEXT, created_at INTEGER);
             INSERT INTO assets VALUES ('a','project'),('b','project');
             INSERT INTO asset_source_health VALUES ('a','project','unreadable','access_denied',10);
             INSERT INTO asset_source_health VALUES ('b','project','unreadable','access_denied',11);",
        ).expect("create health fixtures");
        let result =
            get_asset_health_summary_for_agent(&connection, "project").expect("summarize health");
        assert_eq!(result["unreadable"], 2);
        assert_eq!(result["reasonCounts"][0]["code"], "access_denied");
        assert_eq!(result["reasonCounts"][0]["count"], 2);
        assert_eq!(result["reasonedFailureCount"], 2);
        assert_eq!(result["unexplainedFailureCount"], 0);
        assert_eq!(result["reasonEvidenceAvailable"], true);
        assert!(!result.to_string().contains('\\'));
    }

    #[test]
    fn legacy_individual_imports_rebuild_a_safe_relative_folder_tree() {
        let rows = vec![
            (
                "a".to_owned(),
                r"D:\editing\campaign\workers\a.mp4".to_owned(),
            ),
            (
                "b".to_owned(),
                r"D:\editing\campaign\opening\b.mp4".to_owned(),
            ),
        ];
        let directories = legacy_asset_directories(&rows);
        assert_eq!(
            directories.get("a").map(String::as_str),
            Some(r"campaign\workers")
        );
        assert_eq!(
            directories.get("b").map(String::as_str),
            Some(r"campaign\opening")
        );
        assert!(directories.values().all(|value| !value.contains(":")));
    }
}
