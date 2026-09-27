//! 音频素材节拍分析：内置 FFmpeg 解码单声道 PCM，Rust 本地算速度、节拍、小节、乐句和能量曲线。
//! 不引入新依赖，算法分五步（均在本文件）：
//! 1. 起音包络：一阶低通把信号分成 <150Hz / 150–2500Hz / >2500Hz 三段，按 256 样本（约 11.6ms）
//!    求各段对数能量，正向差分（能量通量）相加，减去约 0.4 秒滑动均值后截负。
//! 2. 速度：起音包络在 60–200 BPM 滞后范围内求自相关系数，乘以 120 BPM 为中心、一个八度为标准差的
//!    对数高斯先验，取峰并抛物线插值；峰值自相关系数即置信度。
//! 3. 跟拍：Ellis (2007) 动态规划，节拍间隔偏离估计周期按 (ln 比值)² 惩罚，回溯出节拍帧；
//!    首尾起音太弱的节拍剪掉。
//! 4. 小节：按 4/4，比较四种相位上低频重音（底鼓）与总起音的平均强度，最强相位为小节起点。
//! 5. 乐句：每 4 小节一句，比较四种相位上小节能量与起音密度的变化量，变化最大的相位为乐句起点。
//! 结果写进素材 metadata.beatAnalysis；版本低于当前或缺失时按需重算。失败只影响卡点，不让素材分析失败。

