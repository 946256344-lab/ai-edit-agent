//! 口播时钟对照源窗：短了就放慢镜头，不换片、不补镜、不拼下一段硬切。

use super::phases::{BeatCandidatePool, RoughStoryboard};
use super::repair::StoryboardIssue;
use super::timing::{self, SpeechTiming};
use crate::models::{StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource};
use serde_json::{json, Value};

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
    if timing.beats.is_empty() {
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
}
