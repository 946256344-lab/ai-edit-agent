//! 真实硬切分段：FFmpeg 低分辨率场景检测，CLIP 验切点真伪。
//! 无硬切、CLIP 不可用或切点两侧仍相似时整条一段；禁止按秒均分。
//! 硬切确定后由 motion 模块写帧差能量，只收缩可用窗。

use crate::models::{KeyframeMetadata, SceneSegment};
use crate::process::{hidden_command, run_hidden_command_with_timeout, HiddenCommandError};
use crate::storyboard::semantic::cosine_similarity;
use std::{fs, path::Path, time::Duration};
use tauri::AppHandle;

/// 已验证硬切分段；低于此版本的就绪视频需要重切。
pub(crate) const HARD_CUT_ANALYSIS_VERSION: u32 = 3;
/// 硬切片段上已写入运动能量可用窗。
pub(crate) const CURRENT_ANALYSIS_VERSION: u32 = 4;
pub(crate) const SCENE_DETECT_BUDGET: Duration = Duration::from_secs(60);
const SCENE_THRESHOLD: &str = "0.30";
/// 闪光等碎段才合并；不把真硬切为凑数量合掉。
const MIN_SEGMENT_MS: i64 = 400;
const LONG_VIDEO_MS: i64 = 180_000;
const FRAME_FFMPEG_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_KEYFRAMES_FOR_GRID: usize = 8;
const MAX_SAMPLE_FRAMES: usize = 8;
const SAMPLE_INTERVAL_MS: i64 = 4_000;
const CUT_PROBE_OFFSET_MS: i64 = 250;
/// 与 Phase 2 去似同一阈值：切点两侧仍像同一画面则丢掉该切。
const CUT_VERIFY_SIMILAR_COSINE: f64 = 0.92;

/// 检测场景切点（毫秒）。超时或失败返回空列表，由调用方整条一段。
pub(crate) fn detect_scene_cuts(source: &Path, duration_ms: i64, budget: Duration) -> Vec<i64> {
    if duration_ms <= 0 {
        return Vec::new();
    }
    let cuts = if duration_ms > LONG_VIDEO_MS {
        let coarse = run_scene_detect(source, budget, true);
        if !coarse.is_empty() {
            coarse
        } else {
            run_scene_detect(source, budget, false)
        }
    } else {
        run_scene_detect(source, budget, false)
    };
    let duration_ms = duration_ms.max(0);
    cuts.into_iter()
        .filter(|cut| *cut > 0 && *cut < duration_ms)
        .collect()
}

fn run_scene_detect(source: &Path, budget: Duration, skip_non_keyframes: bool) -> Vec<i64> {
    let filter = format!("fps=3,scale=160:-2,select='gt(scene\\,{SCENE_THRESHOLD})',showinfo");
    let mut command = hidden_command("ffmpeg");
    command.args(["-hide_banner", "-loglevel", "info"]);
    if skip_non_keyframes {
        command.args(["-skip_frame", "nokey"]);
    }
    command
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
            return Vec::new();
        }
        Err(HiddenCommandError::Failed) => {
            log::warn!(
                "Scene detection failed to start for {} (skip_non_keyframes={skip_non_keyframes}).",
                source.display()
            );
            return Vec::new();
        }
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_showinfo_pts_times(&stderr)
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

fn frame_quality_score(path: &Path) -> Option<f64> {
    image::open(path)
        .ok()
        .and_then(|frame| laplacian_variance(&frame.to_luma8()))
        .map(normalize_laplacian_variance)
}

fn extract_frame(source: &Path, time_ms: i64, destination: &Path) -> Result<bool, String> {
    let time_seconds = (time_ms.max(0) as f64) / 1000.0;
    let mut command = hidden_command("ffmpeg");
    command
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{time_seconds:.3}"),
            "-i",
        ])
        .arg(source)
        .args(["-frames:v", "1", "-vf", "scale=320:-2"])
        .arg(destination);
    run_hidden_command_with_timeout(&mut command, FRAME_FFMPEG_TIMEOUT).map_err(
        |error| match error {
            HiddenCommandError::TimedOut => "Segment frame extraction timed out.".to_owned(),
            HiddenCommandError::Failed => "Segment frame extraction could not start.".to_owned(),
        },
    )?;
    Ok(destination.is_file())
}