use crate::db::now_millis;
use crate::models::BeatAnalysis;
use crate::process::{
    hidden_command, media_open_args, run_hidden_command_with_timeout, HiddenCommandError,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::Duration;

/// 音频分析格式版本；改算法或字段时加一，旧结果在下次用到时重算。
pub(crate) const AUDIO_ANALYSIS_VERSION: u32 = 1;
/// 低于这个置信度（起音包络在节拍周期上的自相关系数）的曲子不拿来卡点。
pub(crate) const MIN_BEAT_CONFIDENCE: f64 = 0.1;
const SAMPLE_RATE: u32 = 22_050;
const HOP: usize = 256;
/// 只分析前 6 分钟，控制内存（22050Hz 单声道 f32 约 32MB）。
const MAX_DECODE_SECONDS: u32 = 360;
const DECODE_TIMEOUT: Duration = Duration::from_secs(90);
const MIN_ANALYSIS_MS: i64 = 6_000;
const BEATS_PER_BAR: usize = 4;
const BARS_PER_PHRASE: usize = 4;
const MIN_BPM: f64 = 60.0;
const MAX_BPM: f64 = 200.0;
/// 动态规划跟拍的节拍间隔紧度（与 librosa 默认值一致）。
const TIGHTNESS: f64 = 100.0;

/// 解码为 22050Hz 单声道 f32 PCM。
pub(crate) fn decode_mono_pcm(source: &Path) -> Result<Vec<f32>, String> {
    let mut command = hidden_command("ffmpeg");
    command
        .args(["-v", "error"])
        .args(media_open_args())
        .arg("-i")
        .arg(source)
        .args(["-vn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string()])
        .args(["-t", &MAX_DECODE_SECONDS.to_string(), "-f", "f32le", "pipe:1"]);
    let output = run_hidden_command_with_timeout(&mut command, DECODE_TIMEOUT).map_err(|error| {
        match error {
            HiddenCommandError::TimedOut => "FFmpeg timed out while decoding the audio for beat analysis.",
            HiddenCommandError::Failed => "FFmpeg could not start to decode the audio for beat analysis.",
        }
        .to_owned()
    })?;
    if !output.status.success() {
        return Err("FFmpeg could not decode this audio file for beat analysis.".to_owned());
    }
    Ok(output
        .stdout
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect())
}

pub(crate) fn analyze_audio_file(source: &Path) -> Result<BeatAnalysis, String> {
    analyze_pcm(&decode_mono_pcm(source)?, SAMPLE_RATE)
}

/// 读已存的节拍分析；缺失或版本旧时现算并写回素材 metadata（条件更新，不覆盖并发写入的其他字段）。
pub(crate) fn ensure_beat_analysis(
    connection: &Connection,
    asset_id: &str,
) -> Result<BeatAnalysis, String> {
    let (source, stored): (String, Option<String>) = connection
        .query_row(
            "SELECT source_reference, json_extract(metadata_json, '$.beatAnalysis') FROM assets WHERE id = ?1 AND kind = 'audio'",
            params![asset_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The music asset is no longer in the library.".to_owned())?;
    if let Some(analysis) = stored
        .and_then(|json| serde_json::from_str::<BeatAnalysis>(&json).ok())
        .filter(|analysis| analysis.version >= AUDIO_ANALYSIS_VERSION)
    {
        return Ok(analysis);
    }
    let analysis = analyze_audio_file(Path::new(&source))?;
    connection
        .execute(
            "UPDATE assets SET metadata_json = json_set(metadata_json, '$.beatAnalysis', json(?1)), updated_at = ?2 WHERE id = ?3",
            params![
                serde_json::to_string(&analysis).map_err(|error| error.to_string())?,
                now_millis(),
                asset_id
            ],
        )
        .map_err(|error| error.to_string())?;
    log::info!(
        "Beat analysis refreshed for asset {asset_id}: {:.1} BPM, confidence {:.2}, {} beats",
        analysis.tempo_bpm,
        analysis.confidence,
        analysis.beats_ms.len()
    );
    Ok(analysis)
}

struct Features {
    rms: Vec<f32>,
    onset: Vec<f32>,
    low_flux: Vec<f32>,
}

pub(crate) fn analyze_pcm(samples: &[f32], sample_rate: u32) -> Result<BeatAnalysis, String> {
    let duration_ms = (samples.len() as f64 * 1000.0 / sample_rate as f64).round() as i64;
    if duration_ms < MIN_ANALYSIS_MS {
        return Err(format!(
            "the audio is only {duration_ms}ms long, too short to find a beat"
        ));
    }
    let frame_ms = HOP as f64 * 1000.0 / sample_rate as f64;
    let features = band_features(samples, sample_rate);
    let (period, confidence) = estimate_period(&features.onset, frame_ms)
        .ok_or_else(|| "no steady pulse was found in the audio".to_owned())?;
    let beat_frames = track_beats(&features.onset, period);
    if beat_frames.len() < BEATS_PER_BAR * 2 {
        return Err("too few beats were found in the audio".to_owned());
    }
    let to_ms = |frame: usize| ((frame as f64 + 0.5) * frame_ms).round() as i64;
    let beats_ms = beat_frames.iter().map(|&frame| to_ms(frame)).collect::<Vec<_>>();
    let mut intervals = beats_ms.windows(2).map(|pair| pair[1] - pair[0]).collect::<Vec<_>>();
    intervals.sort_unstable();
    let tempo_bpm = 60_000.0 / intervals[intervals.len() / 2].max(1) as f64;
    let phase = downbeat_phase(&beat_frames, &features);
    let downbeat_indices = (phase..beat_frames.len()).step_by(BEATS_PER_BAR).collect::<Vec<_>>();
    let bar_spans = downbeat_indices
        .iter()
        .enumerate()
        .map(|(bar, &index)| {
            let end = downbeat_indices
                .get(bar + 1)
                .map(|&next| beat_frames[next])
                .unwrap_or_else(|| (beat_frames[index] + (period * BEATS_PER_BAR as f64) as usize).min(features.rms.len()));
            (beat_frames[index], end.max(beat_frames[index] + 1))
        })
        .collect::<Vec<_>>();
    let bar_energy = bar_energies(&features.rms, &bar_spans);
    let density = bar_spans
        .iter()
        .map(|&(start, end)| mean(&features.onset[start.min(features.onset.len())..end.min(features.onset.len())]))
        .collect::<Vec<_>>();
    let phrase = phrase_phase(&bar_energy, &density);
    let downbeats_ms = downbeat_indices.iter().map(|&index| beats_ms[index]).collect::<Vec<_>>();
    let phrase_starts_ms = downbeats_ms
        .iter()
        .skip(phrase)
        .step_by(BARS_PER_PHRASE)
        .copied()
        .collect();
    Ok(BeatAnalysis {
        version: AUDIO_ANALYSIS_VERSION,
        duration_ms,
        tempo_bpm: (tempo_bpm * 10.0).round() / 10.0,
        confidence: (confidence * 100.0).round() / 100.0,
        beats_ms,
        downbeats_ms,
        phrase_starts_ms,
        bar_energy,
    })
}

fn one_pole_alpha(cutoff_hz: f64, sample_rate: u32) -> f32 {
    (1.0 - (-2.0 * std::f64::consts::PI * cutoff_hz / sample_rate as f64).exp()) as f32
}

/// 三频段对数能量通量；低频通量单独保留给小节相位用。
fn band_features(samples: &[f32], sample_rate: u32) -> Features {
    let (a_low, a_mid) = (one_pole_alpha(150.0, sample_rate), one_pole_alpha(2_500.0, sample_rate));
    let (mut low_state, mut mid_state) = (0.0_f32, 0.0_f32);
    let frames = samples.len() / HOP;
    let mut energies = Vec::with_capacity(frames);
    let mut rms = Vec::with_capacity(frames);
    for frame in samples.chunks_exact(HOP) {
        let mut bands = [0.0_f64; 3];
        let mut total = 0.0_f64;
        for &x in frame {
            low_state += a_low * (x - low_state);
            mid_state += a_mid * (x - mid_state);
            let (low, mid, high) = (low_state, mid_state - low_state, x - mid_state);
            bands[0] += (low * low) as f64;
            bands[1] += (mid * mid) as f64;
            bands[2] += (high * high) as f64;
            total += (x * x) as f64;
        }
        energies.push(bands.map(|energy| (energy / HOP as f64 + 1e-7).ln()));
        rms.push((total / HOP as f64).sqrt() as f32);
    }
    let mut raw = vec![0.0_f32; frames];
    let mut low_flux = vec![0.0_f32; frames];
    for index in 1..frames {
        let flux = |band: usize| (energies[index][band] - energies[index - 1][band]).max(0.0) as f32;
        low_flux[index] = flux(0);
        raw[index] = flux(0) + flux(1) + flux(2);
    }
    Features { rms, onset: detrend_and_normalize(&raw, 17), low_flux: normalize(&low_flux) }
}

/// 减去 ±radius 帧滑动均值、截负、按标准差归一。
fn detrend_and_normalize(values: &[f32], radius: usize) -> Vec<f32> {
    let mut prefix = vec![0.0_f64; values.len() + 1];
    for (index, value) in values.iter().enumerate() {
        prefix[index + 1] = prefix[index] + *value as f64;
    }
    let detrended = (0..values.len())
        .map(|index| {
            let (start, end) = (index.saturating_sub(radius), (index + radius + 1).min(values.len()));
            let local = (prefix[end] - prefix[start]) / (end - start) as f64;
            (values[index] as f64 - local).max(0.0) as f32
        })
        .collect::<Vec<_>>();
    normalize(&detrended)
}

fn normalize(values: &[f32]) -> Vec<f32> {
    let average = mean(values) as f64;
    let variance = values.iter().map(|value| (*value as f64 - average).powi(2)).sum::<f64>()
        / values.len().max(1) as f64;
    let deviation = variance.sqrt();
    if deviation < 1e-9 {
        return vec![0.0; values.len()];
    }
    values.iter().map(|value| (*value as f64 / deviation) as f32).collect()
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}

/// 周期（帧，含小数）与置信度（该周期上的自相关系数）。
fn estimate_period(onset: &[f32], frame_ms: f64) -> Option<(f64, f64)> {
    let min_lag = (60_000.0 / MAX_BPM / frame_ms).floor().max(2.0) as usize;
    let max_lag = (60_000.0 / MIN_BPM / frame_ms).ceil() as usize;
    if onset.len() < max_lag * 4 {
        return None;
    }
    let average = mean(onset) as f64;
    let centered = onset.iter().map(|value| *value as f64 - average).collect::<Vec<_>>();
    let variance = centered.iter().map(|value| value * value).sum::<f64>() / centered.len() as f64;
    if variance < 1e-12 {
        return None;
    }
    let correlation = |lag: usize| {
        let count = centered.len() - lag;
        (0..count).map(|index| centered[index] * centered[index + lag]).sum::<f64>()
            / count as f64
            / variance
    };
    let values = (min_lag - 1..=max_lag + 1).map(correlation).collect::<Vec<_>>();
    let at = |lag: usize| values[lag + 1 - min_lag];
    let prior = |lag: f64| {
        let bpm = 60_000.0 / (lag * frame_ms);
        (-0.5 * (bpm / 120.0).log2().powi(2)).exp()
    };
    let best = (min_lag..=max_lag)
        .max_by(|left, right| {
            (at(*left) * prior(*left as f64)).total_cmp(&(at(*right) * prior(*right as f64)))
        })?;
    let (before, peak, after) = (at(best - 1), at(best), at(best + 1));
    let curvature = before - 2.0 * peak + after;
    let offset = if curvature.abs() > 1e-12 {
        (0.5 * (before - after) / curvature).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    (peak > 0.0).then_some((best as f64 + offset, peak.clamp(0.0, 1.0)))
}

/// Ellis (2007) 动态规划跟拍，返回节拍帧序号（升序）。
fn track_beats(onset: &[f32], period: f64) -> Vec<usize> {
    let count = onset.len();
    let shortest = (period * 0.5).round().max(1.0) as usize;
    let longest = (period * 2.0).round() as usize;
    let mut score = vec![0.0_f64; count];
    let mut back = vec![usize::MAX; count];
    for index in 0..count {
        let mut best = f64::NEG_INFINITY;
        let mut arg = usize::MAX;
        if index >= shortest {
            for previous in index.saturating_sub(longest)..=index - shortest {
                let gap = (index - previous) as f64 / period;
                let candidate = score[previous] - TIGHTNESS * gap.ln().powi(2);
                if candidate > best {
                    best = candidate;
                    arg = previous;
                }
            }
        }
        if arg != usize::MAX && best > 0.0 {
            score[index] = onset[index] as f64 + best;
            back[index] = arg;
        } else {
            score[index] = onset[index] as f64;
        }
    }
    let peaks = (1..count.saturating_sub(1))
        .filter(|&index| score[index] > score[index - 1] && score[index] >= score[index + 1])
        .collect::<Vec<_>>();
    if peaks.is_empty() {
        return Vec::new();
    }
    let mut peak_scores = peaks.iter().map(|&index| score[index]).collect::<Vec<_>>();
    peak_scores.sort_by(f64::total_cmp);
    let threshold = 0.5 * peak_scores[peak_scores.len() / 2];
    let Some(&last) = peaks.iter().rev().find(|&&index| score[index] >= threshold) else {
        return Vec::new();
    };
    let mut beats = vec![last];
    while let Some(&previous) = beats.last().map(|&index| &back[index]) {
        if previous == usize::MAX {
            break;
        }
        beats.push(previous);
    }
    beats.reverse();
    let strength = |frame: usize| {
        onset[frame.saturating_sub(2)..(frame + 3).min(count)]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
    };
    let mut strengths = beats.iter().map(|&frame| strength(frame)).collect::<Vec<_>>();
    strengths.sort_by(f32::total_cmp);
    let floor = 0.3 * strengths[strengths.len() / 2];
    let first = beats.iter().position(|&frame| strength(frame) >= floor).unwrap_or(0);
    let last = beats.iter().rposition(|&frame| strength(frame) >= floor).unwrap_or(beats.len() - 1);
    beats[first..=last].to_vec()
}

/// 小节起点相位：低频重音为主、总起音为辅的平均强度最大的相位。
fn downbeat_phase(beat_frames: &[usize], features: &Features) -> usize {
    let peak = |values: &[f32], frame: usize| {
        values[frame.saturating_sub(2)..(frame + 3).min(values.len())]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
    };
    let accents = beat_frames
        .iter()
        .map(|&frame| peak(&features.low_flux, frame) + 0.5 * peak(&features.onset, frame))
        .collect::<Vec<_>>();
    (0..BEATS_PER_BAR.min(accents.len()))
        .max_by(|left, right| {
            phase_mean(&accents, *left, BEATS_PER_BAR)
                .total_cmp(&phase_mean(&accents, *right, BEATS_PER_BAR))
                .then(right.cmp(left))
        })
        .unwrap_or(0)
}

fn phase_mean(values: &[f32], phase: usize, step: usize) -> f32 {
    let picked = values.iter().skip(phase).step_by(step).copied().collect::<Vec<_>>();
    mean(&picked)
}

/// 每小节平均 RMS 的分贝值，按 10%–95% 分位映射到 0–1。
fn bar_energies(rms: &[f32], spans: &[(usize, usize)]) -> Vec<f32> {
    let decibels = spans
        .iter()
        .map(|&(start, end)| {
            let slice = &rms[start.min(rms.len())..end.min(rms.len())];
            20.0 * (mean(slice).max(1e-6)).log10()
        })
        .collect::<Vec<_>>();
    let mut sorted = decibels.clone();
    sorted.sort_by(f32::total_cmp);
    if sorted.is_empty() {
        return Vec::new();
    }
    let low = sorted[(sorted.len() - 1) / 10];
    let high = sorted[((sorted.len() - 1) * 95) / 100];
    if high - low < 0.5 {
        return vec![0.5; decibels.len()];
    }
    decibels
        .iter()
        .map(|value| (((value - low) / (high - low)).clamp(0.0, 1.0) * 100.0).round() / 100.0)
        .collect()
}

/// 乐句相位：小节能量与起音密度变化量平均最大的 4 小节相位。
fn phrase_phase(energy: &[f32], density: &[f32]) -> usize {
    if energy.len() < BARS_PER_PHRASE * 2 {
        return 0;
    }
    let peak_density = density.iter().copied().fold(0.0_f32, f32::max).max(1e-6);
    let novelty = (0..energy.len())
        .map(|bar| {
            if bar == 0 {
                return 0.0;
            }
            (energy[bar] - energy[bar - 1]).abs()
                + (density[bar] - density[bar - 1]).abs() / peak_density
        })
        .collect::<Vec<_>>();
    (0..BARS_PER_PHRASE)
        .max_by(|left, right| {
            phase_mean(&novelty, *left, BARS_PER_PHRASE)
                .total_cmp(&phase_mean(&novelty, *right, BARS_PER_PHRASE))
                .then(right.cmp(left))
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 120 BPM 合成点击：每拍 1kHz 短音，小节首拍另叠 60Hz 底鼓；第 5–8 小节整体加响，
    /// 于是第 1、5、9 小节起点是能量变化处（乐句起点）。第一拍在 0.5 秒。
    fn click_track(seconds: f64) -> Vec<f32> {
        let rate = SAMPLE_RATE as f64;
        let mut samples = vec![0.0_f32; (seconds * rate) as usize];
        let mut beat = 0;
        loop {
            let start_s = 0.5 + beat as f64 * 0.5;
            if start_s + 0.1 > seconds {
                break;
            }
            let bar = beat / 4;
            let gain = if (4..8).contains(&bar) { 0.9 } else { 0.3 };
            let start = (start_s * rate) as usize;
            for offset in 0..(0.08 * rate) as usize {
                let t = offset as f64 / rate;
                let decay = (-t * 60.0).exp();
                let mut value = (2.0 * std::f64::consts::PI * 1_000.0 * t).sin() * decay * gain;
                if beat % 4 == 0 {
                    value += (2.0 * std::f64::consts::PI * 60.0 * t).sin() * (-t * 25.0).exp() * gain;
                }
                samples[start + offset] += value as f32;
            }
            beat += 1;
        }
        samples
    }

    #[test]
    fn click_track_yields_tempo_beats_downbeats_and_phrases() {
        let analysis = analyze_pcm(&click_track(24.0), SAMPLE_RATE).expect("analysis");
        assert!((analysis.tempo_bpm - 120.0).abs() <= 1.5, "tempo {}", analysis.tempo_bpm);
        assert!(analysis.confidence >= MIN_BEAT_CONFIDENCE, "confidence {}", analysis.confidence);
        assert!(analysis.beats_ms.len() >= 44, "beats {}", analysis.beats_ms.len());
        for beat in &analysis.beats_ms {
            let nearest = ((*beat as f64 - 500.0) / 500.0).round() * 500.0 + 500.0;
            assert!((*beat as f64 - nearest).abs() <= 25.0, "beat {beat} off grid");
        }
        let bar_phase = (analysis.downbeats_ms[0] - 500).rem_euclid(2_000);
        assert!(bar_phase <= 25 || bar_phase >= 1_975, "downbeats {:?}", analysis.downbeats_ms);
        assert!(analysis.downbeats_ms.windows(2).all(|pair| (pair[1] - pair[0] - 2_000).abs() <= 30));
        assert_eq!(analysis.bar_energy.len(), analysis.downbeats_ms.len());
        assert!(analysis
            .phrase_starts_ms
            .iter()
            .any(|start| (start - 8_500).abs() <= 25), "phrases {:?}", analysis.phrase_starts_ms);
        assert!(analysis.phrase_starts_ms.windows(2).all(|pair| (pair[1] - pair[0] - 8_000).abs() <= 60));
    }

    #[test]
    fn silence_and_short_audio_are_rejected() {
        assert!(analyze_pcm(&vec![0.0; SAMPLE_RATE as usize * 10], SAMPLE_RATE).is_err());
        assert!(analyze_pcm(&click_track(3.0), SAMPLE_RATE).is_err());
    }
}
