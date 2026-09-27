//! 音乐先行剪辑：BGM 开、配音关时，先选曲并定下音乐窗口（起点落在乐句/小节起点，终点落在乐句结束），
//! 再让 Phase 4 把切点吸附到窗口内的节拍网格上；配音开时旁白仍是时钟，只在小容差内把切点挪到拍上。
//! 窗口与网格存进分镜 content_json.musicPlan，配乐、预览和各编辑器交付都按同一个起点偏移铺音乐。
//! 纯计算放在这里便于回归测试；选曲与写时间线在 `agentloop/auto_music.rs`。

use crate::assets::beats::MIN_BEAT_CONFIDENCE;
use crate::models::BeatAnalysis;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// 已选中的曲子（选曲在 Phase 1 之前完成，与时长无关）。
#[derive(Clone, Debug)]
pub(crate) struct MusicChoice {
    pub asset_id: String,
    pub title: String,
    /// "library" 或 "jamendo"。
    pub source: &'static str,
    pub duration_ms: i64,
    /// Jamendo 曲目的 (许可链接, 署名)。
    pub license: Option<(String, String)>,
    pub analysis: Option<BeatAnalysis>,
    /// 节拍分析不可用的真实原因。
    pub analysis_note: Option<String>,
}

/// 音乐窗口与窗口内的节拍网格；网格时间相对成片起点（0 = 音乐源 `source_start_ms`）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MusicPlan {
    pub asset_id: String,
    pub title: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    pub tempo_bpm: f64,
    pub source_start_ms: i64,
    pub duration_ms: i64,
    pub ends_on_phrase: bool,
    pub fade_in_ms: i64,
    pub fade_out_ms: i64,
    pub beats_ms: Vec<i64>,
    pub downbeats_ms: Vec<i64>,
    pub phrase_starts_ms: Vec<i64>,
    pub bar_energy: Vec<f32>,
}

fn usable_analysis(choice: &MusicChoice) -> Result<&BeatAnalysis, String> {
    let analysis = choice.analysis.as_ref().ok_or_else(|| {
        choice
            .analysis_note
            .clone()
            .unwrap_or_else(|| "the track has no beat analysis".to_owned())
    })?;
    if analysis.confidence < MIN_BEAT_CONFIDENCE || analysis.downbeats_ms.len() < 2 {
        return Err(format!(
            "the track has no steady beat to cut to (confidence {:.2})",
            analysis.confidence
        ));
    }
    Ok(analysis)
}

fn median_gap(values: &[i64]) -> i64 {
    let mut gaps = values.windows(2).map(|pair| pair[1] - pair[0]).collect::<Vec<_>>();
    gaps.sort_unstable();
    gaps.get(gaps.len() / 2).copied().unwrap_or(2_000).max(1)
}

fn bar_energy_at(analysis: &BeatAnalysis, time_ms: i64) -> f32 {
    analysis
        .downbeats_ms
        .iter()
        .rposition(|downbeat| *downbeat <= time_ms)
        .and_then(|index| analysis.bar_energy.get(index).copied())
        .unwrap_or(0.5)
}

/// 选音乐窗口 (起点, 终点, 终点是否乐句结束)：起点取乐句起点（其次第一个小节起点），终点优先乐句边界或曲尾，
/// 其次小节起点；长度尽量贴近目标，偏差超过 25% 的不要。
pub(crate) fn choose_window(
    analysis: &BeatAnalysis,
    track_ms: i64,
    target_ms: i64,
) -> Option<(i64, i64, bool)> {
    let mut starts = analysis
        .phrase_starts_ms
        .iter()
        .map(|start| (*start, true))
        .collect::<Vec<_>>();
    if let Some(first) = analysis.downbeats_ms.first() {
        if !analysis.phrase_starts_ms.contains(first) {
            starts.push((*first, false));
        }
    }
    let mut ends = analysis
        .phrase_starts_ms
        .iter()
        .map(|end| (*end, true))
        .collect::<Vec<_>>();
    ends.push((track_ms, true));
    ends.extend(
        analysis
            .downbeats_ms
            .iter()
            .filter(|downbeat| !analysis.phrase_starts_ms.contains(downbeat))
            .map(|downbeat| (*downbeat, false)),
    );
    let target = target_ms.max(1) as f64;
    let mut best: Option<(f64, (i64, i64, bool))> = None;
    for &(start, phrase_start) in &starts {
        for &(end, phrase_end) in &ends {
            if end <= start || end > track_ms {
                continue;
            }
            let ratio = (end - start) as f64 / target;
            if !(0.75..=1.25).contains(&ratio) {
                continue;
            }
            let cost = (ratio - 1.0).abs()
                + if phrase_end { 0.0 } else { 0.12 }
                + if phrase_start { 0.0 } else { 0.05 }
                + 0.08 * (1.0 - bar_energy_at(analysis, start) as f64)
                + 0.02 * start as f64 / track_ms.max(1) as f64;
            if best.as_ref().map_or(true, |(current, _)| cost < *current) {
                best = Some((cost, (start, end, phrase_end)));
            }
        }
    }
    best.map(|(_, window)| window)
}

