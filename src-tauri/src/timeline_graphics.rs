//! 品牌图层与镜头转场：规范化、按时间线贴合、解析到具体切点，以及模型工具的校验与截断。
//! 模型只能给模板 id、短文案和转场类型；时间、样式、位置由这里和模板决定，模型不给时间。
use crate::brand_kit::BrandKit;
use crate::cards::templates::{card_template, clamp_copy};
use crate::models::{
    CardSlots, CutTransition, GraphicOverlay, TimelineClip, TimelineGraphics, TimelineTransitions,
    TransitionSpec,
};

pub(crate) const TRANSITION_KINDS: [&str; 3] = ["none", "crossfade", "dip_to_black"];
pub(crate) const DEFAULT_TRANSITION_MS: i64 = 300;
const MIN_TRANSITION_MS: i64 = 200;
const MAX_TRANSITION_MS: i64 = 1000;
/// 转场不超过相邻较短镜头的 40%，短于这个值就退回硬切。
const MIN_RESOLVED_TRANSITION_MS: i64 = 100;
const MIN_CARD_MS: i64 = 800;
const MAX_INFO_CARDS: usize = 3;

pub(crate) fn normalize_transition_spec(spec: &TransitionSpec) -> Result<TransitionSpec, String> {
    if !TRANSITION_KINDS.contains(&spec.kind.as_str()) {
        return Err("Transition must be none, crossfade, or dip_to_black.".to_owned());
    }
    Ok(TransitionSpec {
        kind: spec.kind.clone(),
        duration_ms: spec.duration_ms.clamp(MIN_TRANSITION_MS, MAX_TRANSITION_MS),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedTransition {
    /// 切点左侧镜头在 clips 中的位置。
    pub after_clip: usize,
    pub kind: String,
    pub duration_ms: i64,
    pub cut_ms: i64,
}

/// 把默认值和逐刀覆盖落到每个切点；时长夹到相邻较短镜头的 40%，过短则硬切。
pub(crate) fn resolve_transitions(
    clips: &[TimelineClip],
    transitions: &TimelineTransitions,
) -> Vec<ResolvedTransition> {
    let mut resolved = Vec::new();
    for (index, pair) in clips.windows(2).enumerate() {
        let (left, right) = (&pair[0], &pair[1]);
        let spec = transitions
            .cuts
            .iter()
            .find(|cut| cut.after_shot_index == left.shot_index)
            .map(|cut| (cut.kind.as_str(), cut.duration_ms))
            .or_else(|| transitions.default.as_ref().map(|spec| (spec.kind.as_str(), spec.duration_ms)));
        let Some((kind, duration_ms)) = spec else { continue };
        if kind == "none" || !TRANSITION_KINDS.contains(&kind) {
            continue;
        }
        let shorter = (left.timeline_end_ms - left.timeline_start_ms)
            .min(right.timeline_end_ms - right.timeline_start_ms);
        let duration_ms = duration_ms.min(shorter * 2 / 5);
        if duration_ms < MIN_RESOLVED_TRANSITION_MS {
            continue;
        }
        resolved.push(ResolvedTransition {
            after_clip: index,
            kind: kind.to_owned(),
            duration_ms,
            cut_ms: left.timeline_end_ms,
        });
    }
    resolved
}

fn default_duration(template_id: &str) -> i64 {
    card_template(template_id).map_or(3000, |template| template.manifest.default_duration_ms)
}

fn with_timing(overlay: &GraphicOverlay, start_ms: i64, end_ms: i64) -> GraphicOverlay {
    let span = end_ms - start_ms;
    let fade = 350.min(span / 4);
    GraphicOverlay {
        start_ms,
        end_ms,
        fade_in_ms: fade,
        // 片尾卡停在最后一帧，不淡出成黑。
        fade_out_ms: if overlay.anchor == "closing" { 0 } else { fade },
        ..overlay.clone()
    }
}

/// 每个新时间线版本都重新贴合：开场 / 片尾 / 全程 / 某镜头上的时间由后端按当前镜头算。
/// 锚定的镜头被删掉的信息卡随之移除；切点左侧镜头已不在的逐刀转场也移除。
pub(crate) fn fit_graphics(graphics: &TimelineGraphics, clips: &[TimelineClip]) -> TimelineGraphics {
    let total = clips.iter().map(|clip| clip.timeline_end_ms).max().unwrap_or(0);
    let cap = |template_id: &str| default_duration(template_id).min(total * 2 / 5);
    let overlays = &graphics.graphic_overlays;
    let by_anchor = |anchor: &'static str| overlays.iter().filter(move |overlay| overlay.anchor == anchor);
    let opening = by_anchor("opening").next().and_then(|overlay| {
        let span = cap(&overlay.template_id);
        (span >= MIN_CARD_MS).then(|| with_timing(overlay, 0, span))
    });
    let closing = by_anchor("closing").next().and_then(|overlay| {
        let span = cap(&overlay.template_id);
        (span >= MIN_CARD_MS).then(|| with_timing(overlay, total - span, total))
    });
    let opening_end = opening.as_ref().map_or(0, |overlay| overlay.end_ms);
    let closing_start = closing.as_ref().map_or(total, |overlay| overlay.start_ms);
    let whole = by_anchor("whole").next().and_then(|overlay| {
        (closing_start - opening_end >= 1000)
            .then(|| with_timing(overlay, opening_end, closing_start))
    });
    let mut at_shot = Vec::<GraphicOverlay>::new();
    let mut candidates = by_anchor("at_shot")
        .filter_map(|overlay| {
            let clip = clips
                .iter()
                .find(|clip| Some(clip.shot_index) == overlay.anchor_shot_index)?;
            let length = clip.timeline_end_ms - clip.timeline_start_ms;
            let start = (clip.timeline_start_ms + 200.min(length / 5)).max(opening_end);
            let end = (start + default_duration(&overlay.template_id)).min(closing_start);
            (end - start >= MIN_CARD_MS).then(|| with_timing(overlay, start, end))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|overlay| overlay.start_ms);
    for overlay in candidates {
        if at_shot.last().map_or(true, |previous| previous.end_ms <= overlay.start_ms)
            && at_shot.len() < MAX_INFO_CARDS
        {
            at_shot.push(overlay);
        }
    }
    // 层序从下到上：角标、信息卡、开场卡、片尾卡。
    let graphic_overlays = whole
        .into_iter()
        .chain(at_shot)
        .chain(opening)
        .chain(closing)
        .collect();
    let left_shots = clips
        .iter()
        .take(clips.len().saturating_sub(1))
        .map(|clip| clip.shot_index)
        .collect::<Vec<_>>();
    TimelineGraphics {
        graphic_overlays,
        transitions: TimelineTransitions {
            default: graphics.transitions.default.clone(),
            cuts: graphics
                .transitions
                .cuts
                .iter()
                .filter(|cut| left_shots.contains(&cut.after_shot_index))
                .cloned()
                .collect(),
        },
    }
}

pub(crate) struct CardRequest<'a> {
    pub template_id: &'a str,
    pub shot_index: Option<i64>,
    pub headline: Option<&'a str>,
    pub subline: Option<&'a str>,
    pub cta: Option<&'a str>,
}

/// 按模板 manifest 校验并截断一张卡；返回卡片和给模型看的改动说明。
pub(crate) fn plan_card(request: &CardRequest, kit: &BrandKit) -> Result<(GraphicOverlay, Vec<String>), String> {
    let template = card_template(request.template_id)
        .ok_or_else(|| format!("Unknown card template {}.", request.template_id))?;
    let manifest = &template.manifest;
    if manifest.requires_logo && kit.logo_file.is_none() {
        return Err(format!("{} needs a brand logo; ask the user to add one in Project settings.", manifest.id));
    }
    if manifest.requires_brand && !kit.is_set() {
        return Err(format!("{} needs a brand name or logo; ask the user to set the brand kit in Project settings.", manifest.id));
    }
    let mut notes = Vec::new();
    let end_card = manifest.id == "end_card";
    let fallback = |slot: &str| -> Option<&str> {
        match (end_card, slot) {
            (true, "headline") => Some(kit.name.as_str()),
            (true, "subline") => Some(kit.handle.as_str()),
            (true, "cta") => Some(kit.cta.as_str()),
            _ => None,
        }
        .filter(|value| !value.trim().is_empty())
    };
    let mut slot = |name: &str, raw: Option<&str>| -> Result<Option<String>, String> {
        let raw = raw.map(str::trim).filter(|value| !value.is_empty());
        let Some(limit) = manifest.slots.get(name).copied() else {
            if raw.is_some() {
                notes.push(format!("{name} ignored: {} has no {name} slot", manifest.id));
            }
            return Ok(None);
        };
        let Some(raw) = raw.or_else(|| fallback(name)) else {
            return if limit.required {
                Err(format!("{} needs a {name}.", manifest.id))
            } else {
                Ok(None)
            };
        };
        let (value, changed) = clamp_copy(raw, limit);
        if changed {
            notes.push(format!(
                "{name} shortened to {:?} (limit {} words or {} CJK characters)",
                value.as_deref().unwrap_or(""),
                limit.max_words,
                limit.max_cjk_chars
            ));
        }
        if value.is_none() && limit.required {
            return Err(format!("{} needs a {name} with readable words.", manifest.id));
        }
        Ok(value)
    };
    let slots = CardSlots {
        headline: slot("headline", request.headline)?,
        subline: slot("subline", request.subline)?,
        cta: slot("cta", request.cta)?,
    };
    if end_card && slots == CardSlots::default() && kit.logo_file.is_none() {
        return Err("end_card has nothing to show; set a brand name, handle, CTA, or logo.".to_owned());
    }
    let anchor_shot_index = if manifest.anchor == "at_shot" {
        Some(request.shot_index.ok_or_else(|| format!("{} needs shotIndex.", manifest.id))?)
    } else {
        None
    };
    let id = match anchor_shot_index {
        Some(shot) => format!("card-{}-{shot}", manifest.id),
        None => format!("card-{}", manifest.id),
    };
    Ok((
        GraphicOverlay {
            id,
            template_id: manifest.id.clone(),
            anchor: manifest.anchor.clone(),
            anchor_shot_index,
            slots,
            brand: kit.snapshot(),
            start_ms: 0,
            end_ms: 0,
            fade_in_ms: 0,
            fade_out_ms: 0,
        },
        notes,
    ))
}

/// 同一模板的开场 / 片尾 / 角标只保留一张；信息卡按镜头去重。先按 remove 列表移除。
pub(crate) fn upsert_cards(
    existing: &[GraphicOverlay],
    cards: Vec<GraphicOverlay>,
    remove_template_ids: &[String],
) -> Result<Vec<GraphicOverlay>, String> {
    let mut merged = existing
        .iter()
        .filter(|overlay| !remove_template_ids.contains(&overlay.template_id))
        .cloned()
        .collect::<Vec<_>>();
    for card in cards {
        merged.retain(|overlay| overlay.id != card.id);
        merged.push(card);
    }
    if merged.iter().filter(|overlay| overlay.anchor == "at_shot").count() > MAX_INFO_CARDS {
        return Err(format!("At most {MAX_INFO_CARDS} info cards fit one video."));
    }
    Ok(merged)
}

/// 生成时自动加的品牌卡：设了品牌套件才加；开场标题用故事版标题（模型生成，这里截断）。
pub(crate) fn auto_brand_cards(kit: &BrandKit, title: &str) -> Vec<GraphicOverlay> {
    if !kit.is_set() {
        return Vec::new();
    }
    let mut cards = Vec::new();
    if !title.trim().is_empty() {
        if let Ok((card, _)) = plan_card(
            &CardRequest { template_id: "opening_title", shot_index: None, headline: Some(title), subline: None, cta: None },
            kit,
        ) {
            cards.push(card);
        }
    }
    for template_id in ["end_card", "corner_logo"] {
        if let Ok((card, _)) = plan_card(
            &CardRequest { template_id, shot_index: None, headline: None, subline: None, cta: None },
            kit,
        ) {
            cards.push(card);
        }
    }
    cards
}

const TRANSITION_WORDS: &[&str] = &[
    "transition", "dissolve", "crossfade", "cross-fade", "cross fade", "fade", "dip to black", "hard cut",
    "转场", "叠化", "过渡", "淡入", "淡出", "黑场", "闪黑", "硬切",
];
const CARD_WORDS: &[&str] = &[
    "title", "card", "logo", "brand", "intro", "outro", "ending", "end screen", "cta", "call to action",
    "headline", "watermark", "标题", "片头", "片尾", "字卡", "卡片", "品牌", "角标", "结尾", "开场", "水印", "口号",
];

fn mentions(request: &str, words: &[&str]) -> bool {
    let lower = request.to_lowercase();
    words.iter().any(|word| lower.contains(word))
}

/// 转场和品牌卡是用户的选择：本轮原话没提到时拒绝模型自行改动（同 media_options 的守卫）。
pub(crate) fn user_asked_for_transitions(request: &str) -> bool {
    mentions(request, TRANSITION_WORDS)
}

pub(crate) fn user_asked_for_cards(request: &str) -> bool {
    mentions(request, CARD_WORDS)
}

/// 模型的 set_transitions：afterShotIndices 为空时改默认值并清掉逐刀覆盖。
pub(crate) fn apply_transition_request(
    current: &TimelineTransitions,
    clips: &[TimelineClip],
    spec: TransitionSpec,
    after_shot_indices: Option<&[i64]>,
) -> Result<TimelineTransitions, String> {
    let spec = normalize_transition_spec(&spec)?;
    match after_shot_indices {
        None => Ok(TimelineTransitions { default: Some(spec), cuts: Vec::new() }),
        Some(indices) => {
            let left_shots = clips
                .iter()
                .take(clips.len().saturating_sub(1))
                .map(|clip| clip.shot_index)
                .collect::<Vec<_>>();
            if let Some(missing) = indices.iter().find(|index| !left_shots.contains(index)) {
                return Err(format!("Shot {missing} is not followed by another shot on this timeline."));
            }
            let mut cuts = current
                .cuts
                .iter()
                .filter(|cut| !indices.contains(&cut.after_shot_index))
                .cloned()
                .collect::<Vec<_>>();
            cuts.extend(indices.iter().map(|index| CutTransition {
                after_shot_index: *index,
                kind: spec.kind.clone(),
                duration_ms: spec.duration_ms,
            }));
            Ok(TimelineTransitions { default: current.default.clone(), cuts })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(shot_index: i64, start: i64, end: i64) -> TimelineClip {
        TimelineClip { shot_index, timeline_start_ms: start, timeline_end_ms: end, ..TimelineClip::default() }
    }

    fn kit() -> BrandKit {
        BrandKit { name: "Voycut".to_owned(), handle: "voycut.com".to_owned(), logo_file: Some("logo-1.png".to_owned()), ..BrandKit::default() }
    }

    #[test]
    fn transitions_are_clamped_to_the_shorter_neighbour_and_follow_shots() {
        let clips = vec![clip(1, 0, 2_000), clip(2, 2_000, 2_400), clip(3, 2_400, 6_000)];
        let transitions = TimelineTransitions {
            default: Some(TransitionSpec { kind: "crossfade".to_owned(), duration_ms: 300 }),
            cuts: vec![CutTransition { after_shot_index: 9, kind: "dip_to_black".to_owned(), duration_ms: 500 }],
        };
        let resolved = resolve_transitions(&clips, &transitions);
        assert_eq!(resolved.iter().map(|t| (t.after_clip, t.duration_ms)).collect::<Vec<_>>(), [(0, 160), (1, 160)]);
        assert!(fit_graphics(&TimelineGraphics { transitions, ..Default::default() }, &clips).transitions.cuts.is_empty());
    }

    #[test]
    fn auto_cards_fit_opening_closing_and_corner_without_overlap() {
        let cards = auto_brand_cards(&kit(), "A weekend road trip along the coast");
        let clips = vec![clip(1, 0, 10_000), clip(2, 10_000, 20_000)];
        let fitted = fit_graphics(&TimelineGraphics { graphic_overlays: cards, ..Default::default() }, &clips);
        let spans = fitted
            .graphic_overlays
            .iter()
            .map(|o| (o.template_id.as_str(), o.start_ms, o.end_ms))
            .collect::<Vec<_>>();
        assert_eq!(spans, [("corner_logo", 2_800, 17_000), ("opening_title", 0, 2_800), ("end_card", 17_000, 20_000)]);
        assert_eq!(fitted.graphic_overlays[1].slots.headline.as_deref(), Some("A weekend road trip along the"));
        assert!(auto_brand_cards(&BrandKit::default(), "Title").is_empty());
    }

    #[test]
    fn model_card_requests_are_validated_against_the_manifest() {
        let request = CardRequest { template_id: "info_card", shot_index: None, headline: Some("Fact"), subline: None, cta: Some("Buy") };
        assert!(plan_card(&request, &kit()).is_err(), "info cards need a shot");
        let (card, notes) = plan_card(&CardRequest { shot_index: Some(2), ..request }, &kit()).unwrap();
        assert_eq!(card.slots.cta, None);
        assert!(notes[0].starts_with("cta ignored"));
        assert!(plan_card(&CardRequest { template_id: "corner_logo", shot_index: None, headline: None, subline: None, cta: None }, &BrandKit { name: "X".to_owned(), ..BrandKit::default() }).is_err());
        assert!(!user_asked_for_transitions("Make a 30 second recap"));
        assert!(user_asked_for_cards("加一个片尾"));
    }
}
