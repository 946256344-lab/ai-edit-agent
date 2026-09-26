//! 真实硬切分段：FFmpeg 低分辨率场景检测，CLIP 验切点真伪。
//! 无硬切、CLIP 不可用或切点两侧仍相似时整条一段；禁止按秒均分。
//! 硬切确定后由 motion 模块写帧差能量，只收缩可用窗。

use crate::models::{KeyframeMetadata, SceneSegment};
use crate::process::{
    hidden_command, media_open_args, run_hidden_command_with_timeout, HiddenCommandError,
};
use crate::storyboard::semantic::cosine_similarity;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tauri::AppHandle;

/// 已验证硬切分段；低于此版本的就绪视频需要重切。
pub(crate) const HARD_CUT_ANALYSIS_VERSION: u32 = 3;
/// 硬切片段上已写入运动能量可用窗。
pub(crate) const CURRENT_ANALYSIS_VERSION: u32 = 4;
pub(crate) const SCENE_DETECT_BUDGET: Duration = Duration::from_secs(60);
const SCENE_THRESHOLD: &str = "0.30";
/// 闪光等碎段才合并；不把真硬切为凑数量合掉。
const MIN_SEGMENT_MS: i64 = 400;
/// 全帧补扫上限：超过 1 分钟即使本机也不整段解码。
const FULL_FRAME_MAX_MS: i64 = 60_000;
const KEYFRAME_PASS_BUDGET: Duration = Duration::from_secs(20);
const FULL_DECODE_MIN_REMAINING: Duration = Duration::from_secs(15);
const FRAME_FFMPEG_TIMEOUT: Duration = Duration::from_secs(20);
/// 一次 FFmpeg 最多抽的帧数：每帧一个快速定位的输入，过多会拉长命令行和单进程内存。
const FRAMES_PER_FFMPEG: usize = 12;
const MAX_KEYFRAMES_FOR_GRID: usize = 8;
/// 切点核实帧与清晰度评分用的宽度。
const QUALITY_FRAME_WIDTH: u32 = 320;
/// 段内样本帧宽度：整段识别要放大其中一帧看细节，按大图尺寸抽。
const SAMPLE_FRAME_WIDTH: u32 = 960;
/// 每段抽样帧数：清晰度评分与整段画面识别共用（识别图为 1 帧大图 + 4 帧小图）。
pub(crate) const SEGMENT_SAMPLE_FRAMES: usize = 5;
const CUT_PROBE_OFFSET_MS: i64 = 250;
/// 与 Phase 2 去似同一阈值：切点两侧仍像同一画面则丢掉该切。
const CUT_VERIFY_SIMILAR_COSINE: f64 = 0.92;

/// 检测场景切点（毫秒）。超时或失败返回空列表，由调用方整条一段。
pub(crate) fn detect_scene_cuts(source: &Path, duration_ms: i64, budget: Duration) -> Vec<i64> {
    if duration_ms <= 0 {
        return Vec::new();
    }
    let remote = is_remote_source(source);
    let may_refine = should_consider_full_frame(duration_ms, remote);
    let keyframe_budget = if may_refine {
        budget.min(KEYFRAME_PASS_BUDGET)
    } else {
        budget
    };
    let started = Instant::now();
    let mut cuts = cuts_or_empty(run_scene_detect(source, keyframe_budget, true));
    let remaining = budget.saturating_sub(started.elapsed());
    if should_full_frame_refine(duration_ms, cuts.len(), remaining, remote) {
        log::info!(
            "Scene detect full-frame refine for {}ms local clip with {} keyframe cut(s)",
            duration_ms,
            cuts.len()
        );
        if let SceneDetectResult::Done(extra) = run_scene_detect(source, remaining, false) {
            cuts.extend(extra);
            cuts.sort_unstable();
            cuts.dedup();
        }
    }
    cuts.into_iter()
        .filter(|cut| *cut > 0 && *cut < duration_ms)
        .collect()
}

