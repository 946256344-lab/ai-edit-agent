//! 选镜局部编辑：在当前时间线上只重选指定 beat（P2→P3→P4→P5）或只精修指定镜头（P4→P5），其余镜头冻结。
//! 拍时长取自当前时间线（配音对齐已落在 clip 时长里），配音 / 字幕 / 音乐 / 叠加轨原样继承。
//! 产物是派生 storyboard 版本 + 新 timeline 版本，同一事务提交；任何一步不过就不写入并返回真实原因。

use super::phases::{self, BeatCandidatePool, RoughStoryboard};
use super::timing::{BeatTiming, SpeechTiming, SpeechTimingKind};
use crate::models::{
    StoryboardContent, StoryboardDerivation, StoryboardShot, StoryboardSource, StoryboardVersion,
    TimelineClip, TimelineVersion,
};
use crate::provider::ModelAccess;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use tauri::AppHandle;

/// 一次最多重选几拍；更多时应重新生成整版。
pub(crate) const MAX_RESELECT_BEATS: usize = 5;
/// 一次最多精修几个镜头。
pub(crate) const MAX_REFINE_SHOTS: usize = 10;

/// 写版本与操作日志所需的作用域；全部来自 LoopState，不接受模型参数。
pub(crate) struct LocalEditScope<'a> {
    pub(crate) project_id: &'a str,
    pub(crate) editing_task_id: &'a str,
    pub(crate) conversation_id: &'a str,
    pub(crate) agent_task_id: &'a str,
}

