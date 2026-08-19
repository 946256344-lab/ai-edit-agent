//! 技术分析 worker：FFprobe 元数据、FFmpeg 缩略图/关键帧/场景检测、Tesseract OCR
//! 与分析任务队列。所有分析结果写入 SQLite；原始媒体只读不修改。

use crate::db::{now_millis, open_connection};
use crate::models::{
    Asset, AssetTaskCenter, AssetTaskFailure, AssetTaskStageCounts, BatchAssetActionResult,
    KeyframeMetadata, OcrEvidence, SceneSegment, TechnicalMetadata,
};
use crate::process::{hidden_command, run_hidden_command_with_timeout, HiddenCommandError};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const KEYFRAME_COUNT: usize = 4;
pub(crate) const MAX_INITIAL_OCR_FRAMES: usize = 2;
pub(crate) const MAX_TECHNICAL_ANALYSIS_WORKERS: usize = 2;
pub(crate) const STARTUP_ANALYSIS_BATCH: usize = 4;
pub(crate) const DRAIN_ANALYSIS_BATCH: usize = 4;
const FFPROBE_TIMEOUT: Duration = Duration::from_secs(20);
const THUMBNAIL_FFMPEG_TIMEOUT: Duration = Duration::from_secs(30);
const SCENE_SCAN_FFMPEG_TIMEOUT: Duration = Duration::from_secs(45);
const FALLBACK_FRAME_FFMPEG_TIMEOUT: Duration = Duration::from_secs(20);
const TESSERACT_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) static ANALYSIS_WORKER_COUNT: AtomicUsize = AtomicUsize::new(0);

struct TechnicalAnalysisWorkerSlot;

impl Drop for TechnicalAnalysisWorkerSlot {
    fn drop(&mut self) {
        let _ = ANALYSIS_WORKER_COUNT.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            count.checked_sub(1)
        });
    }
}

