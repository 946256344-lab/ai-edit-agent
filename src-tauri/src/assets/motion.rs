//! 硬切片段内的运动能量：低分辨率帧差曲线，只收缩可用窗，不新增切点。
//! 对比不够、样本不足或抽帧失败时保持原硬切范围。

use crate::models::{MotionEnergySample, MotionProfile, SceneSegment};
use crate::process::{
    hidden_command, media_open_args, run_hidden_command_with_timeout, HiddenCommandError,
};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const MOTION_ASSET_BUDGET: Duration = Duration::from_secs(45);
const MOTION_SEGMENT_TIMEOUT: Duration = Duration::from_secs(20);
const MOTION_WIDTH: u32 = 160;
const MOTION_HEIGHT: u32 = 90;
const FRAME_BYTES: usize = (MOTION_WIDTH * MOTION_HEIGHT) as usize;
const MAX_SAMPLES: usize = 64;
const MIN_SAMPLES: usize = 4;
const MIN_DURATION_MS: i64 = 800;
const STATIC_PEAK: f64 = 0.025;
const MIN_CONTRAST_RATIO: f64 = 0.40;
const STILL_BLEND: f64 = 0.25;
const ACTIVE_BLEND: f64 = 0.45;
const HEAD_STILL_MIN_MS: i64 = 280;
const TAIL_SETTLE_MS: i64 = 480;
const EDGE_PAD_MS: i64 = 120;
const MAX_EDGE_TRIM_RATIO: f64 = 0.35;
const MIN_KEEP_RATIO: f64 = 0.45;
const MIN_KEEP_MS: i64 = 800;

/// 已验证硬切范围与运动可用窗的交集；没有曲线时用硬切两端。
pub(crate) fn effective_window(segment: &SceneSegment) -> (i64, i64) {
    let Some(profile) = segment.motion_profile.as_ref() else {
        return (segment.start_ms, segment.end_ms);
    };
    clamp_window(
        segment.start_ms,
        segment.end_ms,
        profile.usable_start_ms,
        profile.usable_end_ms,
    )
}

pub(crate) fn profile_is_uncertain(segment: &SceneSegment) -> bool {
    segment
        .motion_profile
        .as_ref()
        .is_some_and(|profile| profile.uncertain)
}