pub(crate) enum ReselectTarget {
    Beats(Vec<String>),
    Shots(Vec<i64>),
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShotRef {
    pub(crate) shot_index: i64,
    pub(crate) asset_id: String,
    pub(crate) segment_id: Option<String>,
    pub(crate) source_start_ms: i64,
    pub(crate) source_end_ms: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BeatChange {
    pub(crate) beat_id: String,
    pub(crate) before: Vec<ShotRef>,
    pub(crate) after: Vec<ShotRef>,
    pub(crate) match_level: String,
    /// 本拍新池里未被选中的候选数；为 0 时再重选只能换回已排除的素材。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remaining_alternates: Option<usize>,
}

pub(crate) struct LocalEditOutcome {
    pub(crate) storyboard: StoryboardVersion,
    pub(crate) timeline: TimelineVersion,
    pub(crate) changes: Vec<BeatChange>,
}

/// 以当前时间线为准的编辑基准。
struct EditBase {
    timeline: TimelineVersion,
    storyboard: StoryboardVersion,
    /// 按时间线同步后的 storyboard 镜头（顺序与 order_index 不变）。
    shots: Vec<StoryboardShot>,
    /// 与 `timeline.clips` 一一对应：该 clip 属于哪一拍（手动插入的 clip 归到前一拍）。
    clip_beats: Vec<Option<String>>,
    /// 每拍 storyboard 镜头对应 clip 的时长之和。
    beat_story_ms: HashMap<String, i64>,
    /// 每拍全部 clip（含手动插入）的时长之和；重选时新镜头要填满它。
    beat_all_ms: HashMap<String, i64>,
    /// 覆盖拍的时间线顺序。
    beat_order: Vec<String>,
}

pub(crate) fn reselect_shots(
    app: &AppHandle,
    scope: &LocalEditScope<'_>,
    timeline_version_id: &str,
    target: ReselectTarget,
    instruction: Option<&str>,
    keep_current: bool,
    already_reselected: &HashSet<String>,
) -> Result<LocalEditOutcome, String> {
    let connection = crate::db::open_connection(app)?;
    let sources = expanded_sources(&connection, scope.project_id)?;
    let base = prepare_base(&connection, scope, timeline_version_id, &sources)?;
    let target_beats = resolve_target_beats(&base, target)?;
    if let Some(beat) = target_beats
        .iter()
        .find(|beat| already_reselected.contains(*beat))
    {
        return Err(format!(
            "Beat '{beat}' was already re-picked in this turn. Show the user the new preview and ask before trying again."
        ));
    }
    let target_set = target_beats.iter().cloned().collect::<HashSet<_>>();
    let instruction = instruction.map(str::trim).filter(|text| !text.is_empty());

    let timing = synthetic_timing(&base, |beat| {
        if target_set.contains(beat) {
            base.beat_all_ms.get(beat).copied()
        } else {
            base.beat_story_ms.get(beat).copied()
        }
    });

    // P2：只为目标拍召回；用户原话拼进检索文本，但不写回 beat。
    let query_beats = target_beats
        .iter()
        .filter_map(|id| base.storyboard.beats.iter().find(|beat| &beat.id == id))
        .map(|beat| {
            let mut beat = beat.clone();
            if let Some(note) = instruction {
                beat.required_visual = format!("{} {note}", beat.required_visual);
            }
            beat
        })
        .collect::<Vec<_>>();
    let embeddings = super::semantic::encode_beats(app, &query_beats).unwrap_or_else(|error| {
        log::warn!("Local reselect semantic ranking unavailable: {error}");
        Vec::new()
    });
    let clip_embeddings = super::clip::encode_beats(app, &query_beats).unwrap_or_else(|error| {
        log::warn!("Local reselect CLIP ranking unavailable: {error}");
        Vec::new()
    });
    let usage_counts = super::storyboard_usage_counts(&connection, scope.project_id)?;
    let score_first_slots =
        crate::projects::candidate_score_first_slots(&connection, scope.project_id)?;
    let max_uses = super::max_asset_uses_for_shot_count(base.shots.len());

    let frozen = base
        .shots
        .iter()
        .filter(|shot| !target_set.contains(&shot.beat_id))
        .collect::<Vec<_>>();
    let frozen_sources = frozen
        .iter()
        .filter_map(|shot| source_for_shot(&sources, shot))
        .collect::<Vec<_>>();
    let mut frozen_uses: HashMap<&str, usize> = HashMap::new();
    for shot in &frozen {
        *frozen_uses.entry(shot.asset_id.as_str()).or_default() += 1;
    }

    let shared_terms = super::scoring::shared_lexical_terms(&base.storyboard.beats);
    let mut new_pools = Vec::new();
    for (index, beat) in query_beats.iter().enumerate() {
        let current_assets = base
            .shots
            .iter()
            .filter(|shot| shot.beat_id == beat.id)
            .map(|shot| shot.asset_id.as_str())
            .collect::<HashSet<_>>();
        let neighbour_assets = neighbour_assets(&base.shots, &beat.id, &target_set);
        let filtered = sources
            .iter()
            .filter(|source| keep_current || !current_assets.contains(source.asset_id.as_str()))
            .filter(|source| !neighbour_assets.contains(source.asset_id.as_str()))
            .filter(|source| {
                frozen_uses
                    .get(source.asset_id.as_str())
                    .map_or(true, |uses| *uses < max_uses)
            })
            .filter(|source| {
                !frozen.iter().any(|shot| {
                    shot.asset_id == source.asset_id
                        && ranges_overlap(
                            (shot.source_start_ms, shot.source_end_ms),
                            source_range(source),
                        )
                })
            })
            .filter(|source| {
                !frozen_sources
                    .iter()
                    .any(|frozen| phases::sources_are_similar(frozen, source))
            })
            .cloned()
            .collect::<Vec<_>>();
        let target_ms = timing.duration(&beat.id).unwrap_or(0).max(1);
        let pool = phases::build_beat_pool(
            beat,
            &filtered,
            &usage_counts,
            embeddings.get(index).map(Vec::as_slice),
            clip_embeddings.get(index).map(Vec::as_slice),
            target_ms,
            score_first_slots,
            &shared_terms,
        )
        .ok_or_else(|| {
            format!(
                "storyboard_local_reselect_failed: beat '{}' has no candidates left after excluding the current, neighbouring and similar footage. Ask the user whether to import more clips or keep the current shot.",
                beat.id
            )
        })?;
        new_pools.push(pool);
    }

    let brief = base.storyboard.brief.clone();
    let mut rough = RoughStoryboard {
        speech_timing: timing,
        title: base.storyboard.title.clone(),
        summary: base.storyboard.summary.clone(),
        target_duration_ms: base.storyboard.target_duration_ms,
        script_mode: base.storyboard.script_mode.clone(),
        shot_length_hint: String::new(),
        beats: base.storyboard.beats.clone(),
        uncovered_beat_ids: base.storyboard.uncovered_beat_ids.clone(),
        shots: Vec::new(),
        candidate_pools: new_pools.clone(),
    };
    let access = ModelAccess::resolve()?;
    let context = frozen
        .iter()
        .map(|shot| {
            (
                shot.beat_id.clone(),
                shot.asset_id.clone(),
                shot.segment_id.clone().unwrap_or_else(|| "-".to_owned()),
            )
        })
        .collect::<Vec<_>>();
    let picked = phases::phase3_select_beats(app, &access, &brief, &rough, &context, instruction)?;

    // 合并：目标拍换成新镜头，其余原样；按位置重新编号，记下冻结镜头的新旧序号。
    let mut merged = Vec::new();
    let mut new_to_old = HashMap::new();
    let mut mutable = HashSet::new();
    for beat_id in &base.beat_order {
        if target_set.contains(beat_id) {
            for shot in picked.shots.iter().filter(|shot| &shot.beat_id == beat_id) {
                let mut shot = shot.clone();
                shot.order_index = merged.len() as i64 + 1;
                mutable.insert(shot.order_index);
                merged.push(shot);
            }
        } else {
            for shot in base.shots.iter().filter(|shot| &shot.beat_id == beat_id) {
                let mut shot = shot.clone();
                new_to_old.insert(merged.len() as i64 + 1, shot.order_index);
                shot.order_index = merged.len() as i64 + 1;
                merged.push(shot);
            }
        }
    }
    let selected = base_content(&base, &brief, merged);
    rough.candidate_pools = merged_pools(&connection, &base, new_pools.clone())?;
    let refined = super::run_phase4_and_validate(
        app,
        &access,
        &brief,
        &selected,
        &rough,
        &sources,
        Some((mutable, instruction.map(str::to_owned))),
    )?;
    ensure_frozen_untouched(&base, &refined, &new_to_old)?;
    ensure_beats_fill_slots(&refined, &rough.speech_timing, &target_set)?;

    let changes = target_beats
        .iter()
        .map(|beat_id| {
            let pool = new_pools.iter().find(|pool| &pool.beat_id == beat_id);
            let after = shot_refs(&refined.shots, beat_id);
            BeatChange {
                beat_id: beat_id.clone(),
                before: shot_refs(&base.shots, beat_id),
                match_level: refined
                    .shots
                    .iter()
                    .find(|shot| &shot.beat_id == beat_id)
                    .map(|shot| shot.match_level.clone())
                    .unwrap_or_default(),
                remaining_alternates: pool
                    .map(|pool| pool.candidates.len().saturating_sub(after.len())),
                after,
            }
        })
        .collect();
    persist(
        &connection,
        scope,
        &base,
        refined,
        &new_to_old,
        &target_set,
        &rough.candidate_pools,
        "reselect_shots",
        changes,
    )
}

pub(crate) fn refine_shot_ranges(
    app: &AppHandle,
    scope: &LocalEditScope<'_>,
    timeline_version_id: &str,
    shot_indexes: &[i64],
    instruction: Option<&str>,
) -> Result<LocalEditOutcome, String> {
    if shot_indexes.is_empty() {
        return Err("refine_shot_ranges needs at least one shotIndex.".to_owned());
    }
    if shot_indexes.len() > MAX_REFINE_SHOTS {
        return Err(format!(
            "refine_shot_ranges accepts at most {MAX_REFINE_SHOTS} shots at once."
        ));
    }
    let connection = crate::db::open_connection(app)?;
    let sources = expanded_sources(&connection, scope.project_id)?;
    let base = prepare_base(&connection, scope, timeline_version_id, &sources)?;
    let wanted = shot_indexes.iter().copied().collect::<HashSet<_>>();
    for index in &wanted {
        if !base.shots.iter().any(|shot| shot.order_index == *index) {
            return Err(format!(
                "Shot {index} is not a storyboard shot on the current timeline (inserted clips cannot be refined)."
            ));
        }
    }
    let instruction = instruction.map(str::trim).filter(|text| !text.is_empty());
    let timing = synthetic_timing(&base, |beat| base.beat_story_ms.get(beat).copied());
    let mut merged = Vec::new();
    let mut new_to_old = HashMap::new();
    let mut mutable = HashSet::new();
    let mut touched_beats = Vec::new();
    for beat_id in &base.beat_order {
        for shot in base.shots.iter().filter(|shot| &shot.beat_id == beat_id) {
            let mut shot = shot.clone();
            let new_index = merged.len() as i64 + 1;
            if shot.order_index != new_index {
                return Err(
                    "Storyboard shot numbering does not match the timeline; regenerate the storyboard first."
                        .to_owned(),
                );
            }
            if wanted.contains(&shot.order_index) {
                mutable.insert(new_index);
                if !touched_beats.contains(beat_id) {
                    touched_beats.push(beat_id.clone());
                }
            } else {
                new_to_old.insert(new_index, shot.order_index);
            }
            shot.order_index = new_index;
            merged.push(shot);
        }
    }
    let brief = base.storyboard.brief.clone();
    let selected = base_content(&base, &brief, merged);
    let rough = RoughStoryboard {
        speech_timing: timing,
        title: base.storyboard.title.clone(),
        summary: base.storyboard.summary.clone(),
        target_duration_ms: base.storyboard.target_duration_ms,
        script_mode: base.storyboard.script_mode.clone(),
        shot_length_hint: String::new(),
        beats: base.storyboard.beats.clone(),
        uncovered_beat_ids: base.storyboard.uncovered_beat_ids.clone(),
        shots: Vec::new(),
        candidate_pools: merged_pools(&connection, &base, Vec::new())?,
    };
    let access = ModelAccess::resolve()?;
    let mut refined = super::run_phase4_and_validate(
        app,
        &access,
        &brief,
        &selected,
        &rough,
        &sources,
        Some((mutable.clone(), instruction.map(str::to_owned))),
    )?;
    // 只精修切点：每个镜头的时间线时长锁回原槽位，素材与片段不得变化。
    for shot in &mut refined.shots {
        if !mutable.contains(&shot.order_index) {
            continue;
        }
        let original = &selected.shots[(shot.order_index - 1) as usize];
        if shot.asset_id != original.asset_id || shot.segment_id != original.segment_id {
            return Err(format!(
                "refine_shot_ranges changed the footage of shot {}; nothing was saved.",
                shot.order_index
            ));
        }
        shot.duration_ms = original.duration_ms;
        if shot.source_end_ms - shot.source_start_ms > shot.duration_ms {
            shot.source_end_ms = shot.source_start_ms + shot.duration_ms;
        }
    }
    rough.speech_timing.validate(&refined)?;
    super::validate_storyboard(&refined, &sources, &brief)?;
    ensure_frozen_untouched(&base, &refined, &new_to_old)?;

    let original_by_new = selected
        .shots
        .iter()
        .map(|shot| (shot.order_index, shot))
        .collect::<HashMap<_, _>>();
    let changes = refined
        .shots
        .iter()
        .filter(|shot| mutable.contains(&shot.order_index))
        .map(|shot| BeatChange {
            beat_id: shot.beat_id.clone(),
            before: original_by_new
                .get(&shot.order_index)
                .map(|original| vec![shot_ref(original)])
                .unwrap_or_default(),
            after: vec![shot_ref(shot)],
            match_level: shot.match_level.clone(),
            remaining_alternates: None,
        })
        .collect();
    let touched = touched_beats.into_iter().collect::<HashSet<_>>();
    // 精修不换素材：沿用每拍原有 clip，但按新范围写入。
    persist_refine(
        &connection,
        scope,
        &base,
        refined,
        &mutable,
        &touched,
        &rough.candidate_pools,
        changes,
    )
}

fn expanded_sources(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<StoryboardSource>, String> {
    let (whole, _) = super::storyboard_sources(connection, project_id, None)?;
    let ids = whole
        .iter()
        .map(|source| source.asset_id.clone())
        .collect::<HashSet<_>>();
    let (sources, _) = super::storyboard_sources(connection, project_id, Some(&ids))?;
    Ok(sources)
}

fn prepare_base(
    connection: &Connection,
    scope: &LocalEditScope<'_>,
    timeline_version_id: &str,
    sources: &[StoryboardSource],
) -> Result<EditBase, String> {
    let timeline = crate::timeline::load_timeline_version(connection, timeline_version_id)?;
    if timeline.project_id != scope.project_id {
        return Err("Timeline does not belong to this project.".to_owned());
    }
    let storyboard = super::load_storyboard_version(connection, &timeline.storyboard_version_id)?;
    if storyboard.project_id != scope.project_id
        || storyboard.editing_task_id != scope.editing_task_id
    {
        return Err("Storyboard does not belong to this editing session.".to_owned());
    }
    if storyboard.shots.is_empty() {
        return Err("The current storyboard has no shots to edit.".to_owned());
    }
    let shot_by_index = storyboard
        .shots
        .iter()
        .map(|shot| (shot.order_index, shot))
        .collect::<HashMap<_, _>>();

    let mut clip_beats = Vec::with_capacity(timeline.clips.len());
    let mut beat_story_ms: HashMap<String, i64> = HashMap::new();
    let mut beat_all_ms: HashMap<String, i64> = HashMap::new();
    let mut beat_order: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut seen_story_orders = Vec::new();
    for clip in &timeline.clips {
        let span = (clip.timeline_end_ms - clip.timeline_start_ms).max(0);
        if let Some(shot) = shot_by_index.get(&clip.shot_index) {
            seen_story_orders.push(shot.order_index);
            if current.as_deref() != Some(shot.beat_id.as_str()) {
                if beat_order.contains(&shot.beat_id) {
                    return Err(
                        "The timeline order no longer matches the storyboard beats (clips were reordered). Regenerate the storyboard or edit clips directly."
                            .to_owned(),
                    );
                }
                beat_order.push(shot.beat_id.clone());
            }
            current = Some(shot.beat_id.clone());
            *beat_story_ms.entry(shot.beat_id.clone()).or_default() += span;
        }
        if let Some(beat) = &current {
            *beat_all_ms.entry(beat.clone()).or_default() += span;
        }
        clip_beats.push(current.clone());
    }
    let mut sorted = seen_story_orders.clone();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted != seen_story_orders || sorted.len() != storyboard.shots.len() {
        return Err(
            "The timeline no longer maps one-to-one onto the storyboard shots. Regenerate the storyboard or edit clips directly."
                .to_owned(),
        );
    }

    // 以时间线为准同步镜头：replace_clips / change_clip_duration 的手动修改写回 shot。
    let shots = storyboard
        .shots
        .iter()
        .map(|shot| {
            let mut shot = shot.clone();
            let Some(clip) = timeline
                .clips
                .iter()
                .find(|clip| clip.shot_index == shot.order_index)
            else {
                return shot;
            };
            shot.duration_ms = (clip.timeline_end_ms - clip.timeline_start_ms).max(1);
            if clip.asset_id != shot.asset_id
                || clip.source_start_ms != shot.source_start_ms
                || clip.source_end_ms != shot.source_end_ms
            {
                if clip.asset_id != shot.asset_id {
                    shot.reason = "Synced from a manual timeline replacement.".to_owned();
                    shot.match_level = "contextual".to_owned();
                }
                shot.asset_id = clip.asset_id.clone();
                shot.source_start_ms = clip.source_start_ms;
                shot.source_end_ms = clip.source_end_ms;
                shot.segment_id = containing_segment(sources, &shot);
            }
            shot.crop_focus = clip.crop_focus;
            shot
        })
        .collect();
    Ok(EditBase {
        timeline,
        storyboard,
        shots,
        clip_beats,
        beat_story_ms,
        beat_all_ms,
        beat_order,
    })
}

fn resolve_target_beats(base: &EditBase, target: ReselectTarget) -> Result<Vec<String>, String> {
    let mut beats = Vec::new();
    match target {
        ReselectTarget::Beats(ids) => {
            for id in ids {
                if !base.beat_order.contains(&id) {
                    return Err(format!("Beat '{id}' has no shot on the current timeline."));
                }
                if !beats.contains(&id) {
                    beats.push(id);
                }
            }
        }
        ReselectTarget::Shots(indexes) => {
            for index in indexes {
                let position = base
                    .timeline
                    .clips
                    .iter()
                    .position(|clip| clip.shot_index == index)
                    .ok_or_else(|| {
                        format!("Shot {index} does not exist on the current timeline.")
                    })?;
                let beat = base.clip_beats[position].clone().ok_or_else(|| {
                    format!("Shot {index} does not belong to any storyboard beat.")
                })?;
                if !beats.contains(&beat) {
                    beats.push(beat);
                }
            }
        }
    }
    if beats.is_empty() {
        return Err("reselect_shots needs at least one beatId or shotIndex.".to_owned());
    }
    if beats.len() > MAX_RESELECT_BEATS {
        return Err(format!(
            "reselect_shots accepts at most {MAX_RESELECT_BEATS} beats at once; regenerate the storyboard for larger changes."
        ));
    }
    // 按时间线顺序处理，让前面的新镜头成为后面拍的上下文。
    beats.sort_by_key(|beat| base.beat_order.iter().position(|item| item == beat));
    Ok(beats)
}

/// 按时间线顺序为每个覆盖拍生成首尾相接的时段；Pacing 保证有 uncovered 拍时也照样校验。
fn synthetic_timing(base: &EditBase, duration: impl Fn(&str) -> Option<i64>) -> SpeechTiming {
    let mut cursor = 0;
    let beats = base
        .beat_order
        .iter()
        .filter_map(|beat| {
            let ms = duration(beat)?.max(1);
            let timing = BeatTiming {
                beat_id: beat.clone(),
                start_ms: cursor,
                end_ms: cursor + ms,
            };
            cursor += ms;
            Some(timing)
        })
        .collect();
    SpeechTiming {
        kind: SpeechTimingKind::Pacing,
        beats,
        pauses_ms: Vec::new(),
    }
}

fn base_content(base: &EditBase, brief: &str, shots: Vec<StoryboardShot>) -> StoryboardContent {
    StoryboardContent {
        brief: brief.to_owned(),
        title: base.storyboard.title.clone(),
        summary: base.storyboard.summary.clone(),
        target_duration_ms: base.storyboard.target_duration_ms,
        script_mode: base.storyboard.script_mode.clone(),
        beats: base.storyboard.beats.clone(),
        uncovered_beat_ids: base.storyboard.uncovered_beat_ids.clone(),
        shots,
    }
}

/// 派生版本沿用上一版的候选池，只替换重新召回过的拍，手动换镜面板继续可用。
fn merged_pools(
    connection: &Connection,
    base: &EditBase,
    fresh: Vec<BeatCandidatePool>,
) -> Result<Vec<BeatCandidatePool>, String> {
    let mut pools =
        crate::shot_replacement::load_pools(connection, &base.storyboard.id)?.unwrap_or_default();
    for pool in fresh {
        match pools.iter_mut().find(|item| item.beat_id == pool.beat_id) {
            Some(existing) => *existing = pool,
            None => pools.push(pool),
        }
    }
    Ok(pools)
}

fn neighbour_assets<'a>(
    shots: &'a [StoryboardShot],
    beat_id: &str,
    targets: &HashSet<String>,
) -> HashSet<&'a str> {
    let mut assets = HashSet::new();
    let first = shots.iter().position(|shot| shot.beat_id == beat_id);
    let last = shots.iter().rposition(|shot| shot.beat_id == beat_id);
    if let Some(prev) = first
        .and_then(|index| index.checked_sub(1))
        .and_then(|i| shots.get(i))
    {
        if !targets.contains(&prev.beat_id) {
            assets.insert(prev.asset_id.as_str());
        }
    }
    if let Some(next) = last.and_then(|index| shots.get(index + 1)) {
        if !targets.contains(&next.beat_id) {
            assets.insert(next.asset_id.as_str());
        }
    }
    assets
}

fn source_range(source: &StoryboardSource) -> (i64, i64) {
    match &source.segment {
        Some(segment) => (segment.start_ms, segment.end_ms),
        None => (0, source.duration_ms.unwrap_or(0).max(0)),
    }
}

fn ranges_overlap(a: (i64, i64), b: (i64, i64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn source_for_shot<'a>(
    sources: &'a [StoryboardSource],
    shot: &StoryboardShot,
) -> Option<&'a StoryboardSource> {
    sources.iter().find(|source| {
        source.asset_id == shot.asset_id
            && match (&source.segment, &shot.segment_id) {
                (Some(segment), Some(id)) => &segment.id == id,
                (None, None) => true,
                _ => false,
            }
    })
}

fn containing_segment(sources: &[StoryboardSource], shot: &StoryboardShot) -> Option<String> {
    sources
        .iter()
        .filter(|source| source.asset_id == shot.asset_id)
        .filter_map(|source| source.segment.as_ref())
        .find(|segment| {
            segment.start_ms <= shot.source_start_ms && shot.source_end_ms <= segment.end_ms
        })
        .map(|segment| segment.id.clone())
}

fn shot_ref(shot: &StoryboardShot) -> ShotRef {
    ShotRef {
        shot_index: shot.order_index,
        asset_id: shot.asset_id.clone(),
        segment_id: shot.segment_id.clone(),
        source_start_ms: shot.source_start_ms,
        source_end_ms: shot.source_end_ms,
    }
}

fn shot_refs(shots: &[StoryboardShot], beat_id: &str) -> Vec<ShotRef> {
    shots
        .iter()
        .filter(|shot| shot.beat_id == beat_id)
        .map(shot_ref)
        .collect()
}

/// 可信边界：冻结镜头的素材、源范围、时长和构图必须与编辑前逐字一致。
fn ensure_frozen_untouched(
    base: &EditBase,
    refined: &StoryboardContent,
    new_to_old: &HashMap<i64, i64>,
) -> Result<(), String> {
    for (new_index, old_index) in new_to_old {
        let after = refined
            .shots
            .iter()
            .find(|shot| shot.order_index == *new_index);
        let before = base
            .shots
            .iter()
            .find(|shot| shot.order_index == *old_index);
        let unchanged = match (before, after) {
            (Some(before), Some(after)) => {
                before.asset_id == after.asset_id
                    && before.source_start_ms == after.source_start_ms
                    && before.source_end_ms == after.source_end_ms
                    && before.duration_ms == after.duration_ms
                    && before.crop_focus == after.crop_focus
            }
            _ => false,
        };
        if !unchanged {
            return Err(format!(
                "Local edit would change frozen shot {old_index}; nothing was saved."
            ));
        }
    }
    Ok(())
}

/// 新镜头必须正好填满原拍槽位，否则后续画面会与配音错位。
fn ensure_beats_fill_slots(
    refined: &StoryboardContent,
    timing: &SpeechTiming,
    targets: &HashSet<String>,
) -> Result<(), String> {
    for beat in targets {
        let expected = timing.duration(beat).unwrap_or(0);
        let actual = refined
            .shots
            .iter()
            .filter(|shot| &shot.beat_id == beat)
            .map(|shot| shot.duration_ms)
            .sum::<i64>();
        if actual != expected {
            return Err(format!(
                "storyboard_local_reselect_failed: new picture for beat '{beat}' is {actual}ms but its slot is {expected}ms; nothing was saved."
            ));
        }
    }
    Ok(())
}

/// 重选：目标拍的旧 clip（含手动插入的）整体换成新镜头；其余 clip 原样保留，只更新序号。
fn build_reselect_clips(
    base: &EditBase,
    refined: &StoryboardContent,
    new_to_old: &HashMap<i64, i64>,
    targets: &HashSet<String>,
) -> Vec<TimelineClip> {
    let old_to_new = new_to_old
        .iter()
        .map(|(new, old)| (*old, *new))
        .collect::<HashMap<_, _>>();
    let story_orders = base
        .storyboard
        .shots
        .iter()
        .map(|shot| shot.order_index)
        .collect::<HashSet<_>>();
    let mut emitted = HashSet::new();
    let mut clips = Vec::new();
    for (clip, beat) in base.timeline.clips.iter().zip(&base.clip_beats) {
        if let Some(beat) = beat.as_ref().filter(|beat| targets.contains(*beat)) {
            if emitted.insert(beat.clone()) {
                let lead_text = clip.on_screen_text.clone();
                for (offset, shot) in refined
                    .shots
                    .iter()
                    .filter(|shot| &shot.beat_id == beat)
                    .enumerate()
                {
                    clips.push(TimelineClip {
                        crop_focus: shot.crop_focus,
                        shot_index: shot.order_index,
                        asset_id: shot.asset_id.clone(),
                        source_start_ms: shot.source_start_ms,
                        source_end_ms: shot.source_end_ms,
                        timeline_start_ms: 0,
                        timeline_end_ms: shot.duration_ms,
                        on_screen_text: if offset == 0 {
                            lead_text.clone()
                        } else {
                            String::new()
                        },
                        ..TimelineClip::default()
                    });
                }
            }
            continue;
        }
        let mut clip = clip.clone();
        clip.shot_index = if story_orders.contains(&clip.shot_index) {
            old_to_new.get(&clip.shot_index).copied().unwrap_or(-1)
        } else {
            -1
        };
        clip.derived_from_shot_index = clip
            .derived_from_shot_index
            .and_then(|old| old_to_new.get(&old).copied());
        clips.push(clip);
    }
    finalize_clips(clips, refined.shots.len() as i64)
}

/// 手动插入的 clip 编号接在 storyboard 镜头之后，并重新首尾相接排布时间线位置。
fn finalize_clips(mut clips: Vec<TimelineClip>, story_count: i64) -> Vec<TimelineClip> {
    let mut next = story_count + 1;
    let mut cursor = 0;
    for clip in &mut clips {
        if clip.shot_index < 0 {
            clip.shot_index = next;
            next += 1;
        }
        let span = clip.timeline_end_ms - clip.timeline_start_ms;
        clip.timeline_start_ms = cursor;
        clip.timeline_end_ms = cursor + span;
        cursor += span;
    }
    clips
}

#[allow(clippy::too_many_arguments)]
fn persist(
    connection: &Connection,
    scope: &LocalEditScope<'_>,
    base: &EditBase,
    refined: StoryboardContent,
    new_to_old: &HashMap<i64, i64>,
    targets: &HashSet<String>,
    pools: &[BeatCandidatePool],
    operation: &str,
    changes: Vec<BeatChange>,
) -> Result<LocalEditOutcome, String> {
    let clips = build_reselect_clips(base, &refined, new_to_old, targets);
    write_versions(
        connection, scope, base, refined, clips, targets, pools, operation, changes,
    )
}

#[allow(clippy::too_many_arguments)]
fn persist_refine(
    connection: &Connection,
    scope: &LocalEditScope<'_>,
    base: &EditBase,
    refined: StoryboardContent,
    mutable: &HashSet<i64>,
    touched: &HashSet<String>,
    pools: &[BeatCandidatePool],
    changes: Vec<BeatChange>,
) -> Result<LocalEditOutcome, String> {
    // 精修不改镜头数量与顺序：clip 序号与 storyboard 一致，只写回被精修镜头的源范围与构图。
    let clips = base
        .timeline
        .clips
        .iter()
        .cloned()
        .map(|mut clip| {
            if let Some(shot) = refined.shots.iter().find(|shot| {
                shot.order_index == clip.shot_index && mutable.contains(&shot.order_index)
            }) {
                clip.source_start_ms = shot.source_start_ms;
                clip.source_end_ms = shot.source_end_ms;
                clip.crop_focus = shot.crop_focus;
            }
            clip
        })
        .collect();
    write_versions(
        connection,
        scope,
        base,
        refined,
        clips,
        touched,
        pools,
        "refine_shot_ranges",
        changes,
    )
}

#[allow(clippy::too_many_arguments)]
fn write_versions(
    connection: &Connection,
    scope: &LocalEditScope<'_>,
    base: &EditBase,
    refined: StoryboardContent,
    clips: Vec<TimelineClip>,
    changed_beats: &HashSet<String>,
    pools: &[BeatCandidatePool],
    operation: &str,
    changes: Vec<BeatChange>,
) -> Result<LocalEditOutcome, String> {
    let media_options = crate::media_options::storyboard_options(connection, &base.storyboard.id)?;
    let mut changed_beat_ids = base
        .beat_order
        .iter()
        .filter(|beat| changed_beats.contains(*beat))
        .cloned()
        .collect::<Vec<_>>();
    changed_beat_ids.dedup();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let storyboard = super::insert_storyboard_version(
        &transaction,
        scope.project_id.to_owned(),
        scope.editing_task_id,
        &base.storyboard.brief,
        refined,
        pools,
        media_options,
        StoryboardDerivation {
            derived_from_version_id: Some(base.storyboard.id.clone()),
            changed_beat_ids,
        },
    )?;
    let mut timeline_base = base.timeline.clone();
    timeline_base.storyboard_version_id = storyboard.id.clone();
    let timeline = crate::timeline::insert_timeline_version_with_log(
        &transaction,
        scope.project_id,
        scope.editing_task_id,
        scope.conversation_id,
        scope.agent_task_id,
        &timeline_base,
        operation,
        clips,
        base.timeline.text_tracks.clone(),
        base.timeline.music_tracks.clone(),
        base.timeline.voiceover_tracks.clone(),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(LocalEditOutcome {
        storyboard,
        timeline,
        changes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(
        order_index: i64,
        beat_id: &str,
        asset_id: &str,
        start: i64,
        end: i64,
    ) -> StoryboardShot {
        StoryboardShot {
            crop_focus: None,
            order_index,
            duration_ms: end - start,
            purpose: String::new(),
            on_screen_text: String::new(),
            narration_text: String::new(),
            asset_id: asset_id.to_owned(),
            source_start_ms: start,
            source_end_ms: end,
            reason: String::new(),
            beat_id: beat_id.to_owned(),
            match_level: "direct".to_owned(),
            beat_part_index: 1,
            beat_part_count: 1,
            split_role: "lead".to_owned(),
            segment_id: None,
        }
    }

    fn clip(
        shot_index: i64,
        asset_id: &str,
        source: (i64, i64),
        timeline: (i64, i64),
    ) -> TimelineClip {
        TimelineClip {
            shot_index,
            asset_id: asset_id.to_owned(),
            source_start_ms: source.0,
            source_end_ms: source.1,
            timeline_start_ms: timeline.0,
            timeline_end_ms: timeline.1,
            ..TimelineClip::default()
        }
    }

    #[test]
    fn reselect_replaces_only_the_target_beat_and_keeps_frozen_clips_identical() {
        let shots = vec![
            shot(1, "a", "A", 0, 2_000),
            shot(2, "b", "B", 0, 2_000),
            shot(3, "c", "C", 1_000, 3_000),
        ];
        let base = EditBase {
            timeline: TimelineVersion {
                id: "t1".to_owned(),
                project_id: "p".to_owned(),
                storyboard_version_id: "s1".to_owned(),
                version_number: 1,
                clips: vec![
                    clip(1, "A", (0, 2_000), (0, 2_000)),
                    clip(2, "B", (0, 2_000), (2_000, 4_000)),
                    // 手动插入在 b 之后的补画面 clip，属于 b 拍，重选时一起被替换。
                    clip(4, "D", (0, 1_000), (4_000, 5_000)),
                    clip(3, "C", (1_000, 3_000), (5_000, 7_000)),
                ],
                text_tracks: Vec::new(),
                music_tracks: Vec::new(),
                voiceover_tracks: Vec::new(),
                overlay_clips: Vec::new(),
                quality_report: None,
                created_at: 0,
            },
            storyboard: StoryboardVersion {
                id: "s1".to_owned(),
                project_id: "p".to_owned(),
                editing_task_id: "e".to_owned(),
                version_number: 1,
                brief: String::new(),
                title: String::new(),
                summary: String::new(),
                target_duration_ms: 7_000,
                script_mode: "key_message".to_owned(),
                beats: Vec::new(),
                uncovered_beat_ids: Vec::new(),
                shots: shots.clone(),
                created_at: 0,
                derivation: StoryboardDerivation::default(),
            },
            shots: shots.clone(),
            clip_beats: ["a", "b", "b", "c"]
                .map(|beat| Some(beat.to_owned()))
                .to_vec(),
            beat_story_ms: HashMap::new(),
            beat_all_ms: HashMap::new(),
            beat_order: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        };
        let mut refined = base_content(
            &base,
            "",
            vec![
                shot(1, "a", "A", 0, 2_000),
                shot(2, "b", "E", 500, 3_500),
                shot(3, "c", "C", 1_000, 3_000),
            ],
        );
        let new_to_old = HashMap::from([(1, 1), (3, 3)]);
        let targets = HashSet::from(["b".to_owned()]);

        let clips = build_reselect_clips(&base, &refined, &new_to_old, &targets);
        let summary = clips
            .iter()
            .map(|clip| {
                (
                    clip.shot_index,
                    clip.asset_id.as_str(),
                    clip.source_start_ms,
                    clip.source_end_ms,
                    clip.timeline_start_ms,
                    clip.timeline_end_ms,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![
                (1, "A", 0, 2_000, 0, 2_000),
                (2, "E", 500, 3_500, 2_000, 5_000),
                (3, "C", 1_000, 3_000, 5_000, 7_000),
            ]
        );
        assert!(ensure_frozen_untouched(&base, &refined, &new_to_old).is_ok());

        refined.shots[2].source_start_ms = 1_200;
        assert!(ensure_frozen_untouched(&base, &refined, &new_to_old).is_err());
    }
}