fn relative(values: &[i64], start: i64, end: i64) -> Vec<i64> {
    values
        .iter()
        .filter(|value| **value >= start && **value <= end)
        .map(|value| value - start)
        .collect()
}

fn plan_from_window(
    choice: &MusicChoice,
    analysis: &BeatAnalysis,
    (start, end, ends_on_phrase): (i64, i64, bool),
) -> MusicPlan {
    let bar_ms = median_gap(&analysis.downbeats_ms);
    let natural_end = end >= choice.duration_ms - 300;
    let bar_energy = analysis
        .downbeats_ms
        .iter()
        .zip(&analysis.bar_energy)
        .filter(|(downbeat, _)| **downbeat >= start && **downbeat < end)
        .map(|(_, energy)| *energy)
        .collect();
    MusicPlan {
        asset_id: choice.asset_id.clone(),
        title: choice.title.clone(),
        source: choice.source.to_owned(),
        license_url: choice.license.as_ref().map(|(url, _)| url.clone()),
        attribution: choice.license.as_ref().map(|(_, text)| text.clone()),
        tempo_bpm: analysis.tempo_bpm,
        source_start_ms: start,
        duration_ms: end - start,
        ends_on_phrase,
        fade_in_ms: if start < 50 { 0 } else { 150 },
        // 曲子自然结束就短淡出；截在乐句结束处用最后一小节（不超过 2 秒）淡出。
        fade_out_ms: if natural_end { 300 } else { bar_ms.clamp(800, 2_000) },
        beats_ms: relative(&analysis.beats_ms, start, end),
        downbeats_ms: relative(&analysis.downbeats_ms, start, end),
        phrase_starts_ms: relative(&analysis.phrase_starts_ms, start, end),
        bar_energy,
    }
}

/// 无配音：按目标时长定音乐窗口；不能卡点时返回真实原因，由调用方退回内容时长。
pub(crate) fn plan_window(choice: &MusicChoice, target_ms: i64) -> Result<MusicPlan, String> {
    let analysis = usable_analysis(choice)?;
    let window = choose_window(analysis, choice.duration_ms, target_ms).ok_or_else(|| {
        format!(
            "the {}ms track has no phrase-aligned section close to {target_ms}ms",
            choice.duration_ms
        )
    })?;
    Ok(plan_from_window(choice, analysis, window))
}