/// 在已有硬切片段上写入运动曲线；超时则后面的片段保持原范围。
pub(crate) fn attach_motion_profiles(source: &Path, segments: &mut [SceneSegment]) {
    let deadline = Instant::now() + MOTION_ASSET_BUDGET;
    for segment in segments.iter_mut() {
        if Instant::now() >= deadline {
            log::warn!("Motion energy budget exhausted; later segments keep hard-cut bounds.");
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = remaining.min(MOTION_SEGMENT_TIMEOUT);
        match compute_profile(source, segment.start_ms, segment.end_ms, timeout) {
            Some(profile) => {
                segment.motion_score = mean_energy(&profile.energy);
                segment.motion_profile = Some(profile);
            }
            None => {
                log::info!(
                    "Motion energy skipped for {} [{}-{}].",
                    segment.id,
                    segment.start_ms,
                    segment.end_ms
                );
            }
        }
    }
}

pub(crate) fn trim_from_energy(
    start_ms: i64,
    end_ms: i64,
    samples: &[MotionEnergySample],
) -> MotionProfile {
    let span = (end_ms - start_ms).max(0);
    let fallback = MotionProfile {
        energy: samples.to_vec(),
        usable_start_ms: start_ms,
        usable_end_ms: end_ms,
        tail_settled: true,
        uncertain: samples.len() < MIN_SAMPLES,
    };
    if span < MIN_DURATION_MS || samples.len() < MIN_SAMPLES {
        return fallback;
    }
    let energies: Vec<f64> = samples.iter().map(|sample| sample.energy).collect();
    let peak = energies.iter().copied().fold(0.0, f64::max);
    let p20 = percentile(&energies, 0.20);
    if peak <= STATIC_PEAK {
        return MotionProfile {
            tail_settled: true,
            uncertain: false,
            ..fallback
        };
    }
    let contrast_ratio = (peak - p20) / peak;
    let last_mean = tail_mean(samples, end_ms);
    if contrast_ratio < MIN_CONTRAST_RATIO {
        return MotionProfile {
            tail_settled: false,
            uncertain: false,
            ..fallback
        };
    }

    let still_gate = p20 + (peak - p20) * STILL_BLEND;
    let active_gate = p20 + (peak - p20) * ACTIVE_BLEND;
    let tail_settled = last_mean <= still_gate * 1.15;
    let tail_borderline = !tail_settled && last_mean < active_gate;

    let mut usable_start = start_ms;
    let mut usable_end = end_ms;
    if let Some(rise_ms) = first_rise_after_still_head(samples, still_gate) {
        let still_head = rise_ms.saturating_sub(start_ms);
        if still_head >= HEAD_STILL_MIN_MS {
            let padded = rise_ms.saturating_sub(EDGE_PAD_MS).max(start_ms);
            let max_trim = ((span as f64) * MAX_EDGE_TRIM_RATIO) as i64;
            usable_start = padded.max(start_ms).min(start_ms + max_trim);
        }
    }
    if tail_settled {
        if let Some(last_active) = last_active_time(samples, still_gate) {
            let still_tail = end_ms.saturating_sub(last_active);
            if still_tail >= HEAD_STILL_MIN_MS {
                let padded = (last_active + EDGE_PAD_MS).min(end_ms);
                let max_trim = ((span as f64) * MAX_EDGE_TRIM_RATIO) as i64;
                usable_end = padded.min(end_ms).max(end_ms - max_trim);
            }
        }
    }

    let (mut usable_start, mut usable_end) =
        clamp_window(start_ms, end_ms, usable_start, usable_end);
    let min_keep = MIN_KEEP_MS
        .max(((span as f64) * MIN_KEEP_RATIO) as i64)
        .min(span);
    let kept = usable_end.saturating_sub(usable_start);
    if kept < min_keep {
        let deficit = min_keep - kept;
        let back = deficit / 2;
        usable_start = usable_start.saturating_sub(back).max(start_ms);
        usable_end = (usable_end + (deficit - back)).min(end_ms);
        if usable_end.saturating_sub(usable_start) < min_keep {
            if usable_start > start_ms {
                usable_start = (usable_end - min_keep).max(start_ms);
            } else {
                usable_end = (usable_start + min_keep).min(end_ms);
            }
        }
    }

    MotionProfile {
        energy: samples.to_vec(),
        usable_start_ms: usable_start,
        usable_end_ms: usable_end,
        tail_settled,
        uncertain: tail_borderline,
    }
}

fn compute_profile(
    source: &Path,
    start_ms: i64,
    end_ms: i64,
    timeout: Duration,
) -> Option<MotionProfile> {
    let samples = extract_energy_samples(source, start_ms, end_ms, timeout)?;
    Some(trim_from_energy(start_ms, end_ms, &samples))
}

fn extract_energy_samples(
    source: &Path,
    start_ms: i64,
    end_ms: i64,
    timeout: Duration,
) -> Option<Vec<MotionEnergySample>> {
    let duration_ms = (end_ms - start_ms).max(0);
    if duration_ms < MIN_DURATION_MS {
        return None;
    }
    let fps = ((MAX_SAMPLES as f64) / (duration_ms as f64 / 1000.0)).clamp(1.5, 2.5);
    let mut command = hidden_command("ffmpeg");
    command.args(["-hide_banner", "-loglevel", "error"]);
    command.args(media_open_args());
    command.args(["-threads", &super::analysis::analysis_ffmpeg_threads()]);
    command.args([
        "-ss",
        &format!("{:.3}", start_ms as f64 / 1000.0),
        "-t",
        &format!("{:.3}", duration_ms as f64 / 1000.0),
        "-i",
    ]);
    command.arg(source);
    command.args([
        "-an",
        "-vf",
        &format!(
            "fps={fps:.3},scale={MOTION_WIDTH}:{MOTION_HEIGHT}:flags=fast_bilinear,format=gray"
        ),
        "-pix_fmt",
        "gray",
        "-f",
        "rawvideo",
        "-",
    ]);
    let output = match run_hidden_command_with_timeout(&mut command, timeout) {
        Ok(output) => output,
        Err(HiddenCommandError::TimedOut) => {
            log::warn!(
                "Motion energy ffmpeg timed out for {} [{}-{}].",
                source.display(),
                start_ms,
                end_ms
            );
            return None;
        }
        Err(HiddenCommandError::Failed) => {
            log::warn!(
                "Motion energy ffmpeg failed to start for {}.",
                source.display()
            );
            return None;
        }
    };
    if output.stdout.len() < FRAME_BYTES * 2 {
        return None;
    }
    let frame_count = output.stdout.len() / FRAME_BYTES;
    if frame_count < 2 {
        return None;
    }
    let interval_ms = (duration_ms as f64 / frame_count.max(1) as f64).max(1.0);
    let mut samples = Vec::with_capacity(frame_count.saturating_sub(1));
    for index in 1..frame_count {
        let previous = &output.stdout[(index - 1) * FRAME_BYTES..index * FRAME_BYTES];
        let current = &output.stdout[index * FRAME_BYTES..(index + 1) * FRAME_BYTES];
        let time_ms = start_ms + ((index as f64) * interval_ms).round() as i64;
        samples.push(MotionEnergySample {
            time_ms: time_ms.min(end_ms.saturating_sub(1)).max(start_ms),
            energy: mean_abs_diff(previous, current),
        });
    }
    if samples.len() < MIN_SAMPLES {
        return None;
    }
    Some(samples)
}

fn mean_abs_diff(left: &[u8], right: &[u8]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let sum: u64 = left
        .iter()
        .zip(right)
        .map(|(a, b)| a.abs_diff(*b) as u64)
        .sum();
    sum as f64 / (left.len() as f64 * 255.0)
}

fn mean_energy(samples: &[MotionEnergySample]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    Some(samples.iter().map(|sample| sample.energy).sum::<f64>() / samples.len() as f64)
}

fn percentile(values: &[f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * fraction.clamp(0.0, 1.0)).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn tail_mean(samples: &[MotionEnergySample], end_ms: i64) -> f64 {
    let cutoff = end_ms.saturating_sub(TAIL_SETTLE_MS);
    let tail: Vec<f64> = samples
        .iter()
        .filter(|sample| sample.time_ms >= cutoff)
        .map(|sample| sample.energy)
        .collect();
    if tail.is_empty() {
        return samples.last().map(|sample| sample.energy).unwrap_or(0.0);
    }
    tail.iter().sum::<f64>() / tail.len() as f64
}

fn first_rise_after_still_head(samples: &[MotionEnergySample], still_gate: f64) -> Option<i64> {
    let start = samples.first()?.time_ms;
    for sample in samples {
        if sample.energy > still_gate {
            if sample.time_ms.saturating_sub(start) >= HEAD_STILL_MIN_MS {
                return Some(sample.time_ms);
            }
            return None;
        }
    }
    None
}

fn last_active_time(samples: &[MotionEnergySample], still_gate: f64) -> Option<i64> {
    samples
        .iter()
        .rev()
        .find(|sample| sample.energy > still_gate)
        .map(|sample| sample.time_ms)
}

fn clamp_window(start_ms: i64, end_ms: i64, usable_start: i64, usable_end: i64) -> (i64, i64) {
    let start = usable_start.clamp(start_ms, end_ms);
    let end = usable_end.clamp(start.saturating_add(1), end_ms);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(time_ms: i64, energy: f64) -> MotionEnergySample {
        MotionEnergySample { time_ms, energy }
    }

    fn bell_curve() -> Vec<MotionEnergySample> {
        let mut samples = Vec::new();
        for index in 0..50 {
            let time_ms = index * 200;
            let energy = if time_ms < 2_000 || time_ms > 7_000 {
                0.01
            } else {
                let phase = (time_ms - 2_000) as f64 / 5_000.0;
                0.01 + 0.22 * (std::f64::consts::PI * phase).sin()
            };
            samples.push(sample(time_ms, energy));
        }
        samples
    }

    #[test]
    fn trim_drops_still_head_and_settled_tail() {
        let profile = trim_from_energy(0, 10_000, &bell_curve());
        assert!(profile.tail_settled);
        assert!(!profile.uncertain);
        assert!(profile.usable_start_ms > 1_200);
        assert!(profile.usable_start_ms < 2_400);
        assert!(profile.usable_end_ms < 8_800);
        assert!(profile.usable_end_ms > 6_400);
    }

    #[test]
    fn unconverged_tail_keeps_hard_cut_end() {
        let mut samples = bell_curve();
        for sample in samples.iter_mut().filter(|item| item.time_ms >= 8_000) {
            sample.energy = 0.18;
        }
        let profile = trim_from_energy(0, 10_000, &samples);
        assert!(!profile.tail_settled);
        assert_eq!(profile.usable_end_ms, 10_000);
    }

    #[test]
    fn flat_low_energy_does_not_trim() {
        let samples: Vec<_> = (0..20).map(|index| sample(index * 200, 0.008)).collect();
        let profile = trim_from_energy(0, 4_000, &samples);
        assert!(profile.tail_settled);
        assert_eq!(profile.usable_start_ms, 0);
        assert_eq!(profile.usable_end_ms, 4_000);
    }

    #[test]
    fn flat_high_energy_does_not_invent_cuts() {
        let samples: Vec<_> = (0..20).map(|index| sample(index * 200, 0.16)).collect();
        let profile = trim_from_energy(0, 4_000, &samples);
        assert!(!profile.tail_settled);
        assert_eq!(profile.usable_start_ms, 0);
        assert_eq!(profile.usable_end_ms, 4_000);
    }

    #[test]
    fn too_few_samples_keep_full_range() {
        let samples = vec![sample(0, 0.2), sample(500, 0.01)];
        let profile = trim_from_energy(0, 4_000, &samples);
        assert!(profile.uncertain);
        assert_eq!((profile.usable_start_ms, profile.usable_end_ms), (0, 4_000));
    }

    #[test]
    fn effective_window_falls_back_without_profile() {
        let segment = SceneSegment {
            id: "s001".to_owned(),
            start_ms: 1_000,
            end_ms: 5_000,
            scene_duration_ms: Some(4_000),
            visual_quality_score: None,
            frames: Vec::new(),
            visual_evidence: None,
            motion_score: None,
            motion_profile: None,
        };
        assert_eq!(effective_window(&segment), (1_000, 5_000));
    }
}
