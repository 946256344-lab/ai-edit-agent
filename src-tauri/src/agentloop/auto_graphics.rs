//! 品牌卡与转场：generate_storyboard 的自动收尾，以及 add_title_cards / set_transitions 两个工具。
//! 模型只给模板 id、短文案和转场类型；本轮原话没提到时拒绝改动，时间与样式由后端决定。
use super::auto_music::MusicScope;
use super::schema::LoopState;
use crate::brand_kit::{read_brand_kit, read_default_transition};
use crate::models::{TimelineGraphics, TimelineVersion, TransitionSpec};
use crate::timeline::insert_timeline_version_with_graphics;
use crate::timeline_graphics::{
    apply_transition_request, auto_brand_cards, plan_card, resolve_transitions, upsert_cards,
    user_asked_for_cards, user_asked_for_transitions, CardRequest, DEFAULT_TRANSITION_MS,
};
use serde_json::{json, Value};

fn new_version(
    scope: &MusicScope,
    timeline: &TimelineVersion,
    operation: &str,
    graphics: &TimelineGraphics,
) -> Result<TimelineVersion, String> {
    insert_timeline_version_with_graphics(
        scope.connection,
        scope.project_id,
        scope.editing_task_id,
        scope.conversation_id,
        scope.agent_task_id,
        timeline,
        operation,
        timeline.clips.clone(),
        timeline.text_tracks.clone(),
        timeline.music_tracks.clone(),
        timeline.voiceover_tracks.clone(),
        graphics,
    )
}

/// 生成后的自动收尾：设了品牌套件加开场 / 片尾 / 角标，项目默认转场不是硬切时写入默认转场。
/// 没有可加的内容时返回 None，不新建版本。
pub(super) fn apply_finishing_defaults(
    scope: &MusicScope,
    timeline: &TimelineVersion,
    storyboard_title: &str,
) -> Result<Option<TimelineVersion>, String> {
    let mut graphics = timeline.graphics.clone();
    let mut changed = false;
    if graphics.graphic_overlays.is_empty() {
        let kit = read_brand_kit(scope.connection, scope.project_id);
        let cards = auto_brand_cards(&kit, storyboard_title);
        if !cards.is_empty() {
            graphics.graphic_overlays = cards;
            changed = true;
        }
    }
    let default_transition = read_default_transition(scope.connection, scope.project_id);
    if graphics.transitions.default.is_none()
        && graphics.transitions.cuts.is_empty()
        && default_transition.kind != "none"
    {
        graphics.transitions.default = Some(default_transition);
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    new_version(scope, timeline, "apply_brand_and_transition_defaults", &graphics).map(Some)
}

/// 给模型和 appliedMedia 看的实际落地结果：哪些卡在什么时间，哪些切点有转场。
pub(crate) fn graphics_summary(timeline: &TimelineVersion) -> Value {
    json!({
        "brandCards": timeline.graphics.graphic_overlays.iter().map(|overlay| json!({
            "templateId": overlay.template_id,
            "startMs": overlay.start_ms,
            "endMs": overlay.end_ms,
        })).collect::<Vec<_>>(),
        "transitions": resolve_transitions(&timeline.clips, &timeline.graphics.transitions)
            .into_iter()
            .map(|transition| json!({
                "afterShotIndex": timeline.clips[transition.after_clip].shot_index,
                "kind": transition.kind,
                "durationMs": transition.duration_ms,
            }))
            .collect::<Vec<_>>(),
    })
}

pub(super) fn add_title_cards(
    state: &LoopState,
    scope: &MusicScope,
    timeline: &TimelineVersion,
    args: &Value,
) -> Result<(TimelineVersion, Value), String> {
    if !user_asked_for_cards(&state.user_request) {
        return Err("The user did not ask for a title, card, or logo this turn. Brand cards are added automatically when a brand kit is set; do not add them on your own.".to_owned());
    }
    let kit = read_brand_kit(scope.connection, scope.project_id);
    let mut cards = Vec::new();
    let mut adjustments = Vec::new();
    for card in args["cards"].as_array().into_iter().flatten() {
        let text = |key: &str| card.get(key).and_then(Value::as_str);
        let request = CardRequest {
            template_id: text("templateId").unwrap_or_default(),
            shot_index: card.get("shotIndex").and_then(Value::as_i64),
            headline: text("headline"),
            subline: text("subline"),
            cta: text("cta"),
        };
        let (overlay, notes) = plan_card(&request, &kit)?;
        adjustments.extend(notes.into_iter().map(|note| format!("{}: {note}", overlay.template_id)));
        cards.push(overlay);
    }
    let remove = args["removeTemplateIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let requested = cards.iter().map(|card| card.id.clone()).collect::<Vec<_>>();
    let graphics = TimelineGraphics {
        graphic_overlays: upsert_cards(&timeline.graphics.graphic_overlays, cards, &remove)?,
        transitions: timeline.graphics.transitions.clone(),
    };
    let created = new_version(scope, timeline, "add_title_cards", &graphics)?;
    let placed = created
        .graphics
        .graphic_overlays
        .iter()
        .map(|overlay| overlay.id.clone())
        .collect::<Vec<_>>();
    let not_placed = requested
        .into_iter()
        .filter(|id| !placed.contains(id))
        .collect::<Vec<_>>();
    let mut result = json!({
        "tool": "add_title_cards",
        "status": "ok",
        "timelineVersionId": created.id,
        "versionNumber": created.version_number,
        "cards": graphics_summary(&created)["brandCards"],
        "copyAdjustments": adjustments,
        "editableInEditor": false,
        "note": "Brand cards reach the editor as images; tell the user their text cannot be edited there.",
    });
    if !not_placed.is_empty() {
        result["notPlaced"] = json!({
            "cards": not_placed,
            "reason": "The video is too short for these cards, or they would overlap another card.",
        });
    }
    Ok((created, result))
}

pub(super) fn set_transitions(
    state: &LoopState,
    scope: &MusicScope,
    timeline: &TimelineVersion,
    args: &Value,
) -> Result<(TimelineVersion, Value), String> {
    if !user_asked_for_transitions(&state.user_request) {
        return Err("The user did not ask for transitions this turn; the project default stays in place. Do not change transitions on your own.".to_owned());
    }
    let spec = TransitionSpec {
        kind: args["kind"].as_str().unwrap_or_default().to_owned(),
        duration_ms: args["durationMs"].as_i64().unwrap_or(DEFAULT_TRANSITION_MS),
    };
    let indices = args["afterShotIndices"].as_array().map(|items| {
        items.iter().filter_map(Value::as_i64).collect::<Vec<_>>()
    });
    let transitions = apply_transition_request(
        &timeline.graphics.transitions,
        &timeline.clips,
        spec,
        indices.as_deref(),
    )?;
    let graphics = TimelineGraphics {
        graphic_overlays: timeline.graphics.graphic_overlays.clone(),
        transitions,
    };
    let created = new_version(scope, timeline, "set_transitions", &graphics)?;
    let resolved = graphics_summary(&created)["transitions"].clone();
    Ok((
        created.clone(),
        json!({
            "tool": "set_transitions",
            "status": "ok",
            "timelineVersionId": created.id,
            "versionNumber": created.version_number,
            "resolvedTransitions": resolved,
            "note": "Cuts next to very short shots stay hard cuts. FCPXML and OTIO carry crossfades only; dip_to_black reaches Jianying and CapCut as their native flash-black transition.",
        }),
    ))
}
