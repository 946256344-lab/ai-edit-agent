//! 真实场景分段：低分辨率 FFmpeg 场景检测、均匀兜底、每片段抽帧与清晰度。
//! 纯本地、无模型调用；单素材硬预算 60s，超时或无切点时均匀切分。

use crate::models::{KeyframeMetadata, SceneSegment};
use crate::process::{hidden_command, run_hidden_command_with_timeout, HiddenCommandError};
use std::{path::Path, time::Duration};

pub(crate) const CURRENT_ANALYSIS_VERSION: u32 = 2;
pub(crate) const SCENE_DETECT_BUDGET: Duration = Duration::from_secs(60);
const SCENE_THRESHOLD: &str = "0.30";
const MIN_SEGMENT_MS: i64 = 1_500;
const MAX_SEGMENTS: usize = 24;
const LONG_VIDEO_MS: i64 = 180_000;
const FRAME_FFMPEG_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_KEYFRAMES_FOR_GRID: usize = 8;

/// 检测场景切点（毫秒）。超时或失败返回空列表，由调用方走均匀兜底。
pub(crate) fn detect_scene_cuts(source: &Path, duration_ms: i64, budget: Duration) -> Vec<i64> {
    if duration_ms <= 0 {
        return Vec::new();
    }
    let cuts = if duration_ms > LONG_VIDEO_MS {
        let coarse = run_scene_detect(source, budget, true);
        if coarse.len() >= 1 {
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

/// 由切点构建片段区间；切点不足时均匀切分。合并短段，上限 24。
pub(crate) fn build_segment_ranges(cuts: &[i64], duration_ms: i64) -> Vec<(i64, i64)> {
    let duration_ms = duration_ms.max(0);
    if duration_ms == 0 {
        return Vec::new();
    }
    let boundaries = if cuts.is_empty() {
        uniform_boundaries(duration_ms)
    } else {
        let mut boundaries = vec![0];
        boundaries.extend(cuts.iter().copied());
        if boundaries.last().copied() != Some(duration_ms) {
            boundaries.push(duration_ms);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries
    };
    let mut ranges = boundaries
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .filter(|(start, end)| *end > *start)
        .collect::<Vec<_>>();
    ranges = merge_short_segments(ranges, MIN_SEGMENT_MS);
    while ranges.len() > MAX_SEGMENTS {
        merge_shortest_adjacent(&mut ranges);
    }
    ranges
}

fn uniform_boundaries(duration_ms: i64) -> Vec<i64> {
    let segment_ms = ((duration_ms as f64) / 8.0).clamp(3_000.0, 8_000.0).round() as i64;
    let mut boundaries = vec![0];
    let mut cursor = segment_ms;
    while cursor < duration_ms - MIN_SEGMENT_MS {
        boundaries.push(cursor);
        cursor += segment_ms;
    }
    boundaries.push(duration_ms);
    boundaries
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

fn merge_shortest_adjacent(ranges: &mut Vec<(i64, i64)>) {
    if ranges.len() < 2 {
        return;
    }
    let mut best_index = 0;
    let mut best_span = i64::MAX;
    for index in 0..ranges.len() - 1 {
        let span = ranges[index + 1].1 - ranges[index].0;
        if span < best_span {
            best_span = span;
            best_index = index;
        }
    }
    let merged = (ranges[best_index].0, ranges[best_index + 1].1);
    ranges.remove(best_index + 1);
    ranges[best_index] = merged;
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
    let mid = start_ms + duration / 2;
    if duration > 6_000 {
        let q1 = start_ms + duration / 4;
        let q3 = start_ms + (duration * 3) / 4;
        vec![q1, mid, q3]
    } else {
        vec![mid]
    }
}

/// 检测场景、抽帧、写清晰度，返回完整片段列表与用于网格的中点关键帧（上限 8）。
pub(crate) fn analyze_video_segments(
    source: &Path,
    derived_dir: &Path,
    duration_ms: Option<i64>,
) -> Result<(Vec<KeyframeMetadata>, Vec<SceneSegment>), String> {
    let duration_ms = duration_ms.unwrap_or(0).max(0);
    let cuts = detect_scene_cuts(source, duration_ms, SCENE_DETECT_BUDGET);
    let ranges = if cuts.is_empty() {
        build_segment_ranges(&[], duration_ms)
    } else {
        build_segment_ranges(&cuts, duration_ms)
    };
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
        let motion_score = if frames.len() >= 2 {
            Some(
                frames
                    .windows(2)
                    .filter_map(|pair| {
                        let a = frame_quality_score(Path::new(&pair[0].image_path))?;
                        let b = frame_quality_score(Path::new(&pair[1].image_path))?;
                        Some((a - b).abs())
                    })
                    .sum::<f64>()
                    / (frames.len() - 1) as f64,
            )
        } else {
            None
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
            motion_score,
        });
    }

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
    fn build_segments_falls_back_to_uniform_when_no_cuts() {
        let ranges = build_segment_ranges(&[], 24_000);
        assert!(ranges.len() >= 3);
        assert_eq!(ranges.first().map(|r| r.0), Some(0));
        assert_eq!(ranges.last().map(|r| r.1), Some(24_000));
        assert!(ranges
            .iter()
            .all(|(start, end)| end - start >= MIN_SEGMENT_MS || *end == 24_000));
    }

    #[test]
    fn build_segments_merges_short_cuts_and_caps_count() {
        let cuts: Vec<i64> = (1..40).map(|i| i * 500).collect();
        let ranges = build_segment_ranges(&cuts, 20_000);
        assert!(ranges.len() <= MAX_SEGMENTS);
        assert!(ranges.iter().all(|(start, end)| *end > *start));
    }

    #[test]
    fn sample_times_add_quartiles_for_long_segments() {
        assert_eq!(sample_times_for_segment(0, 4_000), vec![2_000]);
        assert_eq!(
            sample_times_for_segment(0, 8_000),
            vec![2_000, 4_000, 6_000]
        );
    }
}