fn sample_times_for_segment(start_ms: i64, end_ms: i64) -> Vec<i64> {
    let duration = (end_ms - start_ms).max(0);
    if duration == 0 {
        return Vec::new();
    }
    let pad = (duration / 8).clamp(1, 250);
    let first = start_ms + pad;
    let last = (end_ms - pad).max(first);
    let mid = start_ms + duration / 2;
    if duration <= 6_000 {
        if first == last {
            return vec![mid];
        }
        let mut times = vec![first, mid, last];
        times.sort_unstable();
        times.dedup();
        return times;
    }
    let mut times = vec![first];
    let mut cursor = first.saturating_add(SAMPLE_INTERVAL_MS);
    while cursor + 500 < last {
        times.push(cursor);
        cursor = cursor.saturating_add(SAMPLE_INTERVAL_MS);
    }
    times.push(last);
    times.sort_unstable();
    times.dedup();
    if times.len() <= MAX_SAMPLE_FRAMES {
        return times;
    }
    let last_index = times.len() - 1;
    let mut kept = vec![times[0]];
    let inner = MAX_SAMPLE_FRAMES - 2;
    for index in 1..=inner {
        let sample_index = index * last_index / (inner + 1);
        let value = times[sample_index];
        if kept.last().copied() != Some(value) {
            kept.push(value);
        }
    }
    if kept.last().copied() != Some(times[last_index]) {
        kept.push(times[last_index]);
    }
    kept
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
    let mut pairs: Vec<(i64, Vec<u8>, Vec<u8>)> = Vec::new();
    for cut in &cuts {
        let before_ms = (*cut - CUT_PROBE_OFFSET_MS).max(0);
        let after_ms = (*cut + CUT_PROBE_OFFSET_MS).min(duration_ms.saturating_sub(1));
        if after_ms <= before_ms {
            continue;
        }
        let before_path = derived_dir.join(format!("cut_before_{cut}.jpg"));
        let after_path = derived_dir.join(format!("cut_after_{cut}.jpg"));
        let extracted = extract_frame(source, before_ms, &before_path).unwrap_or(false)
            && extract_frame(source, after_ms, &after_path).unwrap_or(false);
        let before_bytes = extracted.then(|| fs::read(&before_path).ok()).flatten();
        let after_bytes = extracted.then(|| fs::read(&after_path).ok()).flatten();
        let _ = fs::remove_file(&before_path);
        let _ = fs::remove_file(&after_path);
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
    let embeddings = match crate::storyboard::clip::encode_image_bytes(app, &image_refs) {
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
    let raw_cuts = detect_scene_cuts(source, duration_ms, SCENE_DETECT_BUDGET);
    let cuts = verify_hard_cuts(app, source, derived_dir, duration_ms, raw_cuts);
    let ranges = build_segment_ranges(&cuts, duration_ms);
    let ranges = if ranges.is_empty() && duration_ms > 0 {
        vec![(0, duration_ms)]
    } else {
        ranges
    };

    let mut segments = Vec::with_capacity(ranges.len());
    let mut midpoint_keyframes = Vec::new();
    for (index, (start_ms, end_ms)) in ranges.into_iter().enumerate() {
        let segment_id = format!("s{:03}", index + 1);
        let sample_times = sample_times_for_segment(start_ms, end_ms);
        let mut frames = Vec::with_capacity(sample_times.len());
        let mut quality_scores = Vec::new();
        for (frame_index, time_ms) in sample_times.iter().enumerate() {
            let destination =
                derived_dir.join(format!("seg_{segment_id}_{:02}.jpg", frame_index + 1));
            if extract_frame(source, *time_ms, &destination)? {
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

    super::motion::attach_motion_profiles(source, &mut segments);
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
    fn sample_times_add_head_mid_tail_for_medium_segments() {
        assert_eq!(sample_times_for_segment(0, 4_000), vec![250, 2_000, 3_750]);
        let long = sample_times_for_segment(0, 8_000);
        assert_eq!(*long.first().unwrap(), 250);
        assert_eq!(*long.last().unwrap(), 7_750);
        assert!(long.len() >= 3);
    }

    #[test]
    fn sample_times_cap_long_segments() {
        let times = sample_times_for_segment(0, 120_000);
        assert!(times.len() <= MAX_SAMPLE_FRAMES);
        assert_eq!(*times.first().unwrap(), 250);
        assert_eq!(*times.last().unwrap(), 119_750);
    }
}
