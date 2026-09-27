//! 昼夜顺序：按拍文字里明说的时段和素材证据 timeOfDay 过滤召回，保持简报的白天→夜晚顺序。
//! 只信证据字段（day / night），unknown 与旧证据不参与过滤；过滤后候选太少就不过滤。

use crate::models::{StoryboardBeat, StoryboardSource};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DaypartRule {
    Any,
    NotNight,
    NotDay,
}

const NIGHT_WORDS: &[&str] = &[
    "night",
    "nighttime",
    "campfire",
    "bonfire",
    "starry",
    "moonlit",
    "moonlight",
];
const DAY_WORDS: &[&str] = &[
    "day",
    "daytime",
    "daylight",
    "sunny",
    "morning",
    "midday",
    "noon",
    "afternoon",
];
const NIGHT_CJK: &[&str] = &["夜", "晚上", "篝火"];
const DAY_CJK: &[&str] = &["白天", "早晨", "上午", "中午", "下午"];

/// 拍文字（purpose / requiredVisual / visualKeywords）明说的时段；没说返回 None。
fn stated_daypart(beat: &StoryboardBeat) -> Option<&'static str> {
    let text = format!(
        "{} {} {}",
        beat.purpose,
        beat.required_visual,
        beat.visual_keywords.join(" ")
    )
    .to_lowercase();
    let words = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let has = |latin: &[&str], cjk: &[&str]| {
        words.iter().any(|word| latin.contains(word)) || cjk.iter().any(|term| text.contains(term))
    };
    match (has(NIGHT_WORDS, NIGHT_CJK), has(DAY_WORDS, DAY_CJK)) {
        (true, false) => Some("night"),
        (false, true) => Some("day"),
        _ => None,
    }
}

/// 每拍的昼夜约束：明说夜的拍不要白天素材，明说白天的拍不要夜景；
/// 第一个夜拍之前没说时段的拍不要夜景，夜拍之间的拍不要白天素材；最后一个夜拍之后不限。
pub(crate) fn beat_rules(beats: &[StoryboardBeat]) -> Vec<DaypartRule> {
    let stated = beats.iter().map(stated_daypart).collect::<Vec<_>>();
    let first_night = stated.iter().position(|value| *value == Some("night"));
    let last_night = stated.iter().rposition(|value| *value == Some("night"));
    stated
        .iter()
        .enumerate()
        .map(|(index, value)| match (value, first_night, last_night) {
            (Some("night"), _, _) => DaypartRule::NotDay,
            (Some(_), _, _) => DaypartRule::NotNight,
            (None, Some(first), _) if index < first => DaypartRule::NotNight,
            (None, Some(first), Some(last)) if index > first && index < last => DaypartRule::NotDay,
            _ => DaypartRule::Any,
        })
        .collect()
}

/// 候选段的证据时段：优先本段证据，整片候选取第一条带时段的证据。
fn source_daypart(source: &StoryboardSource) -> Option<&str> {
    let segment_id = source.segment.as_ref().map(|segment| segment.id.as_str());
    source
        .visual_evidence
        .iter()
        .filter(|evidence| segment_id.is_none() || evidence.segment_id.as_deref() == segment_id)
        .filter_map(|evidence| evidence.detail.as_ref()?.time_of_day.as_deref())
        .find(|value| matches!(*value, "day" | "night"))
}

/// 按规则剔除时段相反的候选；剩下的视频/图片候选少于 `min_keep` 时原样返回，交给召回排序。
pub(crate) fn sources_for_rule<'a>(
    sources: &'a [StoryboardSource],
    rule: DaypartRule,
    min_keep: usize,
) -> Cow<'a, [StoryboardSource]> {
    let excluded = match rule {
        DaypartRule::Any => return Cow::Borrowed(sources),
        DaypartRule::NotNight => "night",
        DaypartRule::NotDay => "day",
    };
    let kept = sources
        .iter()
        .filter(|source| source_daypart(source) != Some(excluded))
        .cloned()
        .collect::<Vec<_>>();
    let visual = kept
        .iter()
        .filter(|source| matches!(source.kind.as_str(), "video" | "image"))
        .count();
    if kept.len() == sources.len() || visual < min_keep {
        Cow::Borrowed(sources)
    } else {
        Cow::Owned(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(id: &str, visual: &str) -> StoryboardBeat {
        StoryboardBeat {
            id: id.into(),
            purpose: id.into(),
            required_visual: visual.into(),
            ..Default::default()
        }
    }

    /// 2026-09-27「Weekend Road Trip」：夜景车内镜头被选进简报白天段的第 4 拍。
    #[test]
    fn beats_before_the_first_night_beat_exclude_night_footage() {
        let beats = vec![
            beat(
                "guitar",
                "Acoustic guitar being held in the dimly lit van interior",
            ),
            beat("beach", "Grassy lakeshore next to a parked van"),
            beat("campfire", "Exterior of a van at night with a campfire"),
            beat("glow", "Dimly lit van and campfire interaction"),
        ];
        assert_eq!(
            beat_rules(&beats),
            vec![
                DaypartRule::NotNight,
                DaypartRule::NotNight,
                DaypartRule::NotDay,
                DaypartRule::NotDay,
            ]
        );
    }
}
