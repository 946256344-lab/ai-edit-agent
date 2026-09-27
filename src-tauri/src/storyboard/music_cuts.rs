//! 音乐先行（BGM 开、配音关）：Phase 4 按精修内容定好每镜时长后，把切点吸附到音乐窗口的节拍网格上，
//! 总长等于音乐窗口，源区间随新槽位重放（仍 1 倍速，窗不够才放慢并写明）。网格算不出时保留内容时长。

use super::length::apply_content_slot;
use super::multimodal::Phase4ContentWindow;
use crate::models::{StoryboardContent, StoryboardSource};
use crate::music_plan::{snap_slots_to_beats, MusicPlan};
use std::collections::HashMap;

pub(crate) fn snap_shots_to_music(
    content: &mut StoryboardContent,
    windows: &HashMap<i64, (Phase4ContentWindow, bool)>,
    sources: &[StoryboardSource],
    bounds: (i64, i64),
    plan: &MusicPlan,
) -> bool {
    let shots = &content.shots;
    if shots.is_empty() {
        return false;
    }
    let window_of = |index: usize| {
        let shot = &shots[index];
        let is_image = sources
            .iter()
            .find(|source| source.asset_id == shot.asset_id)
            .is_some_and(|source| source.kind == "image");
        if is_image {
            return None;
        }
        Some(
            windows
                .get(&shot.order_index)
                .map(|(window, _)| window)
                .filter(|window| window.asset_id == shot.asset_id)
                .map_or((shot.source_start_ms, shot.source_end_ms), |window| {
                    (window.start_ms, window.end_ms)
                }),
        )
    };
    let preferred = shots.iter().map(|shot| shot.duration_ms.max(1)).collect::<Vec<_>>();
    let sections = shots
        .iter()
        .enumerate()
        .map(|(index, shot)| index > 0 && shot.beat_id != shots[index - 1].beat_id)
        .collect::<Vec<_>>();
    let placement = (0..shots.len()).map(window_of).collect::<Vec<_>>();
    let capacity = placement
        .iter()
        .map(|window| window.map_or(i64::MAX, |(start, end)| (end - start).max(1)))
        .collect::<Vec<_>>();
    let Some(slots) = snap_slots_to_beats(&preferred, &sections, &capacity, bounds, plan) else {
        log::warn!(
            "Music-first pacing: {} shots do not fit the {}ms beat grid; keeping content-driven lengths",
            shots.len(),
            plan.duration_ms
        );
        return false;
    };
    for ((shot, slot), window) in content.shots.iter_mut().zip(&slots).zip(&placement) {
        let refined = (shot.source_start_ms, shot.source_end_ms);
        apply_content_slot(shot, refined, *window, sources, *slot);
    }
    content.target_duration_ms = plan.duration_ms;
    log::info!(
        "Music-first pacing: {} shots snapped to \"{}\" ({:.1} BPM, offset {}ms, {}ms): {:?}",
        slots.len(),
        plan.title,
        plan.tempo_bpm,
        plan.source_start_ms,
        plan.duration_ms,
        slots
    );
    true
}
