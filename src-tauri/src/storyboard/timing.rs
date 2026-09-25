//! 用配音真实时间戳或 key_message 屏幕标记可读性安排 beat 时长与切点。
//! 缺失或文案不一致时不制造精确对齐事实。
use super::{multimodal::Phase4ContentWindow, repair::StoryboardIssue};
use crate::models::{StoryboardBeat, StoryboardContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Voice：TTS alignment 时段；Pacing：key_message 屏幕标记可读性节奏。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SpeechTimingKind {
    #[default]
    Voice,
    Pacing,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpeechTiming {
    #[serde(default)]
    pub kind: SpeechTimingKind,
    pub beats: Vec<BeatTiming>,
    pub pauses_ms: Vec<i64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BeatTiming {
    pub beat_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// key_message 屏幕标记可读性下限：最短停留 + 每字最低时长 + 动画余量。
pub(crate) fn marker_readability_floor_ms(marker: &str) -> i64 {
    let chars = marker
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .count()
        .max(1) as i64;
    chars.saturating_mul(125).max(1_500).saturating_add(340)
}

fn spoken(text: &str) -> String {
    text.chars().filter(|ch| ch.is_alphanumeric()).collect()
}

/// 把句级片段按字数比例拆成字符时段，供拍边界落在句中时使用。
fn expand_units_to_chars(units: Vec<(String, i64, i64)>) -> Vec<(String, i64, i64)> {
    let mut expanded = Vec::new();
    for (text, start_ms, end_ms) in units {
        let chars = text.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
        if chars.len() <= 1 {
            expanded.push((text, start_ms, end_ms));
            continue;
        }
        let span = (end_ms - start_ms).max(0);
        let count = chars.len() as i64;
        for (index, ch) in chars.into_iter().enumerate() {
            let index = index as i64;
            let start = start_ms + span * index / count;
            let end = start_ms + span * (index + 1) / count;
            expanded.push((ch, start, end.max(start)));
        }
    }
    expanded
}

fn timed_text_units(alignment: &Value, key: &str) -> Option<Vec<(String, i64, i64)>> {
    let items = alignment
        .get(key)?
        .as_array()
        .filter(|items| !items.is_empty())?;
    items
        .iter()
        .map(|item| {
            Some((
                spoken(item["text"].as_str().or_else(|| item["word"].as_str())?),
                (item["start"].as_f64()? * 1000.0).round() as i64,
                (item["end"].as_f64()? * 1000.0).round() as i64,
            ))
        })
        .collect()
}

fn character_units(alignment: &Value) -> Option<Vec<(String, i64, i64)>> {
    alignment["characters"]
        .as_array()
        .filter(|items| !items.is_empty())?
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            Some((
                spoken(ch.as_str()?),
                (alignment["character_start_times_seconds"][i].as_f64()? * 1000.0).round() as i64,
                (alignment["character_end_times_seconds"][i].as_f64()? * 1000.0).round() as i64,
            ))
        })
        .collect()
}

fn alignment_unit_sources(alignment: &Value) -> Vec<Vec<(String, i64, i64)>> {
    let mut sources = Vec::new();
    if let Some(units) = timed_text_units(alignment, "words") {
        sources.push(units);
    }
    if let Some(units) = timed_text_units(alignment, "segments") {
        sources.push(expand_units_to_chars(units));
    }
    if let Some(units) = character_units(alignment) {
        sources.push(units);
    }
    sources
}

fn map_units_to_beats(
    beats: &[StoryboardBeat],
    units: Vec<(String, i64, i64)>,
    duration_ms: i64,
) -> Option<SpeechTiming> {
    let units = units
        .into_iter()
        .filter(|(text, _, _)| !text.is_empty())
        .collect::<Vec<_>>();
    let expected = beats
        .iter()
        .map(|beat| spoken(&beat.narration))
        .collect::<String>();
    if expected.is_empty()
        || expected
            != units
                .iter()
                .map(|(text, _, _)| text.as_str())
                .collect::<String>()
    {
        return None;
    }
    let mut cursor = 0;
    let mut start_ms = 0;
    let mut result = SpeechTiming {
        kind: SpeechTimingKind::Voice,
        ..SpeechTiming::default()
    };
    for (index, beat) in beats.iter().enumerate() {
        let target = spoken(&beat.narration);
        if target.is_empty() {
            return None;
        }
        let mut matched = String::new();
        while matched.len() < target.len() {
            matched.push_str(&units.get(cursor)?.0);
            cursor += 1;
        }
        if matched != target {
            return None;
        }
        let spoken_end = units[cursor - 1].2;
        let end_ms = if index + 1 == beats.len() {
            duration_ms
        } else {
            let next_start = units.get(cursor)?.1;
            if next_start < spoken_end {
                return None;
            }
            spoken_end + (next_start - spoken_end) / 2
        };
        if end_ms <= start_ms || end_ms > duration_ms {
            return None;
        }
        result.beats.push(BeatTiming {
            beat_id: beat.id.clone(),
            start_ms,
            end_ms,
        });
        start_ms = end_ms;
    }
    result.pauses_ms = units
        .windows(2)
        .filter_map(|pair| {
            (pair[1].1 - pair[0].2 >= 80).then_some(pair[0].2 + (pair[1].1 - pair[0].2) / 2)
        })
        .collect();
    Some(result)
}

pub(crate) fn from_alignment(
    beats: &[StoryboardBeat],
    alignment: &Value,
    duration_ms: i64,
) -> Option<SpeechTiming> {
    // 优先词级时间戳；没有或对不上原文时，把句级片段按字数插值成字符边界。
    alignment_unit_sources(alignment)
        .into_iter()
        .find_map(|units| map_units_to_beats(beats, units, duration_ms))
}

/// key_message：每 beat 可读性下限 + 按比例分配剩余目标时长。
pub(crate) fn from_pacing_plan(
    beats: &[StoryboardBeat],
    covered_ids: &[String],
    target_duration_ms: i64,
) -> SpeechTiming {
    let covered: Vec<&StoryboardBeat> = covered_ids
        .iter()
        .filter_map(|id| beats.iter().find(|beat| beat.id == *id))
        .collect();
    if covered.is_empty() {
        return SpeechTiming {
            kind: SpeechTimingKind::Pacing,
            ..SpeechTiming::default()
        };
    }
    let floors: Vec<i64> = covered
        .iter()
        .map(|beat| {
            let marker = if !beat.on_screen_text.trim().is_empty() {
                beat.on_screen_text.trim()
            } else {
                beat.narration.trim()
            };
            marker_readability_floor_ms(marker)
        })
        .collect();
    let floor_sum: i64 = floors.iter().sum();
    let target = target_duration_ms.max(1);
    let plan_total = if floor_sum > target {
        log::warn!(
            "key_message pacing floors sum to {floor_sum}ms > target {target}ms; using floor sum"
        );
        floor_sum
    } else {
        target
    };
    let leftover = plan_total.saturating_sub(floor_sum);
    let mut start_ms = 0_i64;
    let mut result = SpeechTiming {
        kind: SpeechTimingKind::Pacing,
        beats: Vec::with_capacity(covered.len()),
        pauses_ms: Vec::new(),
    };
    for (index, (beat, floor)) in covered.iter().zip(floors.iter()).enumerate() {
        let share = if leftover > 0 && floor_sum > 0 {
            leftover * *floor / floor_sum
        } else if leftover > 0 {
            leftover / covered.len() as i64
        } else {
            0
        };
        let mut span = floor + share;
        if index + 1 == covered.len() {
            span = (plan_total - start_ms).max(*floor);
        }
        let end_ms = start_ms + span.max(1);
        result.beats.push(BeatTiming {
            beat_id: beat.id.clone(),
            start_ms,
            end_ms,
        });
        start_ms = end_ms;
    }
    result
}

impl SpeechTiming {
    pub(crate) fn duration(&self, beat_id: &str) -> Option<i64> {
        self.beats
            .iter()
            .find(|beat| beat.beat_id == beat_id)
            .map(|beat| beat.end_ms - beat.start_ms)
    }

    pub(crate) fn validate(&self, content: &StoryboardContent) -> Result<(), String> {
        // Voice：有 uncovered 时跳过；Pacing 仍校验已计划的 covered beats。
        if self.kind == SpeechTimingKind::Voice && !content.uncovered_beat_ids.is_empty() {
            return Ok(());
        }
        for beat in &self.beats {
            let actual = content
                .shots
                .iter()
                .filter(|shot| shot.beat_id == beat.beat_id)
                .map(|shot| shot.duration_ms)
                .sum::<i64>();
            let expected = beat.end_ms - beat.start_ms;
            if actual == 0 && expected > 0 {
                let affected = content
                    .shots
                    .iter()
                    .filter(|shot| shot.beat_id == beat.beat_id)
                    .map(|shot| shot.order_index.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "beat_audio_timing: beat '{}' needs {expected}ms of picture but has no shots. Affected shot indices: {affected}.",
                    beat.beat_id
                ));
            }
            if actual > expected {
                let affected = content
                    .shots
                    .iter()
                    .filter(|shot| shot.beat_id == beat.beat_id)
                    .map(|shot| shot.order_index.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "beat_audio_timing: beat '{}' has {actual}ms of picture for a {expected}ms spoken beat. Trim source windows. Affected shot indices: {affected}.",
                    beat.beat_id
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn fit_shots(
    content: &mut StoryboardContent,
    timing: &SpeechTiming,
    windows: &HashMap<i64, (Phase4ContentWindow, bool)>,
) -> Vec<StoryboardIssue> {
    fit_shots_scoped(content, timing, windows, None)
}

pub(crate) fn fit_shots_scoped(
    content: &mut StoryboardContent,
    timing: &SpeechTiming,
    windows: &HashMap<i64, (Phase4ContentWindow, bool)>,
    mutable: Option<&HashSet<i64>>,
) -> Vec<StoryboardIssue> {
    if timing.beats.is_empty() {
        return Vec::new();
    }
    if timing.kind == SpeechTimingKind::Voice && !content.uncovered_beat_ids.is_empty() {
        return Vec::new();
    }
    let mut issues = Vec::new();
    for beat in &timing.beats {
        let indices = content
            .shots
            .iter()
            .enumerate()
            .filter_map(|(i, shot)| (shot.beat_id == beat.beat_id).then_some(i))
            .collect::<Vec<_>>();
        if indices.is_empty() {
            continue;
        }
        if let Some(allowed) = mutable {
            if !indices
                .iter()
                .all(|&i| allowed.contains(&content.shots[i].order_index))
            {
                continue;
            }
        }
        let capacities = indices
            .iter()
            .map(|&i| {
                windows
                    .get(&content.shots[i].order_index)
                    .map(|(window, _)| window.span_ms())
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>();
        let duration = beat.end_ms - beat.start_ms;
        if capacities.iter().any(|capacity| *capacity < 1) || duration < indices.len() as i64 {
            issues.push(StoryboardIssue::new(
                "beat_audio_window_shortfall",
                format!(
                    "Beat '{}' requires {}ms from planned/verified beat timing; locked usable windows are empty. Leave cuts at capacity.",
                    beat.beat_id,
                    duration,
                ),
                false,
            )
            .for_shots(
                indices
                    .iter()
                    .map(|&i| content.shots[i].order_index)
                    .collect(),
            ));
            continue;
        }
        let original_total = indices
            .iter()
            .map(|&i| content.shots[i].duration_ms.max(1))
            .sum::<i64>();
        let mut original_cursor = 0;
        let mut cursor = beat.start_ms;
        for (offset, &index) in indices.iter().enumerate() {
            let shot = &mut content.shots[index];
            original_cursor += shot.duration_ms.max(1);
            let end = if offset + 1 == indices.len() {
                beat.end_ms
            } else {
                let min = cursor + 1;
                let max = beat.end_ms - (indices.len() - offset - 1) as i64;
                let proposed =
                    (beat.start_ms + duration * original_cursor / original_total).clamp(min, max);
                timing
                    .pauses_ms
                    .iter()
                    .copied()
                    .filter(|time| *time >= min && *time <= max && (*time - proposed).abs() <= 500)
                    .min_by_key(|time| (*time - proposed).abs())
                    .unwrap_or(proposed)
            };
            let span = end - cursor;
            let (window, _) = &windows[&shot.order_index];
            let source_len = span.min(capacities[offset].max(1));
            shot.source_start_ms = shot.source_start_ms.clamp(
                window.start_ms,
                window
                    .end_ms
                    .saturating_sub(source_len)
                    .max(window.start_ms),
            );
            shot.source_end_ms = shot.source_start_ms + source_len;
            shot.duration_ms = span;
            if span > source_len {
                log::info!(
                    "Beat '{}' shot {} slowing {}ms source to {}ms on-screen ({:.2}x)",
                    beat.beat_id,
                    shot.order_index,
                    source_len,
                    span,
                    source_len as f64 / span as f64
                );
            }
            cursor = end;
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn beats() -> Vec<StoryboardBeat> {
        vec![
            StoryboardBeat {
                id: "a".into(),
                purpose: "a".into(),
                required_visual: "a".into(),
                visual_keywords: vec![],
                narration: "你好。".into(),
                on_screen_text: String::new(),
                ..Default::default()
            },
            StoryboardBeat {
                id: "b".into(),
                purpose: "b".into(),
                required_visual: "b".into(),
                visual_keywords: vec![],
                narration: "展示产品细节。".into(),
                on_screen_text: String::new(),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn segment_timestamps_map_beats_and_interpolate_mid_sentence() {
        let alignment = json!({"segments": [
            {"text":"你好。", "start":0.2, "end":1.0},
            {"text":"展示产品细节。", "start":1.4, "end":4.0}
        ]});
        let timing = from_alignment(&beats(), &alignment, 4300).unwrap();
        assert_eq!(timing.kind, SpeechTimingKind::Voice);
        assert_eq!(timing.duration("a"), Some(1200));
        assert_eq!(timing.duration("b"), Some(3100));
        assert_eq!(timing.pauses_ms, vec![1200]);
        let merged = json!({"segments":[{"text":"你好。展示产品细节。","start":0.2,"end":4.0}]});
        let interpolated = from_alignment(&beats(), &merged, 4300).unwrap();
        assert_eq!(interpolated.kind, SpeechTimingKind::Voice);
        assert!(interpolated.duration("a").unwrap() > 0);
        assert!(interpolated.duration("b").unwrap() > interpolated.duration("a").unwrap());
        assert_eq!(
            interpolated.duration("a").unwrap() + interpolated.duration("b").unwrap(),
            4300
        );
    }

    #[test]
    fn word_timestamps_are_preferred_over_merged_segments() {
        let alignment = json!({
            "words": [
                {"text":"你好", "start":0.2, "end":1.0},
                {"text":"展示产品细节", "start":1.4, "end":4.0}
            ],
            "segments":[{"text":"你好。展示产品细节。","start":0.2,"end":4.0}]
        });
        let timing = from_alignment(&beats(), &alignment, 4300).unwrap();
        assert_eq!(timing.duration("a"), Some(1200));
        assert_eq!(timing.duration("b"), Some(3100));
    }

    #[test]
    fn pacing_plan_allocates_at_least_readability_floors() {
        let beats = vec![
            StoryboardBeat {
                id: "a".into(),
                purpose: "a".into(),
                required_visual: "a".into(),
                visual_keywords: vec![],
                narration: String::new(),
                on_screen_text: "工厂".into(),
                ..Default::default()
            },
            StoryboardBeat {
                id: "b".into(),
                purpose: "b".into(),
                required_visual: "b".into(),
                visual_keywords: vec![],
                narration: String::new(),
                on_screen_text: "交付能力".into(),
                ..Default::default()
            },
        ];
        let timing = from_pacing_plan(&beats, &["a".into(), "b".into()], 12_000);
        assert_eq!(timing.kind, SpeechTimingKind::Pacing);
        assert_eq!(
            timing
                .beats
                .iter()
                .map(|b| b.end_ms - b.start_ms)
                .sum::<i64>(),
            12_000
        );
        assert!(timing.duration("a").unwrap() >= marker_readability_floor_ms("工厂"));
        assert!(timing.duration("b").unwrap() >= marker_readability_floor_ms("交付能力"));
    }

    #[test]
    fn fitting_respects_window_capacity_and_reports_shortfall() {
        let shot = |order, asset| json!({"orderIndex":order,"durationMs":1000,"purpose":"p","onScreenText":"", "assetId":asset,"sourceStartMs":1000,"sourceEndMs":2000,"reason":"r","beatId":"a"});
        let mut content: StoryboardContent = serde_json::from_value(json!({
            "title":"t","summary":"s","shots":[shot(1,"one"),shot(2,"two")]
        }))
        .unwrap();
        let windows = [1, 2]
            .into_iter()
            .map(|order| {
                (
                    order,
                    (
                        Phase4ContentWindow {
                            window_id: format!("w{order}"),
                            asset_id: String::new(),
                            start_ms: 500,
                            end_ms: 3500,
                        },
                        false,
                    ),
                )
            })
            .collect();
        let mut timing = SpeechTiming {
            kind: SpeechTimingKind::Voice,
            beats: vec![BeatTiming {
                beat_id: "a".into(),
                start_ms: 0,
                end_ms: 5000,
            }],
            pauses_ms: vec![2400],
        };
        assert!(fit_shots(&mut content, &timing, &windows).is_empty());
        assert_eq!(content.shots[0].duration_ms, 2400);
        assert_eq!(content.shots[1].duration_ms, 2600);
        assert!(content
            .shots
            .iter()
            .all(|shot| shot.source_start_ms >= 500 && shot.source_end_ms <= 3500));
        timing.beats[0].end_ms = 7000;
        let shortfall = fit_shots(&mut content, &timing, &windows);
        assert!(shortfall.is_empty());
        assert_eq!(
            content.shots.iter().map(|s| s.duration_ms).sum::<i64>(),
            7000
        );
        assert!(content
            .shots
            .iter()
            .all(|shot| shot.source_end_ms - shot.source_start_ms <= 3000));
        assert!(timing.validate(&content).is_ok());
    }

    #[test]
    fn fit_shots_scoped_does_not_resize_frozen_beats() {
        let shot = |order, asset| json!({"orderIndex":order,"durationMs":1000,"purpose":"p","onScreenText":"", "assetId":asset,"sourceStartMs":1000,"sourceEndMs":2000,"reason":"r","beatId":"a"});
        let mut content: StoryboardContent = serde_json::from_value(json!({
            "title":"t","summary":"s","shots":[shot(1,"one"),shot(2,"two")]
        }))
        .unwrap();
        content.shots[0].crop_focus = Some([0.3, 0.4]);
        let windows = [1, 2]
            .into_iter()
            .map(|order| {
                (
                    order,
                    (
                        Phase4ContentWindow {
                            window_id: format!("w{order}"),
                            asset_id: String::new(),
                            start_ms: 500,
                            end_ms: 3500,
                        },
                        false,
                    ),
                )
            })
            .collect();
        let timing = SpeechTiming {
            kind: SpeechTimingKind::Voice,
            beats: vec![BeatTiming {
                beat_id: "a".into(),
                start_ms: 0,
                end_ms: 5000,
            }],
            pauses_ms: Vec::new(),
        };
        let mutable = HashSet::from([2]);
        assert!(fit_shots_scoped(&mut content, &timing, &windows, Some(&mutable)).is_empty());
        assert_eq!(content.shots[0].duration_ms, 1000);
        assert_eq!(content.shots[0].crop_focus, Some([0.3, 0.4]));
        assert_eq!(content.shots[1].duration_ms, 1000);
    }
}
