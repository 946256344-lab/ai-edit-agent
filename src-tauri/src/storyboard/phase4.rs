//! Phase 4 当次调用内的精修进度：成功结果留在内存，失败从该处继续。
//!
//! 不替代 `RepairPacket`（那只是提示快照），不落库、不跨运行恢复。

#[cfg(test)]
use crate::models::StoryboardShot;
use crate::models::{StoryboardContent, StoryboardSource};
use crate::provider::ModelAccess;
use crate::storyboard::multimodal::{
    build_phase4_windows_from_keyframes, compose_timed_frame_grid, densify_times_in_range,
    extract_frames_at_times, read_input_image, Phase4ContentWindow, PHASE4_MAX_FRAME_SPACING_MS,
    PHASE4_PASS_A_MAX_IMAGES, PHASE4_REFINE_FRAMES, PHASE4_REFINE_SHOTS_PER_BATCH,
    PHASE4_UNCERTAIN_FRAMES,
};
use crate::storyboard::phases::{
    apply_narration_phrase_duration_floor_scoped, clamp_shots_to_chosen_windows_scoped,
    collect_phase4_issues, resolve_overlaps_within_chosen_windows_scoped, RoughStoryboard,
};
use crate::storyboard::repair::{
    parse_affected_shot_indices, repair_packet_prompt_block, RepairPacket, StoryboardIssue,
};
use crate::storyboard::{model_response_json_text, post_model_payload, STORYBOARD_TIMEOUT};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tauri::AppHandle;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Phase4Pass {
    A,
    B,
    C,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PassCMode {
    Uncertain,
    Narrow,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Phase4WindowPick {
    pub(crate) order_index: i64,
    pub(crate) window_id: String,
    #[serde(default)]
    pub(crate) uncertain: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Phase4ShotPatch {
    pub(crate) order_index: i64,
    pub(crate) source_start_ms: i64,
    pub(crate) source_end_ms: i64,
    #[serde(default)]
    pub(crate) crop_focus: Option<[f64; 2]>,
    #[serde(default)]
    pub(crate) narration_text: Option<String>,
    #[serde(default)]
    pub(crate) on_screen_text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct MaterialKey {
    pass: Phase4Pass,
    id: String,
    start_ms: i64,
    end_ms: i64,
    frames: usize,
}

impl MaterialKey {
    pub(crate) fn pass_a(window_id: &str, mid_ms: i64) -> Self {
        Self {
            pass: Phase4Pass::A,
            id: window_id.to_owned(),
            start_ms: mid_ms,
            end_ms: mid_ms,
            frames: 1,
        }
    }

    pub(crate) fn pass_b(order_index: i64, window_id: &str, start_ms: i64, end_ms: i64) -> Self {
        Self {
            pass: Phase4Pass::B,
            id: format!("{order_index}:{window_id}"),
            start_ms,
            end_ms,
            frames: crate::storyboard::multimodal::PHASE4_REFINE_FRAMES,
        }
    }

    pub(crate) fn pass_c(
        order_index: i64,
        window_id: &str,
        start_ms: i64,
        end_ms: i64,
        frames: usize,
    ) -> Self {
        Self {
            pass: Phase4Pass::C,
            id: format!("{order_index}:{window_id}"),
            start_ms,
            end_ms,
            frames,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Phase4ModelCall {
    pub(crate) pass: Phase4Pass,
    pub(crate) orders: Vec<i64>,
}

/// 仅服务于当前 `generate_storyboard_internal` / 换镜调用的 Phase 4 内存状态。
#[derive(Default)]
pub(crate) struct Phase4Session {
    initialized: bool,
    content: Option<StoryboardContent>,
    pick_map: HashMap<i64, (Phase4ContentWindow, bool)>,
    pass_a_done: HashSet<i64>,
    pass_a_pending: HashSet<i64>,
    pass_b_done: HashSet<i64>,
    pass_b_pending: HashSet<i64>,
    pass_c_done: HashSet<i64>,
    pass_c_pending: HashSet<i64>,
    pass_c_modes: HashMap<i64, PassCMode>,
    repair_shots: HashSet<i64>,
    windows_by_asset: HashMap<String, Vec<Phase4ContentWindow>>,
    all_windows: Vec<Phase4ContentWindow>,
    selected_sources: Vec<StoryboardSource>,
    pass_a_asset_batches: Vec<Vec<String>>,
    materials: HashMap<MaterialKey, Vec<Value>>,
    pub(crate) model_calls: Vec<Phase4ModelCall>,
    ranges_seeded: bool,
    pass_c_planned: bool,
}

impl Phase4Session {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn ensure_initialized(
        &mut self,
        selected: &StoryboardContent,
        sources: &[StoryboardSource],
    ) -> Result<(), String> {
        if self.initialized {
            return Ok(());
        }
        let selected_asset_ids = selected
            .shots
            .iter()
            .map(|shot| shot.asset_id.as_str())
            .collect::<HashSet<_>>();
        let mut seen_assets = HashSet::new();
        let selected_sources = sources
            .iter()
            .filter(|source| selected_asset_ids.contains(source.asset_id.as_str()))
            .filter(|source| seen_assets.insert(source.asset_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if selected_sources.len() != selected_asset_ids.len() {
            return Err("Phase 4 source scope was unavailable.".to_owned());
        }

        let mut windows_by_asset: HashMap<String, Vec<Phase4ContentWindow>> = HashMap::new();
        let mut all_windows: Vec<Phase4ContentWindow> = Vec::new();
        for source in &selected_sources {
            let duration_ms = source.duration_ms.unwrap_or(0).max(1);
            let real_segments = source
                .scene_segments
                .iter()
                .filter(|segment| !segment.id.is_empty() && segment.end_ms > segment.start_ms)
                .collect::<Vec<_>>();
            let windows = if !real_segments.is_empty() {
                real_segments
                    .into_iter()
                    .map(|segment| Phase4ContentWindow {
                        asset_id: source.asset_id.clone(),
                        window_id: segment.id.clone(),
                        start_ms: segment.start_ms,
                        end_ms: segment.end_ms,
                    })
                    .collect::<Vec<_>>()
            } else {
                let keyframe_times: Vec<i64> =
                    source.keyframes.iter().map(|frame| frame.time_ms).collect();
                build_phase4_windows_from_keyframes(&source.asset_id, duration_ms, &keyframe_times)
            };
            log::info!(
                "Phase 4 windows for {}: count={} from_segments={}",
                source.asset_id,
                windows.len(),
                source
                    .scene_segments
                    .iter()
                    .filter(|segment| !segment.id.is_empty())
                    .count()
            );
            windows_by_asset.insert(source.asset_id.clone(), windows.clone());
            all_windows.extend(windows);
        }
        if all_windows.is_empty() {
            return Err("Phase 4 could not build content windows.".to_owned());
        }

        let mut pass_a_pending = HashSet::new();
        let mut pass_a_done = HashSet::new();
        for shot in &selected.shots {
            if let Some(segment_id) = shot.segment_id.as_deref() {
                let asset_windows = windows_by_asset
                    .get(&shot.asset_id)
                    .cloned()
                    .unwrap_or_default();
                let locked = asset_windows
                    .iter()
                    .find(|window| window.window_id == segment_id)
                    .cloned()
                    .or_else(|| {
                        Some(Phase4ContentWindow {
                            asset_id: shot.asset_id.clone(),
                            window_id: segment_id.to_owned(),
                            start_ms: shot.source_start_ms,
                            end_ms: shot.source_end_ms.max(shot.source_start_ms + 1),
                        })
                    });
                if let Some(mut window) = locked {
                    if window.span_ms() < shot.duration_ms.max(1) {
                        if let Some(next) = asset_windows.iter().find(|candidate| {
                            candidate.start_ms >= window.end_ms
                                || (candidate.window_id != window.window_id
                                    && candidate.start_ms == window.end_ms)
                        }) {
                            window.end_ms = next.end_ms;
                            window.window_id = format!("{}+{}", window.window_id, next.window_id);
                        }
                    }
                    windows_by_asset
                        .entry(shot.asset_id.clone())
                        .or_default()
                        .retain(|existing| existing.window_id != window.window_id);
                    windows_by_asset
                        .entry(shot.asset_id.clone())
                        .or_default()
                        .push(window.clone());
                    self.pick_map.insert(shot.order_index, (window, false));
                    pass_a_done.insert(shot.order_index);
                    continue;
                }
            }
            pass_a_pending.insert(shot.order_index);
        }

        let pass_a_asset_ids = selected
            .shots
            .iter()
            .filter(|shot| pass_a_pending.contains(&shot.order_index))
            .map(|shot| shot.asset_id.clone())
            .collect::<HashSet<_>>();
        let mut asset_order = pass_a_asset_ids.into_iter().collect::<Vec<_>>();
        asset_order.sort();
        let mut pass_a_asset_batches: Vec<Vec<String>> = Vec::new();
        let mut current_batch: Vec<String> = Vec::new();
        let mut current_images = 0usize;
        for asset_id in &asset_order {
            let window_count = windows_by_asset
                .get(asset_id)
                .map(|windows| windows.len())
                .unwrap_or(0);
            if window_count == 0 {
                continue;
            }
            if !current_batch.is_empty() && current_images + window_count > PHASE4_PASS_A_MAX_IMAGES
            {
                pass_a_asset_batches.push(std::mem::take(&mut current_batch));
                current_images = 0;
            }
            current_batch.push(asset_id.clone());
            current_images += window_count;
        }
        if !current_batch.is_empty() {
            pass_a_asset_batches.push(current_batch);
        }

        let all_orders = selected
            .shots
            .iter()
            .map(|shot| shot.order_index)
            .collect::<HashSet<_>>();
        self.content = Some(selected.clone());
        self.windows_by_asset = windows_by_asset;
        self.all_windows = all_windows;
        self.selected_sources = selected_sources;
        self.pass_a_pending = pass_a_pending;
        self.pass_a_done = pass_a_done;
        self.pass_a_asset_batches = pass_a_asset_batches;
        self.pass_b_pending = all_orders;
        self.pass_b_done = HashSet::new();
        self.initialized = true;
        if self.pass_a_pending.is_empty() {
            log::info!(
                "Phase 4 pass A skipped: all {} shots locked to Phase 3 segments",
                selected.shots.len()
            );
        }
        Ok(())
    }

    pub(crate) fn apply_repair_if_needed(
        &mut self,
        repair: Option<&RepairPacket>,
        selected: &StoryboardContent,
    ) -> Result<(), String> {
        if !self.passes_ready_for_validation() {
            return Ok(());
        }
        let Some(repair) = repair else {
            return Ok(());
        };
        if repair
            .issues
            .iter()
            .all(|issue| issue.kind == "request_failed" || is_structural_phase4_issue(&issue.kind))
        {
            return Ok(());
        }
        if repair
            .issues
            .iter()
            .any(|issue| is_structural_phase4_issue(&issue.kind))
        {
            return Err(format!(
                "Phase 4 structural issue cannot be repaired by re-running refine: {}",
                repair
                    .issues
                    .iter()
                    .find(|issue| is_structural_phase4_issue(&issue.kind))
                    .map(|issue| issue.message.as_str())
                    .unwrap_or("unknown")
            ));
        }
        let content = self.content.as_ref().unwrap_or(selected);
        let set = repair_set_from_issues(&repair.issues, content)?;
        if set.is_empty() {
            return Ok(());
        }
        self.queue_repair(set, issues_need_window_repick(&repair.issues), selected);
        Ok(())
    }

    fn queue_repair(
        &mut self,
        set: HashSet<i64>,
        repick_windows: bool,
        selected: &StoryboardContent,
    ) {
        log::info!(
            "Phase 4 local repair queued for shots {:?} (repick_windows={repick_windows})",
            {
                let mut orders = set.iter().copied().collect::<Vec<_>>();
                orders.sort_unstable();
                orders
            }
        );
        self.repair_shots = set.clone();
        self.pass_c_planned = false;
        self.pass_c_pending.clear();
        for order in &set {
            self.pass_c_done.remove(order);
            self.pass_c_modes.remove(order);
            self.pass_b_done.remove(order);
            self.pass_b_pending.insert(*order);
            if repick_windows {
                self.pass_a_done.remove(order);
                self.pass_a_pending.insert(*order);
                self.invalidate_shot_materials(*order);
            }
        }
        if repick_windows {
            self.ranges_seeded = false;
            self.rebuild_pass_a_asset_batches(selected);
        }
    }

    fn rebuild_pass_a_asset_batches(&mut self, selected: &StoryboardContent) {
        let pass_a_asset_ids = selected
            .shots
            .iter()
            .filter(|shot| self.pass_a_pending.contains(&shot.order_index))
            .map(|shot| shot.asset_id.clone())
            .collect::<HashSet<_>>();
        let mut asset_order = pass_a_asset_ids.into_iter().collect::<Vec<_>>();
        asset_order.sort();
        let mut batches = Vec::new();
        let mut current_batch = Vec::new();
        let mut current_images = 0usize;
        for asset_id in &asset_order {
            let window_count = self
                .windows_by_asset
                .get(asset_id)
                .map(|windows| windows.len())
                .unwrap_or(0);
            if window_count == 0 {
                continue;
            }
            if !current_batch.is_empty() && current_images + window_count > PHASE4_PASS_A_MAX_IMAGES
            {
                batches.push(std::mem::take(&mut current_batch));
                current_images = 0;
            }
            current_batch.push(asset_id.clone());
            current_images += window_count;
        }
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }
        self.pass_a_asset_batches = batches;
    }

    pub(crate) fn selected_sources(&self) -> &[StoryboardSource] {
        &self.selected_sources
    }

    pub(crate) fn all_windows(&self) -> &[Phase4ContentWindow] {
        &self.all_windows
    }

    pub(crate) fn pick_map(&self) -> &HashMap<i64, (Phase4ContentWindow, bool)> {
        &self.pick_map
    }

    pub(crate) fn content(&self) -> Option<&StoryboardContent> {
        self.content.as_ref()
    }

    pub(crate) fn replace_content(&mut self, content: StoryboardContent) {
        self.content = Some(content);
    }

    pub(crate) fn repair_shots(&self) -> &HashSet<i64> {
        &self.repair_shots
    }

    /// 机械调整允许改写的镜头。首次完整精修为全部；局部修复仅限修复集合。
    pub(crate) fn mutable_orders(&self) -> Option<HashSet<i64>> {
        if self.repair_shots.is_empty() {
            None
        } else {
            Some(self.repair_shots.clone())
        }
    }

    pub(crate) fn frozen_orders(&self, all_orders: impl IntoIterator<Item = i64>) -> HashSet<i64> {
        match self.mutable_orders() {
            None => HashSet::new(),
            Some(mutable) => all_orders
                .into_iter()
                .filter(|order| !mutable.contains(order))
                .collect(),
        }
    }

    pub(crate) fn record_call(&mut self, pass: Phase4Pass, mut orders: Vec<i64>) {
        orders.sort_unstable();
        self.model_calls.push(Phase4ModelCall { pass, orders });
    }

    pub(crate) fn cached_blocks(&self, key: &MaterialKey) -> Option<&Vec<Value>> {
        self.materials.get(key)
    }

    pub(crate) fn store_blocks(&mut self, key: MaterialKey, blocks: Vec<Value>) {
        self.materials.insert(key, blocks);
    }

    pub(crate) fn invalidate_shot_materials(&mut self, order_index: i64) {
        let prefix_b = format!("{order_index}:");
        self.materials.retain(|key, _| match key.pass {
            Phase4Pass::A => true,
            Phase4Pass::B | Phase4Pass::C => !key.id.starts_with(&prefix_b),
        });
    }

    pub(crate) fn pending_pass_a_asset_batches(&self) -> Vec<Vec<String>> {
        self.pass_a_asset_batches
            .iter()
            .filter(|batch| {
                !self.pass_a_orders_for_assets(batch).is_empty()
                    && self
                        .pass_a_orders_for_assets(batch)
                        .iter()
                        .any(|order| self.pass_a_pending.contains(order))
            })
            .cloned()
            .collect()
    }

    pub(crate) fn pass_a_orders_for_assets(&self, assets: &[String]) -> Vec<i64> {
        let asset_set = assets.iter().cloned().collect::<HashSet<_>>();
        self.content
            .as_ref()
            .map(|content| {
                content
                    .shots
                    .iter()
                    .filter(|shot| {
                        asset_set.contains(&shot.asset_id)
                            && (self.pass_a_pending.contains(&shot.order_index)
                                || self.pass_a_done.contains(&shot.order_index))
                    })
                    .filter(|shot| self.pass_a_pending.contains(&shot.order_index))
                    .map(|shot| shot.order_index)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn pass_a_is_complete(&self) -> bool {
        self.pass_a_pending.is_empty()
    }

    pub(crate) fn complete_pass_a_batch(
        &mut self,
        batch_assets: &[String],
        text: &str,
        selected: &StoryboardContent,
    ) -> Result<(), String> {
        let allowed = self
            .pass_a_orders_for_assets(batch_assets)
            .into_iter()
            .collect::<HashSet<_>>();
        if allowed.is_empty() {
            return Ok(());
        }
        let valid_windows = window_ids_for_orders(selected, &self.windows_by_asset, &allowed);
        let picks = parse_window_picks_for_batch(text, &allowed, &valid_windows)?;
        for pick in picks {
            let Some(shot) = selected
                .shots
                .iter()
                .find(|shot| shot.order_index == pick.order_index)
            else {
                continue;
            };
            let Some(window) = self
                .windows_by_asset
                .get(&shot.asset_id)
                .and_then(|windows| {
                    windows
                        .iter()
                        .find(|window| window.window_id == pick.window_id)
                        .cloned()
                })
            else {
                return Err(format!(
                    "Phase 4a window '{}' was not valid for shot {}.",
                    pick.window_id, pick.order_index
                ));
            };
            self.pick_map
                .insert(pick.order_index, (window, pick.uncertain));
            self.pass_a_pending.remove(&pick.order_index);
            self.pass_a_done.insert(pick.order_index);
        }
        Ok(())
    }

    pub(crate) fn seed_ranges_from_windows(&mut self, selected: &StoryboardContent) {
        if self.ranges_seeded {
            return;
        }
        let Some(content) = self.content.as_mut() else {
            return;
        };
        for shot in &mut content.shots {
            if !self.pass_b_done.contains(&shot.order_index) {
                if let Some((window, uncertain)) = self.pick_map.get(&shot.order_index) {
                    let target = shot.duration_ms.max(1).min(window.span_ms().max(1));
                    let start = window
                        .mid_ms()
                        .saturating_sub(target / 2)
                        .clamp(window.start_ms, window.end_ms.saturating_sub(1));
                    let end = (start + target).min(window.end_ms).max(start + 1);
                    shot.source_start_ms = start;
                    shot.source_end_ms = end;
                    shot.duration_ms = end - start;
                    let locked = selected
                        .shots
                        .iter()
                        .find(|item| item.order_index == shot.order_index);
                    if let Some(locked) = locked {
                        shot.reason = if *uncertain {
                            format!("{} [window={} uncertain]", locked.reason, window.window_id)
                        } else {
                            format!("{} [window={}]", locked.reason, window.window_id)
                        };
                    }
                }
            }
        }
        self.ranges_seeded = true;
    }

    pub(crate) fn pending_pass_b_batches(&self) -> Vec<Vec<i64>> {
        refine_batches(&self.pending_orders(&self.pass_b_pending))
    }

    pub(crate) fn pass_b_is_complete(&self) -> bool {
        self.pass_b_pending.is_empty()
    }

    pub(crate) fn complete_pass_b_batch(
        &mut self,
        batch_orders: &[i64],
        text: &str,
        locked: &StoryboardContent,
    ) -> Result<(), String> {
        let allowed = batch_orders.iter().copied().collect::<HashSet<_>>();
        let patches = parse_shot_patches_for_batch(text, &allowed)?;
        apply_shot_patches(
            self.content
                .as_mut()
                .ok_or("Phase 4 session has no draft.")?,
            &patches,
            locked,
        )?;
        for order in batch_orders {
            self.pass_b_pending.remove(order);
            self.pass_b_done.insert(*order);
        }
        Ok(())
    }

    pub(crate) fn plan_pass_c_if_needed(&mut self, max_spacing_ms: i64, refine_frames: usize) {
        if self.pass_c_planned {
            return;
        }
        let mut pending = HashSet::new();
        let mut modes = HashMap::new();
        for (order, (window, uncertain)) in &self.pick_map {
            if !self.pass_b_done.contains(order) {
                continue;
            }
            if self.repair_shots.is_empty() || self.repair_shots.contains(order) {
                // 局部修复只给修复集合补 Pass C；首次给所有需要的镜头排期。
            } else {
                continue;
            }
            if self.pass_c_done.contains(order) {
                continue;
            }
            if *uncertain {
                pending.insert(*order);
                modes.insert(*order, PassCMode::Uncertain);
                continue;
            }
            let spacing = window.span_ms() / refine_frames.max(1) as i64;
            if spacing > max_spacing_ms {
                pending.insert(*order);
                modes.insert(*order, PassCMode::Narrow);
            }
        }
        self.pass_c_pending = pending;
        self.pass_c_modes = modes;
        self.pass_c_planned = true;
    }

    pub(crate) fn pending_pass_c_batches(&self) -> Vec<Vec<(i64, PassCMode)>> {
        let mut orders = self.pending_orders(&self.pass_c_pending);
        orders.sort_unstable();
        orders
            .chunks(PHASE4_REFINE_SHOTS_PER_BATCH)
            .map(|chunk| {
                chunk
                    .iter()
                    .filter_map(|order| self.pass_c_modes.get(order).map(|mode| (*order, *mode)))
                    .collect()
            })
            .filter(|batch: &Vec<(i64, PassCMode)>| !batch.is_empty())
            .collect()
    }

    pub(crate) fn pass_c_is_complete(&self) -> bool {
        self.pass_c_pending.is_empty()
    }

    pub(crate) fn complete_pass_c_batch(
        &mut self,
        batch_orders: &[i64],
        text: &str,
        locked: &StoryboardContent,
    ) -> Result<(), String> {
        let allowed = batch_orders.iter().copied().collect::<HashSet<_>>();
        let patches = parse_shot_patches_for_batch(text, &allowed)?;
        apply_shot_patches(
            self.content
                .as_mut()
                .ok_or("Phase 4 session has no draft.")?,
            &patches,
            locked,
        )?;
        for order in batch_orders {
            self.pass_c_pending.remove(order);
            self.pass_c_done.insert(*order);
        }
        Ok(())
    }

    pub(crate) fn passes_ready_for_validation(&self) -> bool {
        self.initialized
            && self.pass_a_is_complete()
            && self.pass_b_is_complete()
            && self.pass_c_planned
            && self.pass_c_is_complete()
    }

    fn pending_orders(&self, pending: &HashSet<i64>) -> Vec<i64> {
        let mut orders = pending.iter().copied().collect::<Vec<_>>();
        orders.sort_unstable();
        orders
    }
}

fn refine_batches(orders: &[i64]) -> Vec<Vec<i64>> {
    orders
        .chunks(PHASE4_REFINE_SHOTS_PER_BATCH)
        .map(|chunk| chunk.to_vec())
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

fn window_ids_for_orders(
    selected: &StoryboardContent,
    windows_by_asset: &HashMap<String, Vec<Phase4ContentWindow>>,
    allowed: &HashSet<i64>,
) -> HashMap<i64, HashSet<String>> {
    let mut map = HashMap::new();
    for shot in &selected.shots {
        if !allowed.contains(&shot.order_index) {
            continue;
        }
        let ids = windows_by_asset
            .get(&shot.asset_id)
            .map(|windows| {
                windows
                    .iter()
                    .map(|window| window.window_id.clone())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        map.insert(shot.order_index, ids);
    }
    map
}

pub(crate) fn parse_window_picks_for_batch(
    text: &str,
    allowed: &HashSet<i64>,
    valid_windows: &HashMap<i64, HashSet<String>>,
) -> Result<Vec<Phase4WindowPick>, String> {
    let value = serde_json::from_str::<Value>(text)
        .map_err(|_| "Phase 4a response did not contain JSON.".to_owned())?;
    let picks_value = value
        .get("picks")
        .cloned()
        .or_else(|| value.get("windowPicks").cloned())
        .ok_or_else(|| "Phase 4a JSON did not include picks.".to_owned())?;
    let picks = serde_json::from_value::<Vec<Phase4WindowPick>>(picks_value)
        .map_err(|_| "Phase 4a JSON did not match window pick schema.".to_owned())?;
    validate_batch_orders(
        "Phase 4a",
        picks.iter().map(|pick| pick.order_index),
        allowed,
    )?;
    for pick in &picks {
        let Some(windows) = valid_windows.get(&pick.order_index) else {
            return Err(format!(
                "Phase 4a returned window for unknown shot {}.",
                pick.order_index
            ));
        };
        if !windows.contains(&pick.window_id) {
            return Err(format!(
                "Phase 4a window '{}' is not valid for shot {}.",
                pick.window_id, pick.order_index
            ));
        }
    }
    Ok(picks)
}

pub(crate) fn parse_shot_patches_for_batch(
    text: &str,
    allowed: &HashSet<i64>,
) -> Result<Vec<Phase4ShotPatch>, String> {
    let value = serde_json::from_str::<Value>(text)
        .map_err(|_| "Phase 4 refine response did not contain JSON.".to_owned())?;
    let shots_value = value
        .get("shots")
        .cloned()
        .ok_or_else(|| "Phase 4 refine JSON did not include shots.".to_owned())?;
    let raw = shots_value
        .as_array()
        .ok_or_else(|| "Phase 4 refine shots must be an array.".to_owned())?;
    let mut patches = Vec::with_capacity(raw.len());
    for item in raw {
        let patch = parse_one_shot_patch(item)?;
        patches.push(patch);
    }
    validate_batch_orders(
        "Phase 4 refine",
        patches.iter().map(|patch| patch.order_index),
        allowed,
    )?;
    for patch in &patches {
        if patch.source_end_ms <= patch.source_start_ms {
            return Err(format!(
                "Phase 4 refine shot {} has invalid source range [{}-{}].",
                patch.order_index, patch.source_start_ms, patch.source_end_ms
            ));
        }
    }
    Ok(patches)
}

fn parse_one_shot_patch(item: &Value) -> Result<Phase4ShotPatch, String> {
    if let Ok(patch) = serde_json::from_value::<Phase4ShotPatch>(item.clone()) {
        return Ok(patch);
    }
    let order_index = item
        .get("orderIndex")
        .and_then(Value::as_i64)
        .ok_or_else(|| "Phase 4 refine shot is missing orderIndex.".to_owned())?;
    let source_start_ms = item
        .get("sourceStartMs")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("Phase 4 refine shot {order_index} is missing sourceStartMs."))?;
    let source_end_ms = item
        .get("sourceEndMs")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("Phase 4 refine shot {order_index} is missing sourceEndMs."))?;
    Ok(Phase4ShotPatch {
        order_index,
        source_start_ms,
        source_end_ms,
        crop_focus: item
            .get("cropFocus")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        narration_text: item
            .get("narrationText")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        on_screen_text: item
            .get("onScreenText")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

fn validate_batch_orders(
    label: &str,
    returned: impl IntoIterator<Item = i64>,
    allowed: &HashSet<i64>,
) -> Result<(), String> {
    let mut seen = HashSet::new();
    for order in returned {
        if !allowed.contains(&order) {
            return Err(format!(
                "{label} returned shot {order} which is outside the current batch."
            ));
        }
        if !seen.insert(order) {
            return Err(format!("{label} returned duplicate shot {order}."));
        }
    }
    let mut missing = allowed.difference(&seen).copied().collect::<Vec<_>>();
    if !missing.is_empty() {
        missing.sort_unstable();
        return Err(format!(
            "{label} omitted required shot(s) {:?}; batch is not complete.",
            missing
        ));
    }
    Ok(())
}

fn apply_shot_patches(
    content: &mut StoryboardContent,
    patches: &[Phase4ShotPatch],
    locked: &StoryboardContent,
) -> Result<(), String> {
    for patch in patches {
        let Some(original) = locked
            .shots
            .iter()
            .find(|shot| shot.order_index == patch.order_index)
        else {
            return Err(format!(
                "Phase 4 refine returned unknown shot {}.",
                patch.order_index
            ));
        };
        let Some(slot) = content
            .shots
            .iter_mut()
            .find(|shot| shot.order_index == patch.order_index)
        else {
            return Err(format!(
                "Phase 4 draft is missing locked shot {}.",
                patch.order_index
            ));
        };
        if slot.asset_id != original.asset_id {
            slot.asset_id = original.asset_id.clone();
        }
        slot.beat_id = original.beat_id.clone();
        slot.segment_id = original.segment_id.clone();
        slot.source_start_ms = patch.source_start_ms;
        slot.source_end_ms = patch.source_end_ms;
        slot.duration_ms = (patch.source_end_ms - patch.source_start_ms).max(1);
        if let Some(crop) = patch.crop_focus {
            slot.crop_focus = Some(crop);
        }
        if let Some(narration) = &patch.narration_text {
            slot.narration_text = narration.clone();
        }
        if let Some(on_screen) = &patch.on_screen_text {
            slot.on_screen_text = on_screen.clone();
        }
    }
    Ok(())
}

pub(crate) fn is_structural_phase4_issue(kind: &str) -> bool {
    matches!(
        kind,
        "shot_count_changed"
            | "asset_swapped"
            | "empty_shot_list"
            | "consecutive_duplicate_asset"
            | "asset_over_diversity_limit"
            | "beat_order_broken"
            | "outside_candidate_pool"
    )
}

pub(crate) fn issues_need_window_repick(issues: &[StoryboardIssue]) -> bool {
    issues
        .iter()
        .any(|issue| issue.kind == "beat_audio_window_shortfall")
}

pub(crate) fn issue_from_validation_error(error: String) -> StoryboardIssue {
    let affected = parse_affected_shot_indices(&error);
    let kind = if error.starts_with("beat_audio_timing:") {
        "beat_audio_timing"
    } else if error.contains("overlapping video source") {
        "source_range_overlap"
    } else if error.contains("invalid video time range")
        || error.contains("duration exceeds its verified")
    {
        "invalid_source_range"
    } else {
        "validation"
    };
    StoryboardIssue::new(kind, error, true).for_shots(affected)
}

/// 由问题自己提供的 `affected_shots` 确定修复集合；必要时显式扩大到冲突/同 beat 镜头。
pub(crate) fn repair_set_from_issues(
    issues: &[StoryboardIssue],
    content: &StoryboardContent,
) -> Result<HashSet<i64>, String> {
    let mut set = HashSet::new();
    let mut saw_repairable = false;
    for issue in issues {
        if !issue.needs_model_decision || is_structural_phase4_issue(&issue.kind) {
            continue;
        }
        if issue.kind == "request_failed" {
            continue;
        }
        saw_repairable = true;
        if issue.affected_shots.is_empty() {
            return Err(format!(
                "Phase 4 cannot localize repair for {} ({}); refusing to redo all shots.",
                issue.kind, issue.message
            ));
        }
        for order in &issue.affected_shots {
            if content.shots.iter().any(|shot| shot.order_index == *order) {
                set.insert(*order);
            }
        }
        match issue.kind.as_str() {
            "beat_audio_timing" | "beat_audio_window_shortfall" => {
                expand_same_beat_shots(&mut set, content);
            }
            "source_range_overlap" => {
                expand_overlap_partners(&mut set, content);
            }
            _ => {}
        }
    }
    if saw_repairable && set.is_empty() {
        return Err(
            "Phase 4 repairable issues did not name any existing shots; refusing to redo all shots."
                .to_owned(),
        );
    }
    Ok(set)
}

fn expand_same_beat_shots(set: &mut HashSet<i64>, content: &StoryboardContent) {
    let beat_ids = content
        .shots
        .iter()
        .filter(|shot| set.contains(&shot.order_index))
        .map(|shot| shot.beat_id.clone())
        .collect::<HashSet<_>>();
    for shot in &content.shots {
        if beat_ids.contains(&shot.beat_id) {
            set.insert(shot.order_index);
        }
    }
}

fn expand_overlap_partners(set: &mut HashSet<i64>, content: &StoryboardContent) {
    let flagged = content
        .shots
        .iter()
        .filter(|shot| set.contains(&shot.order_index))
        .cloned()
        .collect::<Vec<_>>();
    for flagged_shot in flagged {
        for other in &content.shots {
            if other.order_index == flagged_shot.order_index {
                continue;
            }
            if other.asset_id == flagged_shot.asset_id
                && other.source_start_ms < flagged_shot.source_end_ms
                && flagged_shot.source_start_ms < other.source_end_ms
            {
                set.insert(other.order_index);
            }
        }
    }
}

pub(crate) fn patch_response_prompt() -> &'static str {
    "Return JSON only for THIS batch: {\"shots\":[{\"orderIndex\":7,\"sourceStartMs\":12000,\"sourceEndMs\":15000,\"cropFocus\":[0.5,0.4],\"narrationText\":\"optional\",\"onScreenText\":\"optional\"}]}. \
     Rust keeps assetId, beatId, segmentId, shot order and computes durationMs from the endpoints. \
     Include narrationText/onScreenText only when this batch must change them (split beat narration; key_message lead marker). \
     Every orderIndex listed for this batch MUST appear exactly once. Do not return shots from other batches."
}

/// 用脚本化模型响应驱动待处理批次：每批只请求一次，成功立刻写入会话。
#[cfg(test)]
pub(crate) fn run_pending_scripted_batches(
    session: &mut Phase4Session,
    pass: Phase4Pass,
    locked: &StoryboardContent,
    mut responder: impl FnMut(&[i64]) -> Result<String, String>,
) -> Result<(), String> {
    let batches: Vec<Vec<i64>> = match pass {
        Phase4Pass::B => session.pending_pass_b_batches(),
        Phase4Pass::C => session
            .pending_pass_c_batches()
            .into_iter()
            .map(|batch| batch.into_iter().map(|(order, _)| order).collect())
            .collect(),
        Phase4Pass::A => {
            return Err("scripted Pass A uses asset batches, not shot batches.".to_owned())
        }
    };
    let mut first_error = None;
    for batch in batches {
        session.record_call(pass, batch.clone());
        match responder(&batch) {
            Ok(text) => {
                let result = match pass {
                    Phase4Pass::B => session.complete_pass_b_batch(&batch, &text, locked),
                    Phase4Pass::C => session.complete_pass_c_batch(&batch, &text, locked),
                    Phase4Pass::A => unreachable!(),
                };
                if let Err(error) = result {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
pub(crate) fn shot_patch_json(shots: &[StoryboardShot], orders: &[i64]) -> String {
    let items = orders
        .iter()
        .filter_map(|order| {
            shots
                .iter()
                .find(|shot| shot.order_index == *order)
                .map(|shot| {
                    json!({
                        "orderIndex": shot.order_index,
                        "sourceStartMs": shot.source_start_ms,
                        "sourceEndMs": shot.source_end_ms,
                        "cropFocus": shot.crop_focus,
                        "narrationText": shot.narration_text,
                        "onScreenText": shot.on_screen_text,
                    })
                })
        })
        .collect::<Vec<_>>();
    json!({ "shots": items }).to_string()
}

pub(crate) fn phase4_refine_ranges(
    app: &AppHandle,
    access: &ModelAccess,
    brief: &str,
    selected: &StoryboardContent,
    rough: &RoughStoryboard,
    sources: &[StoryboardSource],
    repair: Option<&RepairPacket>,
    session: &mut Phase4Session,
) -> Result<(StoryboardContent, Vec<StoryboardIssue>), String> {
    log::info!(
        "Phase 4: window-select then in-window refine for {} locked shots (local progress retained in-session)",
        selected.shots.len()
    );
    session.ensure_initialized(selected, sources)?;
    session.apply_repair_if_needed(repair, selected)?;
    let feedback_context = repair.map_or(String::new(), repair_packet_prompt_block);
    let attempt = repair.map(|packet| packet.attempt).unwrap_or(1);

    run_pending_pass_a(
        app,
        access,
        brief,
        selected,
        session,
        &feedback_context,
        attempt,
    )?;
    if !session.pass_a_is_complete() {
        return Err("Phase 4a still has incomplete window picks.".to_owned());
    }
    session.seed_ranges_from_windows(selected);

    run_pending_pass_b(
        app,
        access,
        brief,
        selected,
        rough,
        session,
        &feedback_context,
        attempt,
    )?;
    if !session.pass_b_is_complete() {
        return Err("Phase 4b still has incomplete refine batches.".to_owned());
    }

    session.plan_pass_c_if_needed(PHASE4_MAX_FRAME_SPACING_MS, PHASE4_REFINE_FRAMES);
    run_pending_pass_c(app, access, selected, session, attempt)?;
    if !session.pass_c_is_complete() {
        return Err("Phase 4c still has incomplete densify batches.".to_owned());
    }

    let mut refined = session
        .content()
        .cloned()
        .ok_or_else(|| "Phase 4 session lost its draft.".to_owned())?;
    let pick_map = session.pick_map().clone();
    let mutable = session.mutable_orders();
    clamp_shots_to_chosen_windows_scoped(&mut refined, &pick_map, mutable.as_ref());
    if rough.speech_timing.beats.is_empty() {
        apply_narration_phrase_duration_floor_scoped(&mut refined, &pick_map, mutable.as_ref());
    }
    refined.uncovered_beat_ids = selected.uncovered_beat_ids.clone();
    let mut issues = crate::storyboard::timing::fit_shots_scoped(
        &mut refined,
        &rough.speech_timing,
        &pick_map,
        mutable.as_ref(),
    );
    resolve_overlaps_within_chosen_windows_scoped(&mut refined, &pick_map, mutable.as_ref());
    crate::execution_deadline::check()?;
    issues.extend(collect_phase4_issues(&mut refined, selected));
    refined.brief = brief.to_owned();
    refined.title = rough.title.clone();
    refined.summary = rough.summary.clone();
    refined.target_duration_ms = rough.target_duration_ms;
    refined.script_mode = rough.script_mode.clone();
    refined.beats = rough.beats.clone();
    refined.uncovered_beat_ids = selected.uncovered_beat_ids.clone();
    session.replace_content(refined.clone());
    log::info!(
        "Phase 4 refine complete: shots={}, issues={}, repair={:?}",
        refined.shots.len(),
        issues.len(),
        {
            let mut orders = session.repair_shots().iter().copied().collect::<Vec<_>>();
            orders.sort_unstable();
            orders
        }
    );
    Ok((refined, issues))
}

fn run_pending_pass_a(
    app: &AppHandle,
    access: &ModelAccess,
    brief: &str,
    selected: &StoryboardContent,
    session: &mut Phase4Session,
    feedback_context: &str,
    attempt: usize,
) -> Result<(), String> {
    let selected_sources = session.selected_sources().to_vec();
    let all_windows = session.all_windows().to_vec();
    let batches = session.pending_pass_a_asset_batches();
    if batches.is_empty() {
        return Ok(());
    }
    let mut batch_error = None;
    let batch_count = batches.len();
    for (batch_index, batch_assets) in batches.iter().enumerate() {
        crate::execution_deadline::check()?;
        let batch_asset_set = batch_assets.iter().cloned().collect::<HashSet<_>>();
        let pending_orders = session.pass_a_orders_for_assets(batch_assets);
        if pending_orders.is_empty() {
            continue;
        }
        let batch_windows = all_windows
            .iter()
            .filter(|window| batch_asset_set.contains(&window.asset_id))
            .cloned()
            .collect::<Vec<_>>();
        let batch_shots = selected
            .shots
            .iter()
            .filter(|shot| pending_orders.contains(&shot.order_index))
            .map(|shot| {
                json!({
                    "orderIndex": shot.order_index,
                    "assetId": shot.asset_id,
                    "beatId": shot.beat_id,
                    "purpose": shot.purpose,
                    "narrationText": shot.narration_text,
                    "durationMs": shot.duration_ms,
                })
            })
            .collect::<Vec<_>>();
        let window_cards = serde_json::to_string(&batch_windows)
            .map_err(|_| "Could not serialize Phase 4 windows.".to_owned())?;
        let shot_cards = serde_json::to_string(&batch_shots)
            .map_err(|_| "Could not serialize Phase 4 shots.".to_owned())?;
        let mut pass_a_blocks = vec![json!({
            "type": "input_text",
            "text": format!(
                "Brief: {brief}\n\
                Pass A batch {}/{batch_count} — refine window picks ONLY for these assetIds: {:?}.\n\
                Locked shots for this batch (assetIds FINAL): {shot_cards}\n\
                Content windows for this batch: {window_cards}\n\
                {feedback_context}\n\n\
                Each attached image is the midpoint of one windowId (windows come from import keyframes / head-mid-tail thirds, not a fresh full-clip scene scan).\n\
                For EVERY locked shot in this batch, pick exactly one windowId that belongs to that shot's assetId.\n\
                Prefer actionable/on-brief content over setup/prelude/idle when both exist.\n\
                Set uncertain=true when the best window is ambiguous or you may cut mid spoken phrase.\n\
                Return JSON only: {{\"picks\":[{{\"orderIndex\":1,\"windowId\":\"asset:w0\",\"uncertain\":false}}]}}. Every orderIndex in this batch must appear exactly once.",
                batch_index + 1,
                batch_assets
            )
        })];
        let mut batch_frame_count = 0usize;
        for window in &batch_windows {
            let Some(source) = selected_sources
                .iter()
                .find(|source| source.asset_id == window.asset_id)
            else {
                continue;
            };
            let Some(path) = source.source_path.as_deref() else {
                continue;
            };
            let key = MaterialKey::pass_a(&window.window_id, window.mid_ms());
            if let Some(cached) = session.cached_blocks(&key).cloned() {
                batch_frame_count += cached
                    .iter()
                    .filter(|block| {
                        block.get("type").and_then(Value::as_str) == Some("input_image")
                    })
                    .count();
                pass_a_blocks.extend(cached);
                continue;
            }
            let frames = extract_frames_at_times(
                app,
                &window.asset_id,
                Path::new(path),
                &[window.mid_ms()],
                &format!(
                    "passA_b{}_{}",
                    batch_index + 1,
                    window.window_id.replace(':', "_")
                ),
            );
            let mut stored = Vec::new();
            for (time_ms, frame_path) in frames {
                let Some(image) = read_input_image(&frame_path) else {
                    continue;
                };
                let caption = json!({
                    "type": "input_text",
                    "text": format!(
                        "windowId={} assetId={} [{},{}] midTimeMs={}",
                        window.window_id, window.asset_id, window.start_ms, window.end_ms, time_ms
                    )
                });
                stored.push(caption.clone());
                stored.push(image.clone());
                pass_a_blocks.push(caption);
                pass_a_blocks.push(image);
                batch_frame_count += 1;
            }
            if !stored.is_empty() {
                session.store_blocks(key, stored);
            }
        }
        if batch_frame_count == 0 {
            let error = format!(
                "Phase 4a batch {} had no readable window midpoint frames.",
                batch_index + 1
            );
            if batch_error.is_none() {
                batch_error = Some(error);
            }
            continue;
        }
        log::info!(
            "Phase 4 pass A batch {}/{batch_count}: {} asset(s), {} midpoint frame(s), pending shots={:?}",
            batch_index + 1,
            batch_assets.len(),
            batch_frame_count,
            pending_orders
        );
        session.record_call(Phase4Pass::A, pending_orders);
        let pass_a_request = json!({
            "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
            "store": false,
            "stream": true,
            "input": [{ "role": "user", "content": pass_a_blocks }],
            "text": { "format": { "type": "json_object" } }
        });
        crate::storyboard::provider_trace::append_storyboard_trace(
            "Phase 4a",
            None,
            attempt,
            "request",
            &pass_a_request,
        );
        match post_model_payload(access, &pass_a_request, Some(STORYBOARD_TIMEOUT)) {
            Ok(pass_a_body) => {
                crate::storyboard::provider_trace::append_storyboard_trace(
                    "Phase 4a",
                    None,
                    attempt,
                    "response",
                    &serde_json::from_str::<Value>(&pass_a_body)
                        .unwrap_or_else(|_| json!({ "raw": pass_a_body })),
                );
                let pass_a_text = match model_response_json_text(access, &pass_a_body) {
                    Some(text) => text,
                    None => {
                        if batch_error.is_none() {
                            batch_error =
                                Some("Phase 4a response did not contain JSON.".to_owned());
                        }
                        continue;
                    }
                };
                if let Err(error) =
                    session.complete_pass_a_batch(batch_assets, &pass_a_text, selected)
                {
                    if batch_error.is_none() {
                        batch_error = Some(error);
                    }
                }
            }
            Err(error) => {
                if batch_error.is_none() {
                    batch_error = Some(error);
                }
            }
        }
    }
    first_error(batch_error)
}

fn run_pending_pass_b(
    app: &AppHandle,
    access: &ModelAccess,
    brief: &str,
    selected: &StoryboardContent,
    rough: &RoughStoryboard,
    session: &mut Phase4Session,
    feedback_context: &str,
    attempt: usize,
) -> Result<(), String> {
    let selected_sources = session.selected_sources().to_vec();
    let batches = session.pending_pass_b_batches();
    if batches.is_empty() {
        return Ok(());
    }
    let mut batch_error = None;
    let batch_count = batches.len();
    for (batch_index, batch) in batches.iter().enumerate() {
        crate::execution_deadline::check()?;
        let draft_json = session
            .content()
            .and_then(|content| serde_json::to_string(content).ok())
            .unwrap_or_else(|| "{}".to_owned());
        let pick_map = session.pick_map().clone();
        let mut pass_b_blocks = vec![json!({
            "type": "input_text",
            "text": format!(
                "Brief: {brief}\n\
                Batch {}/{batch_count} — refine ONLY these orderIndex values: {:?}.\n\
                Current storyboard draft (assetIds FINAL; Rust keeps identity/order): {draft_json}\n\
                Beat timing (milliseconds; empty means not available): {}\n\
                Chosen windows for this batch: {}\n\
                {feedback_context}\n\n\
                Each attached image is ONE shot's densified window as a left-to-right, top-to-bottom frame grid. Caption lists timesMs in the same order — temporal sample count is unchanged.\n\
                Refine sourceStartMs/sourceEndMs inside that window so the span best matches purpose/requiredVisual.\n\
                Do NOT cut mid spoken phrase in narrationText — prefer natural phrase boundaries.\n\
                Keep durationMs = sourceEndMs - sourceStartMs. No asset swaps, no add/remove/reorder shots.\n\
                No overlapping ranges from the same asset. When beat timing is supplied, the total duration of each beat must equal its endMs-startMs; prefer its verified pausesMs for internal cuts while preserving complete visual actions. Otherwise approach targetDurationMs.\n\
                Divide beat narration across shots when needed.\n\
                For scriptMode=key_message, keep lead-shot onScreenText equal to that beat's onScreenText marker; leave narrationText empty.\n\
                Choose cropFocus from the timed frames so the subject remains inside a 9:16 crop throughout the chosen source range. Prefer complete actions and coherent screen direction at adjacent cuts.\n\
                {}\n\
                matchLevel must stay 'direct' or 'contextual'.",
                batch_index + 1,
                batch,
                serde_json::to_string(&rough.speech_timing).unwrap_or_else(|_| "{}".to_owned()),
                serde_json::to_string(
                    &batch
                        .iter()
                        .filter_map(|order| {
                            pick_map.get(order).map(|(window, uncertain)| json!({
                                "orderIndex": order,
                                "windowId": window.window_id,
                                "startMs": window.start_ms,
                                "endMs": window.end_ms,
                                "uncertain": uncertain
                            }))
                        })
                        .collect::<Vec<_>>()
                )
                .unwrap_or_else(|_| "[]".to_owned()),
                patch_response_prompt()
            )
        })];
        let mut batch_images = 0usize;
        for order_index in batch {
            let Some((window, _)) = pick_map.get(order_index) else {
                continue;
            };
            let Some(source) = selected_sources
                .iter()
                .find(|source| source.asset_id == window.asset_id)
            else {
                continue;
            };
            let Some(path) = source.source_path.as_deref() else {
                continue;
            };
            let key = MaterialKey::pass_b(
                *order_index,
                &window.window_id,
                window.start_ms,
                window.end_ms,
            );
            if let Some(cached) = session.cached_blocks(&key).cloned() {
                batch_images += cached
                    .iter()
                    .filter(|block| {
                        block.get("type").and_then(Value::as_str) == Some("input_image")
                    })
                    .count();
                pass_b_blocks.extend(cached);
                continue;
            }
            let times =
                densify_times_in_range(window.start_ms, window.end_ms, PHASE4_REFINE_FRAMES);
            let frames = extract_frames_at_times(
                app,
                &window.asset_id,
                Path::new(path),
                &times,
                &format!("passB_{order_index}"),
            );
            crate::execution_deadline::check()?;
            if frames.is_empty() {
                continue;
            }
            let times_ms = frames.iter().map(|(time, _)| *time).collect::<Vec<_>>();
            let frame_paths = frames
                .iter()
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>();
            let Some(grid_path) = frames[0]
                .1
                .parent()
                .map(|parent| parent.join(format!("grid_shot_{order_index}.jpg")))
            else {
                continue;
            };
            let Some(grid) = compose_timed_frame_grid(&frame_paths, &grid_path, 3) else {
                continue;
            };
            let Some(image) = read_input_image(&grid) else {
                continue;
            };
            let caption = json!({
                "type": "input_text",
                "text": format!(
                    "shotOrderIndex={} windowId={} gridColumns=3 timesMs={:?} (read cells L→R, T→B)",
                    order_index, window.window_id, times_ms
                )
            });
            let stored = vec![caption.clone(), image.clone()];
            session.store_blocks(key, stored.clone());
            pass_b_blocks.push(caption);
            pass_b_blocks.push(image);
            batch_images += 1;
        }
        if batch_images == 0 {
            let error = format!(
                "Phase 4b batch {} had no readable in-window frame grids.",
                batch_index + 1
            );
            if batch_error.is_none() {
                batch_error = Some(error);
            }
            continue;
        }
        log::info!(
            "Phase 4 pass B batch {}/{batch_count}: refining {} shot(s) as {batch_images} grid image(s)",
            batch_index + 1,
            batch.len()
        );
        session.record_call(Phase4Pass::B, batch.clone());
        let pass_b_request = json!({
            "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
            "store": false,
            "stream": true,
            "input": [{ "role": "user", "content": pass_b_blocks }],
            "text": { "format": { "type": "json_object" } }
        });
        crate::storyboard::provider_trace::append_storyboard_trace(
            "Phase 4b",
            None,
            attempt,
            "request",
            &pass_b_request,
        );
        match post_model_payload(access, &pass_b_request, Some(STORYBOARD_TIMEOUT)) {
            Ok(pass_b_body) => {
                crate::storyboard::provider_trace::append_storyboard_trace(
                    "Phase 4b",
                    None,
                    attempt,
                    "response",
                    &serde_json::from_str::<Value>(&pass_b_body)
                        .unwrap_or_else(|_| json!({ "raw": pass_b_body })),
                );
                let pass_b_text = match model_response_json_text(access, &pass_b_body) {
                    Some(text) => text,
                    None => {
                        if batch_error.is_none() {
                            batch_error =
                                Some("Phase 4b response did not contain JSON.".to_owned());
                        }
                        continue;
                    }
                };
                if let Err(error) = session.complete_pass_b_batch(batch, &pass_b_text, selected) {
                    if batch_error.is_none() {
                        batch_error = Some(error);
                    }
                }
            }
            Err(error) => {
                if batch_error.is_none() {
                    batch_error = Some(error);
                }
            }
        }
    }
    first_error(batch_error)
}

fn run_pending_pass_c(
    app: &AppHandle,
    access: &ModelAccess,
    selected: &StoryboardContent,
    session: &mut Phase4Session,
    attempt: usize,
) -> Result<(), String> {
    let selected_sources = session.selected_sources().to_vec();
    let batches = session.pending_pass_c_batches();
    if batches.is_empty() {
        return Ok(());
    }
    let mut batch_error = None;
    let batch_count = batches.len();
    for (batch_index, batch) in batches.iter().enumerate() {
        crate::execution_deadline::check()?;
        let batch_orders = batch.iter().map(|(order, _)| *order).collect::<Vec<_>>();
        let draft_json = session
            .content()
            .and_then(|content| serde_json::to_string(content).ok())
            .unwrap_or_else(|| "{}".to_owned());
        let pick_map = session.pick_map().clone();
        let mut pass_c_blocks = vec![json!({
            "type": "input_text",
            "text": format!(
                "Re-check ONLY these shots with denser frame grids: {:?}.\n\
                Batch {}/{batch_count}.\n\
                Current storyboard: {draft_json}\n\
                assetIds stay FINAL. Captions mark UNCERTAIN (full chosen window) or NARROW (tighten inside the listed range only).\n\
                For NARROW shots, keep sourceStartMs/sourceEndMs inside the NARROW range; prefer complete actions and natural phrase boundaries.\n\
                Each image is one shot's denser grid; caption lists timesMs L→R, T→B.\n\
                Avoid cutting mid spoken phrase.\n\
                {}",
                batch_orders,
                batch_index + 1,
                patch_response_prompt()
            )
        })];
        let mut attached = 0usize;
        for (order_index, mode) in batch {
            let Some((window, _)) = pick_map.get(order_index) else {
                continue;
            };
            let Some(source) = selected_sources
                .iter()
                .find(|source| source.asset_id == window.asset_id)
            else {
                continue;
            };
            let Some(path) = source.source_path.as_deref() else {
                continue;
            };
            let (sample_start, sample_end, caption_prefix) = match mode {
                PassCMode::Uncertain => (
                    window.start_ms,
                    window.end_ms,
                    format!(
                        "UNCERTAIN shotOrderIndex={} windowId={}",
                        order_index, window.window_id
                    ),
                ),
                PassCMode::Narrow => {
                    let shot = session.content().and_then(|content| {
                        content
                            .shots
                            .iter()
                            .find(|shot| shot.order_index == *order_index)
                    });
                    let Some(shot) = shot else {
                        continue;
                    };
                    let span = (shot.source_end_ms - shot.source_start_ms).max(1);
                    let pad = (span / 4).max(500);
                    let sample_start = shot
                        .source_start_ms
                        .saturating_sub(pad)
                        .max(window.start_ms);
                    let sample_end = (shot.source_end_ms + pad).min(window.end_ms);
                    (
                        sample_start,
                        sample_end.max(sample_start + 1),
                        format!(
                            "NARROW shotOrderIndex={} windowId={} narrowRangeMs=[{},{}]",
                            order_index,
                            window.window_id,
                            sample_start,
                            sample_end.max(sample_start + 1)
                        ),
                    )
                }
            };
            let key = MaterialKey::pass_c(
                *order_index,
                &window.window_id,
                sample_start,
                sample_end,
                PHASE4_UNCERTAIN_FRAMES,
            );
            if let Some(cached) = session.cached_blocks(&key).cloned() {
                attached += cached
                    .iter()
                    .filter(|block| {
                        block.get("type").and_then(Value::as_str) == Some("input_image")
                    })
                    .count();
                pass_c_blocks.extend(cached);
                continue;
            }
            let times = densify_times_in_range(sample_start, sample_end, PHASE4_UNCERTAIN_FRAMES);
            let frames = extract_frames_at_times(
                app,
                &window.asset_id,
                Path::new(path),
                &times,
                &format!("passC_{order_index}"),
            );
            crate::execution_deadline::check()?;
            if frames.is_empty() {
                continue;
            }
            let times_ms = frames.iter().map(|(time, _)| *time).collect::<Vec<_>>();
            let frame_paths = frames
                .iter()
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>();
            let Some(grid_path) = frames[0]
                .1
                .parent()
                .map(|parent| parent.join(format!("grid_passC_{order_index}.jpg")))
            else {
                continue;
            };
            let Some(grid) = compose_timed_frame_grid(&frame_paths, &grid_path, 5) else {
                continue;
            };
            let Some(image) = read_input_image(&grid) else {
                continue;
            };
            let caption = json!({
                "type": "input_text",
                "text": format!(
                    "{caption_prefix} gridColumns=5 timesMs={:?} (read cells L→R, T→B)",
                    times_ms
                )
            });
            let stored = vec![caption.clone(), image.clone()];
            session.store_blocks(key, stored.clone());
            pass_c_blocks.push(caption);
            pass_c_blocks.push(image);
            attached += 1;
        }
        if attached == 0 {
            let error = format!(
                "Phase 4c batch {} had no readable densified frame grids.",
                batch_index + 1
            );
            if batch_error.is_none() {
                batch_error = Some(error);
            }
            continue;
        }
        log::info!(
            "Phase 4 pass C batch {}/{batch_count}: densifying {} shot(s)",
            batch_index + 1,
            batch_orders.len()
        );
        session.record_call(Phase4Pass::C, batch_orders.clone());
        let pass_c_request = json!({
            "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
            "store": false,
            "stream": true,
            "input": [{ "role": "user", "content": pass_c_blocks }],
            "text": { "format": { "type": "json_object" } }
        });
        crate::storyboard::provider_trace::append_storyboard_trace(
            "Phase 4c",
            None,
            attempt,
            "request",
            &pass_c_request,
        );
        match post_model_payload(access, &pass_c_request, Some(STORYBOARD_TIMEOUT)) {
            Ok(pass_c_body) => {
                crate::storyboard::provider_trace::append_storyboard_trace(
                    "Phase 4c",
                    None,
                    attempt,
                    "response",
                    &serde_json::from_str::<Value>(&pass_c_body)
                        .unwrap_or_else(|_| json!({ "raw": pass_c_body })),
                );
                let Some(text) = model_response_json_text(access, &pass_c_body) else {
                    if batch_error.is_none() {
                        batch_error = Some("Phase 4c response did not contain JSON.".to_owned());
                    }
                    continue;
                };
                if let Err(error) = session.complete_pass_c_batch(&batch_orders, &text, selected) {
                    if batch_error.is_none() {
                        batch_error = Some(error);
                    }
                }
            }
            Err(error) => {
                if batch_error.is_none() {
                    batch_error = Some(error);
                }
            }
        }
    }
    first_error(batch_error)
}

fn first_error(batch_error: Option<String>) -> Result<(), String> {
    match batch_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{StoryboardBeat, StoryboardShot};
    use crate::storyboard::step_retry::StepRetryBudget;

    fn shot(order: i64, asset: &str, beat: &str) -> StoryboardShot {
        StoryboardShot {
            crop_focus: Some([0.1 * order as f64, 0.4]),
            order_index: order,
            duration_ms: 1_000,
            purpose: "purpose".to_owned(),
            on_screen_text: String::new(),
            narration_text: format!("n{order}"),
            asset_id: asset.to_owned(),
            source_start_ms: (order - 1) * 2_000,
            source_end_ms: (order - 1) * 2_000 + 1_000,
            reason: "reason".to_owned(),
            beat_id: beat.to_owned(),
            match_level: "direct".to_owned(),
            beat_part_index: 1,
            beat_part_count: 1,
            split_role: "lead".to_owned(),
            segment_id: None,
        }
    }

    fn content_with_shots(count: i64) -> StoryboardContent {
        let shots = (1..=count)
            .map(|order| {
                let asset = format!("asset-{}", (order - 1) % 8);
                let beat = format!("beat-{}", (order - 1) / 3 + 1);
                shot(order, &asset, &beat)
            })
            .collect::<Vec<_>>();
        StoryboardContent {
            brief: String::new(),
            title: "t".to_owned(),
            summary: "s".to_owned(),
            target_duration_ms: count * 1_000,
            script_mode: "full_script".to_owned(),
            beats: vec![StoryboardBeat {
                id: "beat-1".to_owned(),
                purpose: "p".to_owned(),
                required_visual: "v".to_owned(),
                visual_keywords: Vec::new(),
                narration: "n".to_owned(),
                on_screen_text: String::new(),
            }],
            uncovered_beat_ids: Vec::new(),
            shots,
        }
    }

    fn session_ready_for_pass_b(count: i64) -> (Phase4Session, StoryboardContent) {
        let selected = content_with_shots(count);
        let mut session = Phase4Session::new();
        session.content = Some(selected.clone());
        session.initialized = true;
        for shot in &selected.shots {
            session.pick_map.insert(
                shot.order_index,
                (
                    Phase4ContentWindow {
                        window_id: format!("{}:w0", shot.asset_id),
                        asset_id: shot.asset_id.clone(),
                        start_ms: 0,
                        end_ms: 20_000,
                    },
                    false,
                ),
            );
            session.pass_a_done.insert(shot.order_index);
            session.pass_b_pending.insert(shot.order_index);
        }
        session.ranges_seeded = true;
        (session, selected)
    }

    fn ok_patches(selected: &StoryboardContent, orders: &[i64]) -> String {
        shot_patch_json(&selected.shots, orders)
    }

    #[test]
    fn failed_third_pass_b_batch_does_not_rerun_first_two() {
        let (mut session, selected) = session_ready_for_pass_b(25);
        let first =
            run_pending_scripted_batches(&mut session, Phase4Pass::B, &selected, |orders| {
                if *orders.last().unwrap_or(&0) <= 20 {
                    Ok(ok_patches(&selected, orders))
                } else {
                    Err("batch 3 timed out".to_owned())
                }
            });
        assert!(first.is_err());
        assert_eq!(
            session
                .model_calls
                .iter()
                .filter(|call| call.pass == Phase4Pass::B)
                .count(),
            3
        );
        assert!(session.pass_b_done.contains(&1) && session.pass_b_done.contains(&20));
        assert!(session.pass_b_pending.contains(&21));
        let crop_before = session.content.as_ref().unwrap().shots[0].crop_focus;
        let retry =
            run_pending_scripted_batches(&mut session, Phase4Pass::B, &selected, |orders| {
                Ok(ok_patches(&selected, orders))
            });
        assert!(retry.is_ok());
        let pass_b_calls = session
            .model_calls
            .iter()
            .filter(|call| call.pass == Phase4Pass::B)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(pass_b_calls.len(), 4);
        assert_eq!(pass_b_calls[0].orders, (1..=10).collect::<Vec<_>>());
        assert_eq!(pass_b_calls[1].orders, (11..=20).collect::<Vec<_>>());
        assert_eq!(pass_b_calls[2].orders, (21..=25).collect::<Vec<_>>());
        assert_eq!(pass_b_calls[3].orders, (21..=25).collect::<Vec<_>>());
        assert_eq!(
            session.content.as_ref().unwrap().shots[0].crop_focus,
            crop_before
        );
        assert!(session.pass_b_is_complete());
    }

    #[test]
    fn pass_c_failure_does_not_rerun_completed_pass_a_or_b() {
        let (mut session, selected) = session_ready_for_pass_b(3);
        run_pending_scripted_batches(&mut session, Phase4Pass::B, &selected, |orders| {
            Ok(ok_patches(&selected, orders))
        })
        .unwrap();
        session.pass_c_pending.insert(1);
        session.pass_c_pending.insert(2);
        session.pass_c_modes.insert(1, PassCMode::Uncertain);
        session.pass_c_modes.insert(2, PassCMode::Narrow);
        session.pass_c_planned = true;
        let first =
            run_pending_scripted_batches(&mut session, Phase4Pass::C, &selected, |orders| {
                if orders == [1, 2] || orders.contains(&2) && orders.len() > 1 {
                    Err("pass c failed".to_owned())
                } else if orders == [1] {
                    Ok(ok_patches(&selected, orders))
                } else {
                    Err("pass c failed".to_owned())
                }
            });
        assert!(first.is_err() || !session.pass_c_is_complete());
        let a_calls = session
            .model_calls
            .iter()
            .filter(|call| call.pass == Phase4Pass::A)
            .count();
        let b_calls = session
            .model_calls
            .iter()
            .filter(|call| call.pass == Phase4Pass::B)
            .count();
        assert_eq!(a_calls, 0);
        assert_eq!(b_calls, 1);
        assert!(session.pass_a_is_complete());
        assert!(session.pass_b_is_complete());
        let _ = run_pending_scripted_batches(&mut session, Phase4Pass::C, &selected, |orders| {
            Ok(ok_patches(&selected, orders))
        });
        assert_eq!(
            session
                .model_calls
                .iter()
                .filter(|call| call.pass == Phase4Pass::A)
                .count(),
            0
        );
        assert_eq!(
            session
                .model_calls
                .iter()
                .filter(|call| call.pass == Phase4Pass::B)
                .count(),
            1
        );
    }

    #[test]
    fn single_shot_repair_preserves_unrelated_crop_focus() {
        let (mut session, selected) = session_ready_for_pass_b(4);
        run_pending_scripted_batches(&mut session, Phase4Pass::B, &selected, |orders| {
            Ok(ok_patches(&selected, orders))
        })
        .unwrap();
        session.pass_c_planned = true;
        let original = session.content.as_ref().unwrap().shots.clone();
        let issues =
            vec![
                StoryboardIssue::new("invalid_source_range", "shot 3 range invalid", true)
                    .for_shots(vec![3]),
            ];
        let set = repair_set_from_issues(&issues, session.content.as_ref().unwrap()).unwrap();
        session.queue_repair(set, false, &selected);
        assert_eq!(session.repair_shots, HashSet::from([3]));
        run_pending_scripted_batches(&mut session, Phase4Pass::B, &selected, |orders| {
            assert_eq!(orders, &[3]);
            Ok(json!({
                "shots": [{
                    "orderIndex": 3,
                    "sourceStartMs": 8000,
                    "sourceEndMs": 9500,
                    "cropFocus": [0.7, 0.2]
                }]
            })
            .to_string())
        })
        .unwrap();
        let updated = session.content.as_ref().unwrap();
        assert_eq!(updated.shots[0].crop_focus, original[0].crop_focus);
        assert_eq!(updated.shots[1].crop_focus, original[1].crop_focus);
        assert_eq!(updated.shots[3].crop_focus, original[3].crop_focus);
        assert_eq!(updated.shots[2].crop_focus, Some([0.7, 0.2]));
        assert_eq!(updated.shots[2].source_start_ms, 8000);
        assert_eq!(updated.shots[2].duration_ms, 1500);
        assert_eq!(updated.shots[0].narration_text, original[0].narration_text);
    }

    #[test]
    fn overlap_and_beat_duration_expand_related_shots() {
        let selected = content_with_shots(6);
        let mut overlapping = selected.clone();
        overlapping.shots[0].asset_id = "shared".to_owned();
        overlapping.shots[0].source_start_ms = 1000;
        overlapping.shots[0].source_end_ms = 4000;
        overlapping.shots[2].asset_id = "shared".to_owned();
        overlapping.shots[2].source_start_ms = 2000;
        overlapping.shots[2].source_end_ms = 5000;
        overlapping.shots[0].beat_id = "beat-1".to_owned();
        overlapping.shots[1].beat_id = "beat-1".to_owned();
        overlapping.shots[2].beat_id = "beat-2".to_owned();
        let overlap = repair_set_from_issues(
            &[StoryboardIssue::new("source_range_overlap", "overlap", true).for_shots(vec![1])],
            &overlapping,
        )
        .unwrap();
        assert!(overlap.contains(&1) && overlap.contains(&3));
        assert!(!overlap.contains(&2));
        overlapping.shots[0].beat_id = "beat-a".to_owned();
        overlapping.shots[1].beat_id = "beat-a".to_owned();
        overlapping.shots[2].beat_id = "beat-a".to_owned();
        let beat = repair_set_from_issues(
            &[StoryboardIssue::new("beat_audio_timing", "short", true).for_shots(vec![2])],
            &overlapping,
        )
        .unwrap();
        assert_eq!(beat, HashSet::from([1, 2, 3]));
    }

    #[test]
    fn missing_shot_response_is_not_success() {
        let (mut session, selected) = session_ready_for_pass_b(3);
        let error = session
            .complete_pass_b_batch(
                &[1, 2, 3],
                &json!({
                    "shots": [{
                        "orderIndex": 1,
                        "sourceStartMs": 0,
                        "sourceEndMs": 1000
                    }]
                })
                .to_string(),
                &selected,
            )
            .unwrap_err();
        assert!(error.contains("omitted"));
        assert!(session.pass_b_pending.contains(&1));
        assert!(session.pass_b_pending.contains(&2));
        assert!(!session.pass_b_done.contains(&1));
        let extra = parse_shot_patches_for_batch(
            &json!({
                "shots": [
                    {"orderIndex": 1, "sourceStartMs": 0, "sourceEndMs": 1000},
                    {"orderIndex": 9, "sourceStartMs": 0, "sourceEndMs": 1000}
                ]
            })
            .to_string(),
            &HashSet::from([1]),
        )
        .unwrap_err();
        assert!(extra.contains("outside the current batch"));
    }

    #[test]
    fn retry_budget_is_not_multiplied_by_batch_count() {
        let (mut session, selected) = session_ready_for_pass_b(25);
        let mut budget = StepRetryBudget::new("Phase 4");
        let first =
            run_pending_scripted_batches(&mut session, Phase4Pass::B, &selected, |orders| {
                if *orders.last().unwrap_or(&0) <= 20 {
                    Ok(ok_patches(&selected, orders))
                } else {
                    Err("timeout".to_owned())
                }
            });
        assert!(first.is_err());
        budget.record_semantic_failure();
        assert_eq!(budget.semantic_used(), 1);
        assert!(budget.can_retry_semantic());
        run_pending_scripted_batches(&mut session, Phase4Pass::B, &selected, |orders| {
            Ok(ok_patches(&selected, orders))
        })
        .unwrap();
        assert_eq!(
            session
                .model_calls
                .iter()
                .filter(|call| call.pass == Phase4Pass::B)
                .count(),
            4
        );
        assert_eq!(budget.semantic_used(), 1);
        assert!(budget.semantic_used() < 25);
    }

    #[test]
    fn unlocalized_issue_does_not_default_to_all_shots() {
        let selected = content_with_shots(4);
        let error = repair_set_from_issues(
            &[StoryboardIssue::new("validation", "mystery", true)],
            &selected,
        )
        .unwrap_err();
        assert!(error.contains("cannot localize"));
        assert!(!error.contains("redo all") || error.contains("refusing"));
    }

    #[test]
    fn parse_affected_indices_from_validation_message() {
        let issue = issue_from_validation_error(
            "Storyboard cannot reuse overlapping video source ranges across beats. Affected shot indices: 4, 9.".to_owned(),
        );
        assert_eq!(issue.kind, "source_range_overlap");
        assert_eq!(issue.affected_shots, vec![4, 9]);
    }

    #[test]
    fn window_pick_batch_requires_every_shot() {
        let allowed = HashSet::from([1, 2]);
        let mut windows = HashMap::new();
        windows.insert(1, HashSet::from(["a:w0".to_owned()]));
        windows.insert(2, HashSet::from(["b:w0".to_owned()]));
        let error = parse_window_picks_for_batch(
            r#"{"picks":[{"orderIndex":1,"windowId":"a:w0","uncertain":false}]}"#,
            &allowed,
            &windows,
        )
        .unwrap_err();
        assert!(error.contains("omitted"));
    }
}
