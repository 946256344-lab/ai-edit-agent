//! 口播时钟对照源窗：短了就放慢镜头，不换片、不补镜、不拼下一段硬切。
//! 没有配音时钟时，每镜时长跟精修出的内容区间走，源区间与槽位保持等长。

use super::multimodal::Phase4ContentWindow;
use super::phases::{BeatCandidatePool, RoughStoryboard, ShotLengthHint};
use super::repair::StoryboardIssue;
use super::timing::{self, SpeechTiming, SpeechTimingKind};
use crate::models::{StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const TOP_ALTERNATE_INDEX: usize = 4;

pub(crate) fn usable_ms(source: &StoryboardSource) -> i64 {
    source
        .segment
        .as_ref()
        .map(|segment| segment.span_ms())
        .or(source.duration_ms)
        .unwrap_or(0)
        .max(0)
}

pub(crate) fn source_span_ms(shot: &StoryboardShot) -> i64 {
    (shot.source_end_ms - shot.source_start_ms).max(0)
}

/// 改切点时保住成片时长。口播可以长于源窗，由预览/剪映放慢，不要把 `durationMs` 写回源跨度。
pub(crate) fn set_source_range(shot: &mut StoryboardShot, start: i64, end: i64) {
    let on_screen = shot.duration_ms.max(1);
    shot.source_start_ms = start;
    shot.source_end_ms = end.max(start + 1);
    shot.duration_ms = on_screen;
}

/// 每拍成片时长对齐口播/节奏。源窗不够长时保留源窗并放慢，够长则按 1 倍速裁到口播时长。
pub(crate) fn stretch_shots_to_speech_timing(
    content: &mut StoryboardContent,
    timing: &SpeechTiming,
) {
    // Content 的拍时段只是提示：这里裁短会把整片候选的锁定窗一起裁掉。
    if timing.beats.is_empty() || timing.kind == SpeechTimingKind::Content {
        return;
    }
    for beat in &timing.beats {
        let needed = beat.end_ms.saturating_sub(beat.start_ms);
        if needed < 1 {
            continue;
        }
        let indices = content
            .shots
            .iter()
            .enumerate()
            .filter(|(_, shot)| shot.beat_id == beat.beat_id)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if indices.is_empty() {
            continue;
        }
        let source_total = indices
            .iter()
            .map(|&index| source_span_ms(&content.shots[index]).max(1))
            .sum::<i64>()
            .max(1);
        let mut remaining = needed;
        for (offset, &index) in indices.iter().enumerate() {
            let span = source_span_ms(&content.shots[index]).max(1);
            let share = if offset + 1 == indices.len() {
                remaining.max(1)
            } else {
                let proportional = ((needed as i128 * span as i128) / source_total as i128) as i64;
                proportional
                    .max(1)
                    .min(remaining.saturating_sub((indices.len() - offset - 1) as i64))
            };
            let shot = &mut content.shots[index];
            if share > span {
                log::info!(
                    "Beat '{}' shot {} slowing {}ms source to {}ms on-screen ({:.2}x)",
                    beat.beat_id,
                    shot.order_index,
                    span,
                    share,
                    span as f64 / share as f64
                );
            } else if share < span {
                shot.source_end_ms = shot.source_start_ms + share;
            }
            shot.duration_ms = share;
            remaining = remaining.saturating_sub(share);
        }
    }
}

/// 无配音时钟时单镜时长上下界（毫秒）：约 1.5–5 秒，用户要快切或长镜时随 shotLengthHint 调整。
pub(crate) fn content_shot_bounds(hint: ShotLengthHint) -> (i64, i64) {
    match hint {
        ShotLengthHint::Short => (1_000, 2_500),
        ShotLengthHint::Default => (1_500, 5_000),
        ShotLengthHint::Long => (2_000, 8_000),
    }
}

const SLOWED_TAG: &str = " [slowed ";

/// 无配音时钟：每镜时长跟 Phase 4 精修出的内容区间走，在上下界内按比例缩放到目标总长。
/// 源区间始终与槽位等长（1 倍速）：长了按证据最佳区间裁，短了在锁定窗内向两侧补；
/// 锁定窗本身不够长才放慢，并在 reason 里写明倍速，不静默截断。
pub(crate) fn fit_shots_to_content(
    content: &mut StoryboardContent,
    windows: &HashMap<i64, (Phase4ContentWindow, bool)>,
    sources: &[StoryboardSource],
    bounds: (i64, i64),
    mutable: Option<&HashSet<i64>>,
) {
    let (min_ms, max_ms) = bounds;
    let mut plans = Vec::new();
    let mut frozen_ms = 0_i64;
    for (index, shot) in content.shots.iter().enumerate() {
        let adjustable = mutable.map_or(true, |allowed| allowed.contains(&shot.order_index));
        let window = windows
            .get(&shot.order_index)
            .map(|(window, _)| window)
            .filter(|window| window.asset_id == shot.asset_id);
        let (Some(window), true) = (window, adjustable) else {
            frozen_ms += shot.duration_ms.max(0);
            continue;
        };
        let is_image = sources
            .iter()
            .find(|source| source.asset_id == shot.asset_id)
            .is_some_and(|source| source.kind == "image");
        let marker = shot.on_screen_text.trim();
        let floor = if shot.beat_part_index <= 1 && !marker.is_empty() {
            min_ms.max(timing::marker_readability_floor_ms(marker))
        } else {
            min_ms
        };
        let (refined, capacity, preferred) = if is_image {
            ((0, 0), i64::MAX / 4, (min_ms + max_ms) / 2)
        } else {
            let start = shot.source_start_ms.clamp(window.start_ms, window.end_ms);
            let end = shot.source_end_ms.clamp(start, window.end_ms);
            ((start, end), window.span_ms().max(1), (end - start).max(1))
        };
        plans.push(ContentPlan {
            index,
            window: (window.start_ms, window.end_ms),
            refined,
            capacity,
            floor,
            preferred,
            is_image,
        });
    }
    if plans.is_empty() {
        return;
    }
    let target = content
        .target_duration_ms
        .saturating_sub(frozen_ms)
        .max(plans.len() as i64);
    let lo = plans.iter().map(|plan| plan.floor).collect::<Vec<_>>();
    // 先按 1.5–5 秒上限分；素材窗够长但总长不够时放开 5 秒上限（仍 1 倍速）；
    // 窗口本身都不够长时才按比例放慢补足目标时长。
    let hi_capped = plans
        .iter()
        .map(|plan| plan.floor.max(max_ms.min(plan.capacity)))
        .collect::<Vec<_>>();
    let hi = if hi_capped.iter().sum::<i64>() >= target {
        hi_capped
    } else {
        plans
            .iter()
            .map(|plan| plan.floor.max(plan.capacity.min(target)))
            .collect::<Vec<_>>()
    };
    let preferred = plans
        .iter()
        .zip(lo.iter().zip(hi.iter()))
        .map(|(plan, (lo, hi))| plan.preferred.clamp(*lo, *hi))
        .collect::<Vec<_>>();
    let mut slots = allocate_between(&lo, &preferred, &hi, target);
    let filled = slots.iter().sum::<i64>();
    if filled < target {
        log::warn!(
            "Content-driven pacing: usable windows total {filled}ms for a {target}ms target; slowing shots to fill the gap"
        );
        let unbounded = vec![target; slots.len()];
        slots = allocate_between(&slots, &slots, &unbounded, target);
    }
    for (plan, slot) in plans.iter().zip(slots) {
        let shot = &mut content.shots[plan.index];
        if let Some(position) = shot.reason.find(SLOWED_TAG) {
            shot.reason.truncate(position);
        }
        shot.duration_ms = slot;
        if plan.is_image {
            continue;
        }
        let anchor = evidence_anchor_ms(sources, shot, plan.refined);
        let (start, end) = place_source_range(plan.refined, plan.window, slot, anchor);
        if plan.refined.1 - plan.refined.0 > slot {
            log::info!(
                "Shot {} refined range [{}-{}] trimmed to [{start}-{end}] for a {slot}ms slot",
                shot.order_index,
                plan.refined.0,
                plan.refined.1
            );
        }
        shot.source_start_ms = start;
        shot.source_end_ms = end.max(start + 1);
        let span = shot.source_end_ms - shot.source_start_ms;
        if span < slot {
            let speed = span as f64 / slot as f64;
            log::info!(
                "Shot {} slowing {span}ms source to {slot}ms on-screen ({speed:.2}x)",
                shot.order_index
            );
            shot.reason.push_str(&format!(
                "{SLOWED_TAG}{speed:.2}x: usable window {span}ms shorter than {slot}ms slot]"
            ));
        }
    }
}

/// 源区间长于槽位时按证据最佳区间裁到槽位长度，保证成片播放的就是故事版写下的区间。
/// 预览、剪映和 FCPXML/OTIO 都按「源区间 ÷ 槽位」变速，长区间不裁会被加速。
pub(crate) fn trim_sources_to_slots(
    content: &mut StoryboardContent,
    sources: &[StoryboardSource],
    mutable: Option<&HashSet<i64>>,
) {
    for index in 0..content.shots.len() {
        let shot = &content.shots[index];
        if mutable.is_some_and(|allowed| !allowed.contains(&shot.order_index)) {
            continue;
        }
        let refined = (shot.source_start_ms, shot.source_end_ms);
        let slot = shot.duration_ms.max(1);
        if refined.1 - refined.0 <= slot {
            continue;
        }
        let anchor = evidence_anchor_ms(sources, shot, refined);
        let (start, end) = place_source_range(refined, refined, slot, anchor);
        let shot = &mut content.shots[index];
        shot.source_start_ms = start;
        shot.source_end_ms = end;
    }
}

struct ContentPlan {
    index: usize,
    window: (i64, i64),
    refined: (i64, i64),
    capacity: i64,
    floor: i64,
    preferred: i64,
    is_image: bool,
}

/// 目标总长落在 lo..preferred 或 preferred..hi 之间时按各镜余量线性分配，保留镜头间的长短差异。
fn allocate_between(lo: &[i64], preferred: &[i64], hi: &[i64], target: i64) -> Vec<i64> {
    let sum = |values: &[i64]| values.iter().sum::<i64>();
    let (sum_lo, sum_pref, sum_hi) = (sum(lo), sum(preferred), sum(hi));
    let mut slots = if target <= sum_pref {
        let room = sum_pref - sum_lo;
        let take = (sum_pref - target).min(room);
        preferred
            .iter()
            .zip(lo)
            .map(|(pref, lo)| {
                if room <= 0 {
                    *lo
                } else {
                    pref - ((pref - lo) as i128 * take as i128 / room as i128) as i64
                }
            })
            .collect::<Vec<_>>()
    } else {
        let room = sum_hi - sum_pref;
        let add = (target - sum_pref).min(room);
        preferred
            .iter()
            .zip(hi)
            .map(|(pref, hi)| {
                if room <= 0 {
                    *pref
                } else {
                    pref + ((hi - pref) as i128 * add as i128 / room as i128) as i64
                }
            })
            .collect::<Vec<_>>()
    };
    let mut diff = target.clamp(sum_lo, sum_hi) - sum(&slots);
    for index in 0..slots.len() {
        if diff > 0 {
            let step = (hi[index] - slots[index]).min(diff);
            slots[index] += step;
            diff -= step;
        } else if diff < 0 {
            let step = (slots[index] - lo[index]).min(-diff);
            slots[index] -= step;
            diff += step;
        }
    }
    slots
}

/// 在精修区间里放一段槽位长的源区间：长了围绕锚点裁，短了在锁定窗内向两侧补，窗不够就用整窗。
fn place_source_range(
    refined: (i64, i64),
    window: (i64, i64),
    slot: i64,
    anchor: Option<i64>,
) -> (i64, i64) {
    let span = refined.1 - refined.0;
    if span >= slot {
        let center = anchor.unwrap_or(refined.0 + span / 2);
        let start = (center - slot / 2).clamp(refined.0, refined.1 - slot);
        return (start, start + slot);
    }
    if window.1 - window.0 <= slot {
        return window;
    }
    let start = (refined.0 - (slot - span) / 2).clamp(window.0, window.1 - slot);
    (start, start + slot)
}

/// 视觉证据给的最佳区间（与精修区间交叠最多的那段）中心，其次是落在区间内的高光时刻。
fn evidence_anchor_ms(
    sources: &[StoryboardSource],
    shot: &StoryboardShot,
    refined: (i64, i64),
) -> Option<i64> {
    let mut best: Option<(i64, i64)> = None;
    let mut highlight = None;
    let details = sources
        .iter()
        .filter(|source| source.asset_id == shot.asset_id)
        .flat_map(|source| source.visual_evidence.iter())
        .filter_map(|evidence| evidence.detail.as_ref());
    for detail in details {
        if let Some(range) = &detail.best_range {
            let start = range.start_ms.max(refined.0);
            let end = range.end_ms.min(refined.1);
            if end > start && best.map_or(true, |(_, overlap)| end - start > overlap) {
                best = Some((start + (end - start) / 2, end - start));
            }
        }
        if highlight.is_none() {
            highlight = detail
                .highlights
                .iter()
                .map(|moment| moment.time_ms)
                .find(|time| *time >= refined.0 && *time <= refined.1);
        }
    }
    best.map(|(center, _)| center).or(highlight)
}

pub(crate) fn usable_ms_for_shot(shot: &StoryboardShot, pools: &[BeatCandidatePool]) -> i64 {
    pools
        .iter()
        .find(|pool| pool.beat_id == shot.beat_id)
        .and_then(|pool| {
            pool.candidates.iter().find(|candidate| {
                candidate.asset_id == shot.asset_id
                    && candidate
                        .segment
                        .as_ref()
                        .map(|segment| segment.id.as_str())
                        == shot.segment_id.as_deref()
            })
        })
        .map(usable_ms)
        .unwrap_or_else(|| (shot.source_end_ms - shot.source_start_ms).max(0))
}

/// 源窗短于口播时放慢已选镜头，不再把短窗交回选片模型换片。
pub(crate) fn collect_usable_window_shortfalls(
    content: &mut StoryboardContent,
    rough: &RoughStoryboard,
) -> Vec<StoryboardIssue> {
    stretch_shots_to_speech_timing(content, &rough.speech_timing);
    Vec::new()
}

/// 模型改了拍旁白时：只允许一对相邻拍、全文拼接不变；有 TTS 则重算每拍起止。
pub(crate) fn sync_narration_and_timing(
    content: &mut StoryboardContent,
    rough: &mut RoughStoryboard,
    alignment: Option<(&Value, i64)>,
) -> Vec<StoryboardIssue> {
    let original = join_narrations(&rough.beats);
    let updated = join_narrations(&content.beats);
    let changed = changed_beat_ids(&rough.beats, &content.beats);
    if changed.is_empty() {
        return Vec::new();
    }
    if updated != original {
        return vec![StoryboardIssue::new(
            "narration_script_changed",
            "Beat narration edits must keep the spoken script wording and order. Concatenating beat narrations no longer matches the original script.",
            true,
        )
        .allowing(vec![
            "move words only between this beat and one neighbor; keep the full spoken script unchanged",
        ])];
    }
    if !changed_beats_are_one_hop(&rough.beats, &changed) {
        return vec![StoryboardIssue::new(
            "narration_move_not_adjacent",
            "Words may move only once, to the previous or next beat. Other beats must keep their narration.",
            true,
        )
        .allowing(vec![
            "change narration on this beat and one neighbor only",
        ])];
    }
    if let Some((alignment, duration_ms)) = alignment {
        match timing::from_alignment(&content.beats, alignment, duration_ms) {
            Some(timing) => {
                rough.speech_timing = timing;
            }
            None => {
                return vec![StoryboardIssue::new(
                    "narration_timing_unmapped",
                    "After moving words, spoken beats no longer map onto the TTS timestamps. Revert or choose a neighbor split that still matches the audio units.",
                    true,
                )
                .allowing(vec![
                    "move a different span of words to the neighbor, or swap to a longer clip instead",
                ])];
            }
        }
    }
    rough.beats = content.beats.clone();
    for shot in &mut content.shots {
        if shot.beat_part_index != 1 {
            continue;
        }
        if let Some(beat) = content.beats.iter().find(|beat| beat.id == shot.beat_id) {
            shot.narration_text = beat.narration.clone();
        }
    }
    Vec::new()
}

fn join_narrations(beats: &[StoryboardBeat]) -> String {
    beats
        .iter()
        .map(|beat| beat.narration.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn changed_beat_ids(original: &[StoryboardBeat], updated: &[StoryboardBeat]) -> Vec<String> {
    let mut changed = Vec::new();
    for beat in updated {
        let prior = original
            .iter()
            .find(|item| item.id == beat.id)
            .map(|item| item.narration.trim());
        if prior != Some(beat.narration.trim()) {
            changed.push(beat.id.clone());
        }
    }
    changed
}

fn changed_beats_are_one_hop(beats: &[StoryboardBeat], changed: &[String]) -> bool {
    if changed.len() != 2 {
        return false;
    }
    let positions = changed
        .iter()
        .filter_map(|id| beats.iter().position(|beat| beat.id == *id))
        .collect::<Vec<_>>();
    positions.len() == 2 && (positions[0] as i64 - positions[1] as i64).abs() == 1
}

pub(crate) fn user_decision_error(
    content: Option<&StoryboardContent>,
    rough: &RoughStoryboard,
    issues: &[StoryboardIssue],
) -> String {
    let mut facts = Vec::new();
    for issue in issues
        .iter()
        .filter(|issue| issue.kind == "beat_audio_window_shortfall")
    {
        let beat_id = issue
            .affected_shots
            .first()
            .and_then(|order| {
                content.and_then(|board| {
                    board
                        .shots
                        .iter()
                        .find(|shot| shot.order_index == *order)
                        .map(|shot| shot.beat_id.clone())
                })
            })
            .or_else(|| issue.message.split('\'').nth(1).map(str::to_owned))
            .unwrap_or_else(|| "unknown".to_owned());
        let needed = rough.speech_timing.duration(&beat_id).unwrap_or(0);
        let available = content
            .map(|board| {
                board
                    .shots
                    .iter()
                    .filter(|shot| shot.beat_id == beat_id)
                    .map(|shot| usable_ms_for_shot(shot, &rough.candidate_pools))
                    .sum::<i64>()
            })
            .unwrap_or(0);
        let top5 = pool_top5_json(&beat_id, &rough.candidate_pools);
        facts.push(json!({
            "beatId": beat_id,
            "narrationMs": needed,
            "selectedUsableMs": available,
            "top5": top5,
        }));
    }
    if facts.is_empty() {
        facts.push(json!({
            "reason": issues.first().map(|issue| issue.message.clone()).unwrap_or_else(|| {
                "Selected clips are shorter than the spoken beats.".to_owned()
            })
        }));
    }
    format!(
        "storyboard_needs_user_decision: a locked shot is shorter than its spoken beat after model repair. Ask the user how to continue. Do not persist this board. facts={}",
        json!(facts)
    )
}

fn pool_top5_json(beat_id: &str, pools: &[BeatCandidatePool]) -> Value {
    let Some(pool) = pools.iter().find(|pool| pool.beat_id == beat_id) else {
        return json!([]);
    };
    json!(pool
        .candidates
        .iter()
        .take(TOP_ALTERNATE_INDEX + 1)
        .enumerate()
        .map(|(index, candidate)| {
            json!({
                "candidateIndex": index,
                "assetId": candidate.asset_id,
                "usableMs": usable_ms(candidate),
            })
        })
        .collect::<Vec<_>>())
}

pub(crate) fn remaining_shortfall_issues(issues: &[StoryboardIssue]) -> bool {
    issues.iter().any(|issue| {
        issue.needs_model_decision
            && matches!(
                issue.kind.as_str(),
                "beat_audio_window_shortfall"
                    | "narration_script_changed"
                    | "narration_move_not_adjacent"
                    | "narration_timing_unmapped"
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CandidateSegment, StoryboardBeat, StoryboardShot, StoryboardSource};
    use crate::storyboard::timing::{BeatTiming, SpeechTiming};

    fn beat(id: &str, narration: &str) -> StoryboardBeat {
        StoryboardBeat {
            id: id.to_owned(),
            purpose: id.to_owned(),
            required_visual: String::new(),
            visual_keywords: vec![],
            narration: narration.to_owned(),
            on_screen_text: String::new(),
            ..Default::default()
        }
    }

    fn shot(order: i64, beat_id: &str, asset: &str, start: i64, end: i64) -> StoryboardShot {
        StoryboardShot {
            crop_focus: None,
            order_index: order,
            duration_ms: end - start,
            purpose: String::new(),
            on_screen_text: String::new(),
            narration_text: String::new(),
            asset_id: asset.to_owned(),
            source_start_ms: start,
            source_end_ms: end,
            reason: String::new(),
            beat_id: beat_id.to_owned(),
            match_level: "contextual".to_owned(),
            beat_part_index: 1,
            beat_part_count: 1,
            split_role: "lead".to_owned(),
            segment_id: Some("s1".to_owned()),
        }
    }

    fn source(asset: &str, start: i64, end: i64) -> StoryboardSource {
        let mut source: StoryboardSource = serde_json::from_value(json!({
            "assetId": asset,
            "kind": "video",
            "durationMs": 20_000,
            "sceneSegments": [],
            "ocrEvidence": [],
            "visualEvidence": []
        }))
        .unwrap();
        source.segment = Some(CandidateSegment {
            id: "s1".to_owned(),
            start_ms: start,
            end_ms: end,
            frame_paths: vec![],
            shot_type: None,
            camera_motion: None,
        });
        source
    }

    fn rough_with_pool(usable_end: i64, narration_ms: i64) -> RoughStoryboard {
        RoughStoryboard {
            speech_timing: SpeechTiming {
                kind: crate::storyboard::timing::SpeechTimingKind::Voice,
                beats: vec![BeatTiming {
                    beat_id: "engineered-together".into(),
                    start_ms: 0,
                    end_ms: narration_ms,
                }],
                pauses_ms: vec![],
            },
            title: "t".into(),
            summary: String::new(),
            target_duration_ms: narration_ms,
            script_mode: "full_script".into(),
            shot_length_hint: String::new(),
            beats: vec![beat("engineered-together", "engineered together")],
            uncovered_beat_ids: vec![],
            shots: vec![],
            candidate_pools: vec![BeatCandidatePool {
                beat_id: "engineered-together".into(),
                beat_purpose: "close".into(),
                candidates: vec![source("short", 0, usable_end), source("long", 0, 8_000)],
                scores: vec![],
            }],
        }
    }

    #[test]
    fn short_locked_window_slows_instead_of_asking_to_swap() {
        let rough = rough_with_pool(2_000, 3_370);
        let mut content = StoryboardContent {
            brief: String::new(),
            title: "t".into(),
            summary: String::new(),
            target_duration_ms: 3_370,
            script_mode: "full_script".into(),
            beats: rough.beats.clone(),
            uncovered_beat_ids: vec![],
            shots: vec![shot(6, "engineered-together", "short", 0, 2_000)],
        };
        let issues = collect_usable_window_shortfalls(&mut content, &rough);
        assert!(issues.is_empty());
        assert_eq!(content.shots[0].source_start_ms, 0);
        assert_eq!(content.shots[0].source_end_ms, 2_000);
        assert_eq!(content.shots[0].duration_ms, 3_370);
    }

    #[test]
    fn long_enough_usable_window_trims_to_narration() {
        let rough = rough_with_pool(8_000, 3_370);
        let mut content = StoryboardContent {
            brief: String::new(),
            title: "t".into(),
            summary: String::new(),
            target_duration_ms: 3_370,
            script_mode: "full_script".into(),
            beats: rough.beats.clone(),
            uncovered_beat_ids: vec![],
            shots: vec![shot(1, "engineered-together", "long", 0, 8_000)],
        };
        assert!(collect_usable_window_shortfalls(&mut content, &rough).is_empty());
        assert_eq!(content.shots[0].duration_ms, 3_370);
        assert_eq!(
            content.shots[0].source_end_ms - content.shots[0].source_start_ms,
            3_370
        );
    }

    #[test]
    fn one_hop_narration_move_is_accepted() {
        let mut rough = RoughStoryboard {
            speech_timing: SpeechTiming::default(),
            title: "t".into(),
            summary: String::new(),
            target_duration_ms: 4_000,
            script_mode: "full_script".into(),
            shot_length_hint: String::new(),
            beats: vec![beat("a", "hello there"), beat("b", "friends")],
            uncovered_beat_ids: vec![],
            shots: vec![],
            candidate_pools: vec![],
        };
        let mut content = StoryboardContent {
            brief: String::new(),
            title: "t".into(),
            summary: String::new(),
            target_duration_ms: 4_000,
            script_mode: "full_script".into(),
            beats: vec![beat("a", "hello"), beat("b", "there friends")],
            uncovered_beat_ids: vec![],
            shots: vec![shot(1, "a", "x", 0, 2_000)],
        };
        let issues = sync_narration_and_timing(&mut content, &mut rough, None);
        assert!(issues.is_empty());
        assert_eq!(rough.beats[0].narration, "hello");
        assert_eq!(rough.beats[1].narration, "there friends");
        assert_eq!(content.shots[0].narration_text, "hello");
    }

    #[test]
    fn non_adjacent_or_rewritten_narration_is_rejected() {
        let mut rough = RoughStoryboard {
            speech_timing: SpeechTiming::default(),
            title: "t".into(),
            summary: String::new(),
            target_duration_ms: 4_000,
            script_mode: "full_script".into(),
            shot_length_hint: String::new(),
            beats: vec![beat("a", "one"), beat("b", "two"), beat("c", "three")],
            uncovered_beat_ids: vec![],
            shots: vec![],
            candidate_pools: vec![],
        };
        let mut rewritten = StoryboardContent {
            brief: String::new(),
            title: "t".into(),
            summary: String::new(),
            target_duration_ms: 4_000,
            script_mode: "full_script".into(),
            beats: vec![beat("a", "uno"), beat("b", "two"), beat("c", "three")],
            uncovered_beat_ids: vec![],
            shots: vec![],
        };
        assert_eq!(
            sync_narration_and_timing(&mut rewritten, &mut rough, None)[0].kind,
            "narration_script_changed"
        );
        let mut skipped = StoryboardContent {
            brief: String::new(),
            title: "t".into(),
            summary: String::new(),
            target_duration_ms: 4_000,
            script_mode: "full_script".into(),
            beats: vec![beat("a", "one three"), beat("b", "two"), beat("c", "")],
            uncovered_beat_ids: vec![],
            shots: vec![],
        };
        // concat "one three two" != "one two three"
        assert_eq!(
            sync_narration_and_timing(&mut skipped, &mut rough, None)[0].kind,
            "narration_script_changed"
        );
        let mut far = StoryboardContent {
            brief: String::new(),
            title: "t".into(),
            summary: String::new(),
            target_duration_ms: 4_000,
            script_mode: "full_script".into(),
            beats: vec![beat("a", "one three"), beat("b", "two"), beat("c", "")],
            uncovered_beat_ids: vec![],
            shots: vec![],
        };
        // Make concat match but a and c changed (not adjacent pair only... actually
        // "one three" + "two" + "" joins to "one three two" still mismatch.
        // Adjacent-only with matching concat:
        far.beats = vec![beat("a", "one two three"), beat("b", "two"), beat("c", "")];
        // concat "one two three two" mismatch. Need: move between a and c would change two non-adjacent.
        far.beats = vec![beat("a", "one three"), beat("b", "two"), beat("c", "")];
        // skip this messy case; test a+c change with matching join:
        // original "one two three"; a="one two three", b="two", c="" join = "one two three two" no.
        // original join "one two three"; new a="one", b="two", c="three" is a and c changed? a same? a was "one" still. Only c... wait c was "three".
        // Change a and c: a="one two", c="three" but then b still "two" → "one two two three".
        // The real non-adjacent case: a="one two", b="two", c="three" - only a changed, len != 2.
        far.beats = vec![beat("a", "one three"), beat("b", "two"), beat("c", "")];
        // We'll construct matching concat with a and c changed:
        // original: one | two | three
        // new: one two three | two |   → join "one two three two"
        // new: one |  | two three → a unchanged? a still one. changed = b,c adjacent.
        // new: one three | two |   wait join "one three two"
        // To get matching "one two three" with a and c: a="one two three", b="", c="" → changed a,b,c len 3.
        // a="one three", b="two", c="" → join "one three two" !=
        // The function requires join equal AND exactly 2 adjacent. So:
        far.beats = vec![beat("a", "one two three"), beat("b", ""), beat("c", "")];
        // join "one two three" matches; changed = a,b,c (3) → not one hop
        assert_eq!(
            sync_narration_and_timing(&mut far, &mut rough, None)[0].kind,
            "narration_move_not_adjacent"
        );
    }

    /// 2026-09-27「Weekend Road Trip」（无配音）：12 镜全是 2500ms，10 秒以上的精修区间只播前 2.5 秒，
    /// 1.4 秒的区间被放慢到 2.5 秒。改为时长跟精修区间走、源区间与槽位等长。
    #[test]
    fn content_clock_follows_refined_ranges_without_truncating_or_slowing() {
        let refined = [
            ("walk", 3_865, 14_208),
            ("tent", 9_100, 10_500),
            ("jam", 6_500, 8_500),
            ("glow", 0, 22_568),
        ];
        let shots = refined
            .iter()
            .enumerate()
            .map(|(index, (asset, start, end))| {
                let mut shot = shot(index as i64 + 1, asset, asset, *start, *end);
                shot.duration_ms = 2_500;
                shot
            })
            .collect::<Vec<_>>();
        let mut content: StoryboardContent = serde_json::from_value(
            json!({"title":"t","summary":"s","targetDurationMs":12_000,"shots":[]}),
        )
        .unwrap();
        content.shots = shots;
        let windows = refined
            .iter()
            .enumerate()
            .map(|(index, (asset, _, _))| {
                let window = Phase4ContentWindow {
                    window_id: "s1".to_owned(),
                    asset_id: (*asset).to_owned(),
                    start_ms: 0,
                    end_ms: 23_000,
                };
                (index as i64 + 1, (window, false))
            })
            .collect::<HashMap<_, _>>();
        let sources = refined
            .iter()
            .map(|(asset, _, _)| source(asset, 0, 23_000))
            .collect::<Vec<_>>();
        fit_shots_to_content(&mut content, &windows, &sources, (1_500, 5_000), None);

        let durations = content
            .shots
            .iter()
            .map(|shot| shot.duration_ms)
            .collect::<Vec<_>>();
        assert_eq!(durations.iter().sum::<i64>(), 12_000);
        assert!(
            durations.iter().all(|ms| (1_500..=5_000).contains(ms)),
            "{durations:?}"
        );
        assert!(
            durations.windows(2).any(|pair| pair[0] != pair[1]),
            "{durations:?}"
        );
        for (shot, (_, start, end)) in content.shots.iter().zip(refined) {
            assert_eq!(shot.source_end_ms - shot.source_start_ms, shot.duration_ms);
            assert!(!shot.reason.contains("slowed"), "{}", shot.reason);
            // 长区间裁在精修区间内部，不是永远取开头。
            if end - start > shot.duration_ms {
                assert!(shot.source_start_ms >= start && shot.source_end_ms <= end);
            }
        }
        assert!(
            content.shots[3].source_start_ms > 0,
            "long range must not keep only its head"
        );
    }
}