/// 计时日志只写文件名，不写完整本地路径。
fn file_label(source: &Path) -> String {
    source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_remote_source(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    let stripped = raw
        .strip_prefix(r"\\?\")
        .or_else(|| raw.strip_prefix("//?/"))
        .unwrap_or(raw.as_ref());
    stripped.starts_with(r"\\")
        || stripped.starts_with("//")
        || stripped.starts_with("UNC\\")
        || stripped.starts_with("UNC/")
}

fn should_consider_full_frame(duration_ms: i64, remote: bool) -> bool {
    !remote && duration_ms > 0 && duration_ms <= FULL_FRAME_MAX_MS
}

fn should_full_frame_refine(
    duration_ms: i64,
    keyframe_cut_count: usize,
    remaining: Duration,
    remote: bool,
) -> bool {
    should_consider_full_frame(duration_ms, remote)
        && keyframe_cut_count <= 1
        && remaining > FULL_DECODE_MIN_REMAINING
}

fn cuts_or_empty(result: SceneDetectResult) -> Vec<i64> {
    match result {
        SceneDetectResult::Done(cuts) => cuts,
        SceneDetectResult::TimedOut | SceneDetectResult::Failed => Vec::new(),
    }
}

enum SceneDetectResult {
    Done(Vec<i64>),
    TimedOut,
    Failed,
}

fn run_scene_detect(
    source: &Path,
    budget: Duration,
    skip_non_keyframes: bool,
) -> SceneDetectResult {
    let filter = format!("fps=3,scale=160:-2,select='gt(scene\\,{SCENE_THRESHOLD})',showinfo");
    let mut command = hidden_command("ffmpeg");
    command.args(["-hide_banner", "-loglevel", "info"]);
    command.args(media_open_args());
    if skip_non_keyframes {
        command.args(["-skip_frame", "nokey"]);
    }
    command
        .args(["-threads", &super::analysis::analysis_ffmpeg_threads()])
        .arg("-i")
        .arg(source)
        .args(["-vf", &filter, "-an", "-f", "null", "-"]);
    let output = match run_hidden_command_with_timeout(&mut command, budget) {
        Ok(output) => output,
        Err(HiddenCommandError::TimedOut) => {
            log::warn!(
                "Scene detection timed out for {} (skip_non_keyframes={skip_non_keyframes}).",
                source.display()
            );
            return SceneDetectResult::TimedOut;
        }
        Err(HiddenCommandError::Failed) => {
            log::warn!(
                "Scene detection failed to start for {} (skip_non_keyframes={skip_non_keyframes}).",
                source.display()
            );
            return SceneDetectResult::Failed;
        }
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    SceneDetectResult::Done(parse_showinfo_pts_times(&stderr))
}

fn parse_showinfo_pts_times(stderr: &str) -> Vec<i64> {
    let mut cuts = Vec::new();
    for line in stderr.lines() {
        if !line.contains("pts_time:") {
            continue;
        }
        let Some(after) = line.split("pts_time:").nth(1) else {
            continue;
        };
        let token = after
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');
        if let Ok(seconds) = token.parse::<f64>() {
            if seconds.is_finite() && seconds > 0.0 {
                cuts.push((seconds * 1000.0).round() as i64);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

/// CLIP 余弦达到阈值则视为同一画面，丢掉该切点。
pub(crate) fn keep_dissimilar_cuts(cuts: &[i64], similarities: &[Option<f64>]) -> Vec<i64> {
    cuts.iter()
        .zip(similarities.iter())
        .filter_map(|(cut, similarity)| match similarity {
            Some(score) if *score >= CUT_VERIFY_SIMILAR_COSINE => None,
            Some(_) => Some(*cut),
            None => None,
        })
        .collect()
}

/// 由已验证硬切构建片段区间；没有切点则整条一段。
pub(crate) fn build_segment_ranges(cuts: &[i64], duration_ms: i64) -> Vec<(i64, i64)> {
    let duration_ms = duration_ms.max(0);
    if duration_ms == 0 {
        return Vec::new();
    }
    if cuts.is_empty() {
        return vec![(0, duration_ms)];
    }
    let mut boundaries = vec![0];
    boundaries.extend(cuts.iter().copied());
    if boundaries.last().copied() != Some(duration_ms) {
        boundaries.push(duration_ms);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let ranges = boundaries
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .filter(|(start, end)| *end > *start)
        .collect::<Vec<_>>();
    merge_short_segments(ranges, MIN_SEGMENT_MS)
}

fn merge_short_segments(ranges: Vec<(i64, i64)>, min_ms: i64) -> Vec<(i64, i64)> {
    if ranges.is_empty() {
        return ranges;
    }
    let mut merged = Vec::with_capacity(ranges.len());
    let mut current = ranges[0];
    for next in ranges.into_iter().skip(1) {
        if current.1 - current.0 < min_ms {
            current.1 = next.1;
        } else if next.1 - next.0 < min_ms {
            current.1 = next.1;
        } else {
            merged.push(current);
            current = next;
        }
    }
    merged.push(current);
    merged
}

fn laplacian_variance(image: &image::GrayImage) -> Option<f64> {
    let (width, height) = image.dimensions();
    if width < 3 || height < 3 {
        return None;
    }
    let mut sum = 0.0;
    let mut sum_squared = 0.0;
    let mut count = 0.0;
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let center = image.get_pixel(x, y)[0] as f64;
            let response = center * 4.0
                - image.get_pixel(x - 1, y)[0] as f64
                - image.get_pixel(x + 1, y)[0] as f64
                - image.get_pixel(x, y - 1)[0] as f64
                - image.get_pixel(x, y + 1)[0] as f64;
            sum += response;
            sum_squared += response * response;
            count += 1.0;
        }
    }
    let mean = sum / count;
    Some((sum_squared / count - mean * mean).max(0.0))
}

fn normalize_laplacian_variance(variance: f64) -> f64 {
    (variance / (variance + 500.0)).clamp(0.0, 1.0)
}

/// 清晰度按 320 宽校准（`normalize_laplacian_variance`），更大的样本帧先缩到 320 宽再算。
fn frame_quality_score(path: &Path) -> Option<f64> {
    let frame = image::open(path).ok()?;
    let frame = if frame.width() > QUALITY_FRAME_WIDTH {
        frame.resize(QUALITY_FRAME_WIDTH, u32::MAX, image::imageops::FilterType::Triangle)
    } else {
        frame
    };
    laplacian_variance(&frame.to_luma8()).map(normalize_laplacian_variance)
}

fn extract_frame(source: &Path, time_ms: i64, destination: &Path, width: u32) -> bool {
    let time_seconds = (time_ms.max(0) as f64) / 1000.0;
    let mut command = hidden_command("ffmpeg");
    command
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(media_open_args())
        .args(["-threads", "1", "-ss", &format!("{time_seconds:.3}"), "-i"])
        .arg(source)
        .args(["-frames:v", "1", "-vf", &format!("scale={width}:-2")])
        .arg(destination);
    match run_hidden_command_with_timeout(&mut command, FRAME_FFMPEG_TIMEOUT) {
        Ok(_) => destination.is_file(),
        Err(HiddenCommandError::TimedOut) => {
            log::warn!(
                "Segment frame extraction timed out at {time_ms}ms for {}.",
                source.display()
            );
            false
        }
        Err(HiddenCommandError::Failed) => {
            log::warn!(
                "Segment frame extraction could not start at {time_ms}ms for {}.",
                source.display()
            );
            false
        }
    }
}

/// 同一条素材的多帧用一次 FFmpeg 抽完（每个时间点一个 `-ss` 快速定位的输入，各输出一帧），
/// 不再每帧启动一次进程。整批失败或个别缺帧时逐帧补抽，结果与逐帧抽取一致。
fn extract_frames(source: &Path, requests: &[(i64, PathBuf)], width: u32) -> Vec<bool> {
    let mut extracted = vec![false; requests.len()];
    for (chunk_index, chunk) in requests.chunks(FRAMES_PER_FFMPEG).enumerate() {
        let mut command = hidden_command("ffmpeg");
        command.args(["-y", "-hide_banner", "-loglevel", "error"]);
        for (time_ms, destination) in chunk {
            let _ = fs::remove_file(destination);
            let time_seconds = ((*time_ms).max(0) as f64) / 1000.0;
            // 每个输入只解几帧，单线程足够；按全核开线程会让 12 个解码器同时抢满 CPU 和内存。
            command
                .args(media_open_args())
                .args(["-threads", "1", "-ss", &format!("{time_seconds:.3}"), "-i"])
                .arg(source);
        }
        for (input, (_, destination)) in chunk.iter().enumerate() {
            command
                .args(["-map", &format!("{input}:v:0")])
                .args(["-frames:v", "1", "-vf", &format!("scale={width}:-2")])
                .arg(destination);
        }
        let timeout = FRAME_FFMPEG_TIMEOUT + Duration::from_secs(5 * chunk.len() as u64);
        if let Err(error) = run_hidden_command_with_timeout(&mut command, timeout) {
            log::warn!(
                "Batched frame extraction ({} frames) {} for {}; retrying one by one.",
                chunk.len(),
                match error {
                    HiddenCommandError::TimedOut => "timed out",
                    HiddenCommandError::Failed => "could not start",
                },
                file_label(source)
            );
        }
        for (offset, (_, destination)) in chunk.iter().enumerate() {
            extracted[chunk_index * FRAMES_PER_FFMPEG + offset] = destination.is_file();
        }
    }
    for ((time_ms, destination), done) in requests.iter().zip(extracted.iter_mut()) {
        if !*done {
            *done = extract_frame(source, *time_ms, destination, width);
        }
    }
    extracted
}

/// 每段固定取 5 帧：把段均分 5 份取各份中点，避开切点两侧的过渡帧；极短段去重后可少于 5 帧。
/// 这 5 帧既算清晰度，也拼成「中间帧大图 + 其余 4 帧小图」做整段画面识别。
fn sample_times_for_segment(start_ms: i64, end_ms: i64) -> Vec<i64> {
    let duration = (end_ms - start_ms).max(0);
    if duration == 0 {
        return Vec::new();
    }
    let mut times = (0..SEGMENT_SAMPLE_FRAMES as i64)
        .map(|index| start_ms + duration * (2 * index + 1) / (2 * SEGMENT_SAMPLE_FRAMES as i64))
        .collect::<Vec<_>>();
    times.dedup();
    times
}

fn verify_hard_cuts(
    app: Option<&AppHandle>,
    source: &Path,
    derived_dir: &Path,
    duration_ms: i64,
    cuts: Vec<i64>,
) -> Vec<i64> {
    if cuts.is_empty() {
        return cuts;
    }
    let Some(app) = app else {
        log::info!("Hard-cut CLIP verify skipped (no app handle); keeping no cuts.");
        return Vec::new();
    };
    let extract_started = Instant::now();
    let probes = cuts
        .iter()
        .filter_map(|cut| {
            let before_ms = (*cut - CUT_PROBE_OFFSET_MS).max(0);
            let after_ms = (*cut + CUT_PROBE_OFFSET_MS).min(duration_ms.saturating_sub(1));
            (after_ms > before_ms).then_some((*cut, before_ms, after_ms))
        })
        .collect::<Vec<_>>();
    let requests = probes
        .iter()
        .flat_map(|(cut, before_ms, after_ms)| {
            [
                (*before_ms, derived_dir.join(format!("cut_before_{cut}.jpg"))),
                (*after_ms, derived_dir.join(format!("cut_after_{cut}.jpg"))),
            ]
        })
        .collect::<Vec<_>>();
    let extracted_frames = extract_frames(source, &requests, QUALITY_FRAME_WIDTH);
    let mut pairs: Vec<(i64, Vec<u8>, Vec<u8>)> = Vec::new();
    for (index, (cut, _, _)) in probes.iter().enumerate() {
        let (_, before_path) = &requests[index * 2];
        let (_, after_path) = &requests[index * 2 + 1];
        let extracted = extracted_frames[index * 2] && extracted_frames[index * 2 + 1];
        let before_bytes = extracted.then(|| fs::read(before_path).ok()).flatten();
        let after_bytes = extracted.then(|| fs::read(after_path).ok()).flatten();
        let _ = fs::remove_file(before_path);
        let _ = fs::remove_file(after_path);
        match (before_bytes, after_bytes) {
            (Some(before), Some(after)) if !before.is_empty() && !after.is_empty() => {
                pairs.push((*cut, before, after));
            }
            _ => {
                log::info!("Dropped FFmpeg cut at {cut}ms: probe frames unavailable.");
            }
        }
    }
    if pairs.is_empty() {
        return Vec::new();
    }
    let mut image_refs: Vec<&[u8]> = Vec::with_capacity(pairs.len() * 2);
    for (_, before, after) in &pairs {
        image_refs.push(before);
        image_refs.push(after);
    }
    let extract_ms = extract_started.elapsed().as_millis();
    let clip_started = Instant::now();
    let clip_result = crate::storyboard::clip::encode_image_bytes(app, &image_refs);
    log::info!(
        "[PERF] cut verify file={} probes={} extract={}ms clip={}ms",
        file_label(source),
        pairs.len(),
        extract_ms,
        clip_started.elapsed().as_millis()
    );
    let embeddings = match clip_result {
        Ok(embeddings) => embeddings,
        Err(error) => {
            log::warn!("Hard-cut CLIP verify failed ({error}); keeping no cuts.");
            return Vec::new();
        }
    };
    if embeddings.len() != pairs.len() * 2 {
        log::warn!("Hard-cut CLIP verify dimension mismatch; keeping no cuts.");
        return Vec::new();
    }
    let similarities = (0..pairs.len())
        .map(|index| cosine_similarity(&embeddings[index * 2], &embeddings[index * 2 + 1]))
        .collect::<Vec<_>>();
    let cuts: Vec<i64> = pairs.iter().map(|(cut, _, _)| *cut).collect();
    let kept = keep_dissimilar_cuts(&cuts, &similarities);
    for (cut, similarity) in cuts.iter().zip(similarities.iter()) {
        if !kept.contains(cut) {
            log::info!("Dropped FFmpeg cut at {cut}ms: CLIP similarity={similarity:?}.");
        }
    }
    kept
}

/// 检测场景、抽帧、写清晰度，返回完整片段列表与用于网格的中点关键帧（上限 8）。
pub(crate) fn analyze_video_segments(
    app: Option<&AppHandle>,
    source: &Path,
    derived_dir: &Path,
    duration_ms: Option<i64>,
) -> Result<(Vec<KeyframeMetadata>, Vec<SceneSegment>), String> {
    let duration_ms = duration_ms.unwrap_or(0).max(0);
    let detect_started = Instant::now();
    let raw_cuts = detect_scene_cuts(source, duration_ms, SCENE_DETECT_BUDGET);
    let detect_ms = detect_started.elapsed().as_millis();
    let raw_cut_count = raw_cuts.len();
    let verify_started = Instant::now();
    let cuts = verify_hard_cuts(app, source, derived_dir, duration_ms, raw_cuts);
    let verify_ms = verify_started.elapsed().as_millis();
    let kept_cut_count = cuts.len();
    let ranges = build_segment_ranges(&cuts, duration_ms);
    let ranges = if ranges.is_empty() && duration_ms > 0 {
        vec![(0, duration_ms)]
    } else {
        ranges
    };

    let samples_started = Instant::now();
    let plans = ranges
        .into_iter()
        .enumerate()
        .map(|(index, (start_ms, end_ms))| {
            let segment_id = format!("s{:03}", index + 1);
            let requests = sample_times_for_segment(start_ms, end_ms)
                .into_iter()
                .enumerate()
                .map(|(frame_index, time_ms)| {
                    let destination =
                        derived_dir.join(format!("seg_{segment_id}_{:02}.jpg", frame_index + 1));
                    (time_ms, destination)
                })
                .collect::<Vec<_>>();
            (segment_id, start_ms, end_ms, requests)
        })
        .collect::<Vec<_>>();
    let all_requests = plans
        .iter()
        .flat_map(|(_, _, _, requests)| requests.iter().cloned())
        .collect::<Vec<_>>();
    let sampled_frames = all_requests.len();
    let mut extracted = extract_frames(source, &all_requests, SAMPLE_FRAME_WIDTH).into_iter();
    let mut segments = Vec::with_capacity(plans.len());
    let mut midpoint_keyframes = Vec::new();
    for (segment_id, start_ms, end_ms, requests) in plans {
        let mut frames = Vec::with_capacity(requests.len());
        let mut quality_scores = Vec::new();
        for (time_ms, destination) in &requests {
            if extracted.next().unwrap_or(false) {
                if let Some(score) = frame_quality_score(&destination) {
                    quality_scores.push(score);
                }
                frames.push(KeyframeMetadata {
                    time_ms: *time_ms,
                    image_path: destination.to_string_lossy().into_owned(),
                });
            }
        }
        let visual_quality_score = if quality_scores.is_empty() {
            None
        } else {
            quality_scores.sort_by(f64::total_cmp);
            let middle = quality_scores.len() / 2;
            Some(if quality_scores.len() % 2 == 0 {
                (quality_scores[middle - 1] + quality_scores[middle]) / 2.0
            } else {
                quality_scores[middle]
            })
        };
        if let Some(mid_frame) = frames.get(frames.len() / 2).cloned() {
            midpoint_keyframes.push(mid_frame);
        }
        segments.push(SceneSegment {
            id: segment_id,
            start_ms,
            end_ms,
            scene_duration_ms: Some(end_ms - start_ms),
            visual_quality_score,
            frames,
            visual_evidence: None,
            motion_score: None,
            motion_profile: None,
        });
    }

    let samples_ms = samples_started.elapsed().as_millis();
    let motion_started = Instant::now();
    super::motion::attach_motion_profiles(source, &mut segments);
    log::info!(
        "[PERF] segments file={} detect={}ms verify={}ms (cuts {}->{}) samples={}ms ({} frames, {} segments) motion={}ms",
        file_label(source),
        detect_ms,
        verify_ms,
        raw_cut_count,
        kept_cut_count,
        samples_ms,
        sampled_frames,
        segments.len(),
        motion_started.elapsed().as_millis()
    );
    midpoint_keyframes.truncate(MAX_KEYFRAMES_FOR_GRID);
    Ok((midpoint_keyframes, segments))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_showinfo_extracts_pts_times() {
        let stderr = r#"
[Parsed_showinfo_2 @ 0x0] n:  0 pts:      0 pts_time:1.000 pos:123
[Parsed_showinfo_2 @ 0x0] n:  1 pts:   3000 pts_time:4.333 pos:456
"#;
        assert_eq!(parse_showinfo_pts_times(stderr), vec![1000, 4333]);
    }

    #[test]
    fn build_segments_keeps_whole_clip_when_no_cuts() {
        let ranges = build_segment_ranges(&[], 24_000);
        assert_eq!(ranges, vec![(0, 24_000)]);
    }

    #[test]
    fn build_segments_keeps_verified_cuts_without_count_cap() {
        let cuts: Vec<i64> = (1..40).map(|i| i * 2_000).collect();
        let ranges = build_segment_ranges(&cuts, 80_000);
        assert!(ranges.len() > 24);
        assert_eq!(ranges.first().map(|r| r.0), Some(0));
        assert_eq!(ranges.last().map(|r| r.1), Some(80_000));
        assert!(ranges.iter().all(|(start, end)| *end > *start));
    }

    #[test]
    fn keep_dissimilar_cuts_drops_similar_and_unverified() {
        let cuts = vec![1_000, 5_000, 9_000];
        let kept = keep_dissimilar_cuts(&cuts, &[Some(0.99), Some(0.40), None]);
        assert_eq!(kept, vec![5_000]);
    }

    #[test]
    fn sample_times_take_five_centered_frames_per_segment() {
        assert_eq!(
            sample_times_for_segment(0, 6_000),
            vec![600, 1_800, 3_000, 4_200, 5_400]
        );
        let long = sample_times_for_segment(10_000, 130_000);
        assert_eq!(long.len(), SEGMENT_SAMPLE_FRAMES);
        assert_eq!(long.first(), Some(&22_000));
        assert_eq!(long.last(), Some(&118_000));
        assert_eq!(sample_times_for_segment(0, 3), vec![0, 1, 2]);
    }

    #[test]
    fn remote_unc_paths_are_detected() {
        assert!(is_remote_source(Path::new(
            r"\\Shared-huiquan\share\DJI_0125.MP4"
        )));
        assert!(is_remote_source(Path::new(r"\\?\UNC\server\share\a.mp4")));
        assert!(is_remote_source(Path::new("//server/share/a.mp4")));
        assert!(!is_remote_source(Path::new(r"C:\media\DJI_0125.MP4")));
        assert!(!is_remote_source(Path::new(r"D:\自动剪辑系统\clip.mp4")));
    }

    #[test]
    fn full_frame_refine_only_for_short_local_sparse_cuts() {
        let enough = Duration::from_secs(20);
        assert!(should_full_frame_refine(12_000, 0, enough, false));
        assert!(should_full_frame_refine(60_000, 1, enough, false));
        assert!(!should_full_frame_refine(12_000, 2, enough, false));
        assert!(!should_full_frame_refine(60_001, 0, enough, false));
        assert!(!should_full_frame_refine(12_000, 0, enough, true));
        assert!(!should_full_frame_refine(
            12_000,
            0,
            Duration::from_secs(10),
            false
        ));
    }
}