/// 配音开：成片长度由旁白定。选一个起点让成片结尾落在乐句边界（或曲尾），起点尽量靠近小节起点。
pub(crate) fn plan_for_fixed_length(
    choice: &MusicChoice,
    timeline_ms: i64,
) -> Result<MusicPlan, String> {
    let analysis = usable_analysis(choice)?;
    let bar_ms = median_gap(&analysis.downbeats_ms) as f64;
    let mut ends = analysis.phrase_starts_ms.clone();
    ends.push(choice.duration_ms);
    let best = ends
        .into_iter()
        .filter(|end| *end >= timeline_ms && *end <= choice.duration_ms)
        .map(|end| {
            let start = end - timeline_ms;
            let to_downbeat = analysis
                .downbeats_ms
                .iter()
                .map(|downbeat| (downbeat - start).abs())
                .min()
                .unwrap_or(0) as f64;
            let cost = 0.5 * to_downbeat / bar_ms + 0.1 * start as f64 / choice.duration_ms.max(1) as f64;
            (cost, start, end)
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .ok_or_else(|| {
            format!(
                "the {}ms track is shorter than the {timeline_ms}ms picture",
                choice.duration_ms
            )
        })?;
    Ok(plan_from_window(choice, analysis, (best.1, best.2, true)))
}

#[derive(Clone, Copy, PartialEq)]
enum PointKind {
    Phrase,
    Downbeat,
    HalfBar,
    Offbeat,
}

impl PointKind {
    fn cut_cost(self) -> f64 {
        match self {
            PointKind::Phrase | PointKind::Downbeat => 0.0,
            PointKind::HalfBar => 0.03,
            PointKind::Offbeat => 0.1,
        }
    }
}

/// 窗口内所有可切的点：0、每一拍、窗口终点，并标出乐句 / 小节 / 半小节 / 反拍。
fn grid_points(plan: &MusicPlan) -> Vec<(i64, PointKind)> {
    let mut points = vec![(0, PointKind::Phrase)];
    let mut beat_in_bar = 0_usize;
    for beat in &plan.beats_ms {
        if plan.downbeats_ms.contains(beat) {
            beat_in_bar = 0;
        }
        if *beat > 0 && *beat < plan.duration_ms {
            let kind = if plan.phrase_starts_ms.contains(beat) {
                PointKind::Phrase
            } else if beat_in_bar == 0 && plan.downbeats_ms.contains(beat) {
                PointKind::Downbeat
            } else if beat_in_bar == 2 {
                PointKind::HalfBar
            } else {
                PointKind::Offbeat
            };
            points.push((*beat, kind));
        }
        beat_in_bar += 1;
    }
    points.push((plan.duration_ms, PointKind::Phrase));
    points.dedup_by_key(|point| point.0);
    points
}

/// 每镜的节拍意向：能量高 1 小节、中 2 小节、低 4 小节，与内容偏好长度按 3:7 加权几何平均（内容为主，
/// 否则全片都会卡成一小节一镜的节拍器），夹在上下界内。
fn shot_targets(preferred: &[i64], plan: &MusicPlan, bounds: (i64, i64)) -> Vec<f64> {
    let bar_ms = median_gap(&plan.downbeats_ms) as f64;
    let total = preferred.iter().sum::<i64>().max(1) as f64;
    let scale = plan.duration_ms as f64 / total;
    let mut cursor = 0.0;
    preferred
        .iter()
        .map(|pref| {
            let position = cursor as i64;
            cursor += *pref as f64 * scale;
            let energy = plan
                .downbeats_ms
                .iter()
                .rposition(|downbeat| *downbeat <= position)
                .and_then(|index| plan.bar_energy.get(index).copied())
                .unwrap_or(0.5);
            let bars = if energy >= 0.67 {
                1.0
            } else if energy >= 0.34 {
                2.0
            } else {
                4.0
            };
            let content = (*pref as f64 * scale).max(1.0);
            (content.powf(0.7) * (bars * bar_ms).powf(0.3)).clamp(bounds.0 as f64, bounds.1 as f64)
        })
        .collect()
}

/// 把 n 个镜头的切点放到节拍网格上（动态规划），返回每镜新时长，总和等于音乐窗口长度。
/// 代价：时长偏离节拍意向的 3×(ln 比值)²；切在反拍 / 半小节加罚；换拍（故事段落）不在乐句边界重罚；
/// 超过素材可用长度（需要放慢）加罚。短于下限的不允许，下限放不下时退到一拍。
pub(crate) fn snap_slots_to_beats(
    preferred: &[i64],
    section_start: &[bool],
    capacity: &[i64],
    bounds: (i64, i64),
    plan: &MusicPlan,
) -> Option<Vec<i64>> {
    let count = preferred.len();
    if count == 0 || section_start.len() != count || capacity.len() != count {
        return None;
    }
    let points = grid_points(plan);
    let targets = shot_targets(preferred, plan, bounds);
    let beat_ms = median_gap(&plan.beats_ms);
    for min_ms in [bounds.0, beat_ms.min(bounds.0)] {
        if let Some(slots) = snap_with_min(&points, &targets, section_start, capacity, bounds, min_ms) {
            return Some(slots);
        }
    }
    None
}

fn snap_with_min(
    points: &[(i64, PointKind)],
    targets: &[f64],
    section_start: &[bool],
    capacity: &[i64],
    bounds: (i64, i64),
    min_ms: i64,
) -> Option<Vec<i64>> {
    let count = targets.len();
    let last = points.len() - 1;
    let mut cost = vec![vec![f64::INFINITY; points.len()]; count + 1];
    let mut back = vec![vec![usize::MAX; points.len()]; count + 1];
    cost[0][0] = 0.0;
    for shot in 0..count {
        for from in 0..points.len() {
            if !cost[shot][from].is_finite() {
                continue;
            }
            for to in from + 1..points.len() {
                // 窗口终点只留给最后一镜。
                if to == last && shot + 1 != count {
                    break;
                }
                let length = points[to].0 - points[from].0;
                if length < min_ms {
                    continue;
                }
                let mut step = 3.0 * (length as f64 / targets[shot]).ln().powi(2);
                if length > capacity[shot] {
                    step += 0.6 * (1.0 - capacity[shot] as f64 / length as f64);
                }
                if length as f64 > bounds.1 as f64 * 1.5 {
                    step += 1.0;
                }
                if to != last {
                    let kind = points[to].1;
                    step += kind.cut_cost();
                    if section_start.get(shot + 1).copied().unwrap_or(false) && kind != PointKind::Phrase {
                        step += 0.35;
                    }
                }
                let total = cost[shot][from] + step;
                if total < cost[shot + 1][to] {
                    cost[shot + 1][to] = total;
                    back[shot + 1][to] = from;
                }
            }
        }
    }
    if !cost[count][last].is_finite() {
        return None;
    }
    let mut cuts = vec![last];
    for shot in (1..=count).rev() {
        let previous = back[shot][*cuts.last()?];
        cuts.push(previous);
    }
    cuts.reverse();
    Some(cuts.windows(2).map(|pair| points[pair[1]].0 - points[pair[0]].0).collect())
}

/// 配音开：切点只在 ±tolerance 内挪到最近的拍上，前后镜至少保留 min_gap。返回新切点（不含 0 与结尾）。
pub(crate) fn nudge_cuts_to_beats(
    cuts: &[i64],
    beats: &[i64],
    tolerance_ms: i64,
    min_gap_ms: i64,
    end_ms: i64,
) -> Vec<i64> {
    let mut nudged = Vec::with_capacity(cuts.len());
    for (index, cut) in cuts.iter().enumerate() {
        let previous = nudged.last().copied().unwrap_or(0);
        let next = cuts.get(index + 1).copied().unwrap_or(end_ms);
        let candidate = beats
            .iter()
            .copied()
            .filter(|beat| (beat - cut).abs() <= tolerance_ms)
            .min_by_key(|beat| (beat - cut).abs())
            .filter(|beat| beat - previous >= min_gap_ms && next - beat >= min_gap_ms);
        nudged.push(candidate.unwrap_or(*cut));
    }
    nudged
}

/// 把音乐先行的结果写进分镜：成功写 `$.musicPlan`，不能卡点写 `$.musicPlanNote`（真实原因）。
pub(crate) fn store_storyboard_music_outcome(
    connection: &Connection,
    storyboard_id: &str,
    outcome: Result<&MusicPlan, &str>,
) -> Result<(), String> {
    let (path, value) = match outcome {
        Ok(plan) => ("$.musicPlan", serde_json::to_string(plan).map_err(|error| error.to_string())?),
        Err(note) => ("$.musicPlanNote", serde_json::to_string(note).map_err(|error| error.to_string())?),
    };
    connection
        .execute(
            "UPDATE storyboard_versions SET content_json = json_set(content_json, ?2, json(?3)) WHERE id = ?1",
            params![storyboard_id, path, value],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// 读分镜的音乐先行结果：(音乐窗口, 没能卡点的原因)。旧分镜两者都没有。
pub(crate) fn storyboard_music_outcome(
    connection: &Connection,
    storyboard_id: &str,
) -> Result<(Option<MusicPlan>, Option<String>), String> {
    let (plan, note): (Option<String>, Option<String>) = connection
        .query_row(
            "SELECT json_extract(content_json, '$.musicPlan'), json_extract(content_json, '$.musicPlanNote') FROM storyboard_versions WHERE id = ?1",
            params![storyboard_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    Ok((plan.and_then(|json| serde_json::from_str(&json).ok()), note))
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 120 BPM、第一拍在 0，曲长 60 秒；乐句每 4 小节（8 秒）。
    fn steady_analysis() -> BeatAnalysis {
        let beats = (0..120).map(|beat| beat * 500).collect::<Vec<i64>>();
        let downbeats = beats.iter().copied().step_by(4).collect::<Vec<_>>();
        let phrases = downbeats.iter().copied().step_by(4).collect::<Vec<_>>();
        let bar_energy = downbeats.iter().map(|time| if *time < 8_000 { 0.2 } else { 0.8 }).collect();
        BeatAnalysis {
            version: 1,
            duration_ms: 60_000,
            tempo_bpm: 120.0,
            confidence: 0.8,
            beats_ms: beats,
            downbeats_ms: downbeats,
            phrase_starts_ms: phrases,
            bar_energy,
        }
    }

    fn choice() -> MusicChoice {
        MusicChoice {
            asset_id: "m".into(),
            title: "Summer Fun".into(),
            source: "library",
            duration_ms: 60_000,
            license: None,
            analysis: Some(steady_analysis()),
            analysis_note: None,
        }
    }

    #[test]
    fn window_starts_on_a_phrase_and_ends_on_a_phrase_near_target() {
        let plan = plan_window(&choice(), 30_000).unwrap();
        assert_eq!(plan.source_start_ms % 8_000, 0);
        assert_eq!(plan.duration_ms, 32_000);
        assert!(plan.ends_on_phrase);
        assert!(plan.fade_out_ms >= 800 && plan.fade_out_ms <= 2_000);
        assert_eq!(plan.beats_ms.first(), Some(&0));
        assert_eq!(plan.beats_ms.last(), Some(&32_000));
        // 起点避开低能量前奏。
        assert!(plan.source_start_ms >= 8_000);
    }

    #[test]
    fn cuts_land_on_beats_sum_to_window_and_sections_prefer_phrases() {
        let plan = plan_window(&choice(), 24_000).unwrap();
        assert_eq!(plan.duration_ms, 24_000);
        let preferred = vec![1_500, 2_600, 1_800, 2_100, 2_400, 1_600, 2_000, 2_000, 2_900, 1_500, 1_700, 1_900];
        let mut sections = vec![false; 12];
        sections[4] = true;
        sections[8] = true;
        let capacity = vec![i64::MAX; 12];
        let slots = snap_slots_to_beats(&preferred, &sections, &capacity, (1_500, 5_000), &plan).unwrap();
        assert_eq!(slots.len(), 12);
        assert_eq!(slots.iter().sum::<i64>(), plan.duration_ms);
        let mut cursor = 0;
        let mut cuts = Vec::new();
        for slot in &slots {
            assert!(*slot >= 1_500);
            cursor += slot;
            cuts.push(cursor);
            assert!(plan.beats_ms.contains(&cursor), "cut {cursor} off the beat grid");
        }
        // 段落切换落在乐句边界（窗口内 8s、16s）。
        assert!(plan.phrase_starts_ms.contains(&cuts[3]), "cuts {cuts:?}");
        assert!(plan.phrase_starts_ms.contains(&cuts[7]), "cuts {cuts:?}");
        // 长度有变化，不是节拍器。
        assert!(slots.iter().any(|slot| *slot != slots[0]));
    }

    #[test]
    fn too_many_shots_relax_to_single_beats_and_impossible_returns_none() {
        let plan = plan_window(&choice(), 30_000).unwrap();
        let preferred = vec![1_600; 20];
        let slots = snap_slots_to_beats(&preferred, &[false; 20], &[i64::MAX; 20], (1_500, 5_000), &plan).unwrap();
        assert_eq!(slots.iter().sum::<i64>(), 32_000);
        assert!(slots.iter().all(|slot| slot % 500 == 0 && *slot >= 500));
        let preferred = vec![500; 80];
        assert!(snap_slots_to_beats(&preferred, &[false; 80], &[i64::MAX; 80], (1_500, 5_000), &plan).is_none());
    }

    #[test]
    fn voiceover_nudge_stays_within_tolerance_and_fixed_length_ends_on_phrase() {
        let beats = (0..40).map(|beat| beat * 500).collect::<Vec<i64>>();
        let nudged = nudge_cuts_to_beats(&[2_430, 4_800, 6_250, 6_600], &beats, 120, 400, 10_000);
        assert_eq!(nudged, vec![2_500, 4_800, 6_250, 6_600]);
        // 挪到 2000 会让下一镜只剩 300ms（少于 400ms），保持原位；2300 离最近的拍超过容差，也不动。
        let nudged = nudge_cuts_to_beats(&[1_950, 2_300], &beats, 120, 400, 10_000);
        assert_eq!(nudged, vec![1_950, 2_300]);
        let plan = plan_for_fixed_length(&choice(), 27_300).unwrap();
        assert_eq!((plan.source_start_ms + plan.duration_ms) % 8_000, 0);
        assert_eq!(plan.duration_ms, 27_300);
        assert!(plan_for_fixed_length(&choice(), 70_000).is_err());
    }
}