#[cfg(test)]
pub(crate) fn release_all_technical_analysis_workers() {
    ANALYSIS_WORKER_COUNT.store(0, Ordering::Release);
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

pub(crate) fn asset_kind(path: &Path) -> String {
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

pub(crate) fn supported_media_file(path: &Path) -> bool {
    asset_kind(path) != "other"
}

pub(crate) fn collect_media_files(
    directory: &Path,
    sources: &mut Vec<PathBuf>,
) -> Result<(), String> {
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
        keyframe_grid_path: None,
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

fn generate_video_keyframes(
    app: &AppHandle,
    asset_id: &str,
    source: &Path,
    duration_ms: Option<i64>,
) -> Result<(Vec<KeyframeMetadata>, Vec<SceneSegment>), String> {
    let directory = derived_directory(app, asset_id)?;
    let duration_seconds = duration_ms.unwrap_or(0) as f64 / 1000.0;

    // 固定采样 4 帧：第 1 秒、1/3 处、2/3 处、最后 1 秒
    let times = if duration_seconds > 2.0 {
        vec![
            1.0,                               // 第 1 秒
            duration_seconds / 3.0,            // 1/3 处
            duration_seconds * 2.0 / 3.0,      // 2/3 处
            (duration_seconds - 1.0).max(1.5), // 最后 1 秒（不小于 1.5s）
        ]
    } else if duration_seconds > 0.0 {
        // 短视频回退：开头和中间
        vec![0.0, duration_seconds * 0.5]
    } else {
        vec![0.0]
    };

    // 提取每一帧
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
                HiddenCommandError::TimedOut => "Keyframe extraction timed out.".to_owned(),
                HiddenCommandError::Failed => "Keyframe extraction could not start.".to_owned(),
            },
        )?;
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
        .map(|pair| SceneSegment {
            start_ms: pair[0],
            end_ms: pair[1],
            scene_duration_ms: Some(pair[1] - pair[0]),
            visual_quality_score: None,
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

/// 选代表帧用于视觉分析：视频取中间帧（不易是片头卡/转场），图片取缩略图。
pub(crate) fn representative_frame(
    metadata: &TechnicalMetadata,
    kind: &str,
) -> Option<(String, Option<i64>)> {
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

    // 从 source_reference 重新计算 kind，防止文件类型变化时产生不一致
    let new_kind = asset_kind(Path::new(source_reference));

    let updated = if let Some(metadata_json) = metadata_json {
        transaction.execute(
            "UPDATE assets SET analysis_status = ?1, metadata_json = ?2, kind = ?3, updated_at = ?4 WHERE id = ?5 AND source_reference = ?6 AND EXISTS (SELECT 1 FROM agent_tasks WHERE id = ?7 AND tool_name = 'analyze_asset' AND status = 'running' AND json_extract(input_json, '$.assetId') = ?5)",
            params![status, metadata_json, new_kind, timestamp, asset_id, source_reference, task_id],
        ).map_err(|error| error.to_string())?
    } else {
        transaction
            .execute(
                "UPDATE assets SET analysis_status = ?1, kind = ?2, updated_at = ?3 WHERE id = ?4 AND source_reference = ?5 AND EXISTS (SELECT 1 FROM agent_tasks WHERE id = ?6 AND tool_name = 'analyze_asset' AND status = 'running' AND json_extract(input_json, '$.assetId') = ?4)",
                params![status, new_kind, timestamp, asset_id, source_reference, task_id],
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

            // 生成关键帧网格图
            if !metadata.keyframes.is_empty() {
                use crate::storyboard::multimodal::{generate_keyframe_grid, KeyframeGridConfig};
                let keyframe_paths: Vec<String> = metadata
                    .keyframes
                    .iter()
                    .map(|kf| kf.image_path.clone())
                    .collect();
                let derived_dir = derived_directory(&app, &asset_id)?;
                match generate_keyframe_grid(
                    &asset_id,
                    &keyframe_paths,
                    &derived_dir,
                    &KeyframeGridConfig::default(),
                ) {
                    Ok(Some(grid_path)) => {
                        metadata.keyframe_grid_path =
                            Some(grid_path.to_string_lossy().into_owned());
                    }
                    Ok(None) => {
                        log::warn!("Keyframe grid generation returned None for asset {asset_id}");
                    }
                    Err(error) => {
                        log::warn!(
                            "Failed to generate keyframe grid for asset {asset_id}: {error}"
                        );
                    }
                }
            }
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

pub(crate) fn spawn_technical_analysis_tasks(app: AppHandle, tasks: Vec<(String, String)>) {
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
            for batch in tasks.chunks(super::visual::VISUAL_ANALYSIS_BATCH_SIZE) {
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
                    let _ = super::visual::queue_visual_analysis_batch(&app, &asset_ids);
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
    log::info!("[PERF] resume_incomplete_analysis: starting");
    let start = std::time::Instant::now();

    let step_start = std::time::Instant::now();
    if let Err(error) = super::visual::recover_interrupted_visual_batches(app) {
        RECOVERY_STARTED.store(false, Ordering::Release);
        return Err(error);
    }
    log::info!(
        "[PERF] resume_incomplete_analysis: recover_interrupted_visual_batches took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    if let Err(error) = super::visual::backfill_queued_visual_batches(app) {
        RECOVERY_STARTED.store(false, Ordering::Release);
        return Err(error);
    }
    log::info!(
        "[PERF] resume_incomplete_analysis: backfill_queued_visual_batches took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    super::visual::spawn_visual_analysis_worker(app.clone());
    log::info!(
        "[PERF] resume_incomplete_analysis: spawn_visual_analysis_worker took {:?}",
        step_start.elapsed()
    );

    let step_start = std::time::Instant::now();
    let result = (|| {
        let connection = open_connection(app)?;
        connection
            .execute(
                "UPDATE agent_tasks SET status = 'queued', updated_at = ?1 WHERE tool_name = 'analyze_asset' AND status = 'running'",
                params![now_millis()],
            )
            .map_err(|error| error.to_string())?;

        let step_start_inner = std::time::Instant::now();
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
        log::info!("[PERF] resume_incomplete_analysis: query queued analyze_asset tasks took {:?}, found {} rows", step_start_inner.elapsed(), rows.len());

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
        log::info!(
            "[PERF] resume_incomplete_analysis: collected {} tasked asset IDs",
            tasked_asset_ids.len()
        );

        // Re-enqueue analysis for assets that are queued or in progress but had
        // never had an analyze_asset task persisted (for example after an import
        // was interrupted), so they no longer sit in "正在分析媒体" forever.
        let step_start_inner = std::time::Instant::now();
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
        log::info!(
            "[PERF] resume_incomplete_analysis: query orphan assets took {:?}, found {} orphans",
            step_start_inner.elapsed(),
            orphan_assets.len()
        );
        let orphan_timestamp = now_millis();
        let mut created_orphan_tasks = 0;
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
            created_orphan_tasks += 1;
        }
        log::info!("[PERF] resume_incomplete_analysis: created {} orphan tasks, total tasks before truncate: {}", created_orphan_tasks, tasks.len());
        tasks.truncate(STARTUP_ANALYSIS_BATCH);
        log::info!("[PERF] resume_incomplete_analysis: will spawn {} analysis tasks (STARTUP_ANALYSIS_BATCH={})", tasks.len(), STARTUP_ANALYSIS_BATCH);
        spawn_technical_analysis_tasks(app.clone(), tasks);
        Ok(())
    })();
    log::info!(
        "[PERF] resume_incomplete_analysis: technical analysis recovery took {:?}",
        step_start.elapsed()
    );

    log::info!(
        "[PERF] resume_incomplete_analysis: total time {:?}",
        start.elapsed()
    );

    if result.is_err() {
        RECOVERY_STARTED.store(false, Ordering::Release);
    }
    result
}

pub(crate) fn enqueue_technical_analysis(app: &AppHandle, assets: &[Asset]) -> Result<(), String> {
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

/// 只为本项目已有素材排队新分析；Rust 负责路径/任务，且绝不重复 active 分析。
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

pub(crate) fn drain_pending_analysis(app: &AppHandle, project_id: &str) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn keyframe_extraction_uses_fixed_sampling() {
        // 验证固定采样策略：不再依赖场景检测，改为固定时间点采样
        // 该测试验证关键帧提取不使用 scene detection filter
        assert_eq!(KEYFRAME_COUNT, 4);
    }
}
