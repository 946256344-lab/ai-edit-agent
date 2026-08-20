//! 版本化内部 timeline 的创建、片段替换、时长调整、排序、文字和音乐编辑。
//! 每次编辑都创建新版本并校验素材作用域与源时间范围，不覆盖旧版本。
use crate::db::{now_millis, open_connection};
use crate::models::{
    LatestTimeline, MusicTrack, PreviewQualityReport, PreviewResult, TechnicalMetadata,
    TextAnimation, TextCue, TextLayout, TextStyle, TextTrack, TimelineClip, TimelineContent,
    TimelineVersion,
};
use crate::storyboard::load_storyboard_version;
use rusqlite::{params, Connection, OptionalExtension};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

fn storyboard_text_tracks(storyboard: &crate::models::StoryboardVersion) -> Vec<TextTrack> {
    let mut cues = Vec::new();
    let mut cursor = 0_i64;
    for shot in &storyboard.shots {
        let start_ms = cursor;
        let end_ms = start_ms + shot.duration_ms;
        cursor = end_ms;
        let text = shot.on_screen_text.trim();
        if text.is_empty() {
            continue;
        }
        cues.push(TextCue {
            id: format!("shot-{}-subtitle", shot.order_index),
            template_id: Some("subtitle_safe".to_owned()),
            start_ms,
            end_ms,
            text: text.chars().take(280).collect(),
            style: TextStyle::default(),
            layout: TextLayout::default(),
            entrance: Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 180,
                intensity: 0.6,
            }),
            exit: Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 160,
                intensity: 0.5,
            }),
            loop_animation: None,
            jianying_compatibility: "verified".to_owned(),
        });
    }
    if cues.is_empty() {
        Vec::new()
    } else {
        vec![TextTrack {
            id: "storyboard-subtitles".to_owned(),
            role: "subtitle".to_owned(),
            layer: 1,
            enabled: true,
            origin: "storyboard_generated".to_owned(),
            generation_id: None,
            editable: true,
            locked: false,
            cues,
        }]
    }
}

#[tauri::command]
/// 从指定 storyboard 创建新的 timeline v1；镜头源范围来自 storyboard 证据，不从文件名推断。
pub fn create_timeline_draft(
    app: AppHandle,
    project_id: String,
    storyboard_version_id: String,
) -> Result<TimelineVersion, String> {
    let connection = open_connection(&app)?;
    let storyboard = load_storyboard_version(&connection, &storyboard_version_id)?;
    if storyboard.project_id != project_id {
        return Err("Storyboard does not belong to this project.".to_owned());
    }
    // timeline 区间首尾相接，源区间保持不变；后续编辑也只创建新版本，不修改此版本。
    let mut cursor = 0_i64;
    let clips = storyboard
        .shots
        .iter()
        .map(|shot| {
            let end = cursor + shot.duration_ms;
            let clip = TimelineClip {
                shot_index: shot.order_index,
                asset_id: shot.asset_id.clone(),
                source_start_ms: shot.source_start_ms,
                source_end_ms: shot.source_end_ms,
                timeline_start_ms: cursor,
                timeline_end_ms: end,
                on_screen_text: shot.on_screen_text.clone(),
                clip_kind: "source".to_owned(),
                derived_from_shot_index: None,
                fit_reason: None,
            };
            cursor = end;
            clip
        })
        .collect::<Vec<_>>();
    let text_tracks = storyboard_text_tracks(&storyboard);
    let version_number = connection.query_row(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM timeline_versions WHERE project_id = ?1",
        params![project_id], |row| row.get::<_, i64>(0),
    ).map_err(|error| error.to_string())?;
    let version = TimelineVersion {
        id: Uuid::new_v4().to_string(),
        project_id,
        storyboard_version_id,
        version_number,
        clips,
        text_tracks,
        music_tracks: Vec::new(),
        voiceover_tracks: Vec::new(),
        quality_report: None,
        created_at: now_millis(),
    };
    connection.execute(
        "INSERT INTO timeline_versions (id, project_id, storyboard_version_id, version_number, status, content_json, created_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
        params![version.id, version.project_id, version.storyboard_version_id, version.version_number, serde_json::to_string(&version.to_content()).map_err(|error| error.to_string())?, version.created_at],
    ).map_err(|error| error.to_string())?;
    Ok(version)
}

#[derive(Clone)]
pub(crate) struct ClipReplacement {
    pub(crate) shot_index: i64,
    pub(crate) asset_id: String,
    pub(crate) source_start_ms: i64,
    pub(crate) source_end_ms: i64,
}

#[derive(Clone)]
pub(crate) struct ClipAdjustment {
    pub(crate) shot_index: i64,
    pub(crate) new_duration_ms: Option<i64>,
    pub(crate) new_source_start_ms: Option<i64>,
}

fn duplicate_shot_index(shots: &[i64]) -> Option<i64> {
    let mut seen = std::collections::HashSet::new();
    shots.iter().copied().find(|shot| !seen.insert(*shot))
}

fn asset_kind_and_metadata(
    connection: &Connection,
    project_id: &str,
    asset_id: &str,
) -> Result<(String, TechnicalMetadata), String> {
    let (kind, metadata_json): (String, String) = connection
        .query_row(
            "SELECT kind, metadata_json FROM assets WHERE id = ?1 AND project_id = ?2 AND analysis_status = 'ready'",
            params![asset_id, project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| "Replacement asset is unavailable or has not finished analysis.".to_owned())?;
    let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
    Ok((kind, metadata))
}

fn recompute_timeline_positions(clips: &mut [TimelineClip]) {
    let mut cursor = 0_i64;
    for clip in clips {
        let duration = clip.timeline_end_ms - clip.timeline_start_ms;
        clip.timeline_start_ms = cursor;
        clip.timeline_end_ms = cursor + duration;
        cursor += duration;
    }
}

pub(crate) fn insert_timeline_version_with_log(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    timeline: &TimelineVersion,
    operation_type: &str,
    clips: Vec<TimelineClip>,
    text_tracks: Vec<TextTrack>,
    music_tracks: Vec<MusicTrack>,
    voiceover_tracks: Vec<crate::models::VoiceoverTrack>,
) -> Result<TimelineVersion, String> {
    let version_number = connection
        .query_row(
            "SELECT COALESCE(MAX(version_number), 0) + 1 FROM timeline_versions WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    let version = TimelineVersion {
        id: Uuid::new_v4().to_string(),
        project_id: project_id.to_owned(),
        storyboard_version_id: timeline.storyboard_version_id.clone(),
        version_number,
        clips,
        text_tracks,
        music_tracks,
        voiceover_tracks,
        quality_report: None,
        created_at: now_millis(),
    };
    let content_json =
        serde_json::to_string(&version.to_content()).map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO timeline_versions (id, project_id, storyboard_version_id, version_number, status, content_json, created_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
            params![version.id, version.project_id, version.storyboard_version_id, version.version_number, content_json, version.created_at],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO operation_logs (id, project_id, editing_task_id, conversation_id, agent_task_id, actor, operation_type, entity_type, entity_id, before_json, after_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'agent', ?6, 'timeline_version', ?7, ?8, ?9, ?10)",
            params![Uuid::new_v4().to_string(), project_id, editing_task_id, conversation_id, agent_task_id, operation_type, version.id, serde_json::to_string(&timeline.to_content()).map_err(|error| error.to_string())?, serde_json::to_string(&version.to_content()).map_err(|error| error.to_string())?, now_millis()],
        )
        .map_err(|error| error.to_string())?;
    Ok(version)
}

fn apply_text_template(cue: &mut TextCue, track_role: &str) -> Result<(), String> {
    let Some(template_id) = cue.template_id.as_deref() else {
        return Ok(());
    };
    let expected_role = match template_id {
        "subtitle_safe" => "subtitle",
        "headline_rise" => "headline",
        "headline_pop" => "headline",
        "headline_drop" => "headline",
        "callout_card" => "callout",
        "cta_card" => "cta",
        _ => return Err("Text template is unsupported.".to_owned()),
    };
    if track_role != expected_role {
        return Err("Text template does not match the text track role.".to_owned());
    }
    let (style, layout, entrance, exit) = match template_id {
        "subtitle_safe" => (
            TextStyle {
                font_key: "jianying_default".to_owned(),
                font_size: 0.055,
                bold: true,
                color: "#FFFFFF".to_owned(),
                stroke_color: None,
                stroke_width: 0.0,
                shadow: false,
                background_color: None,
                alignment: "center".to_owned(),
                letter_spacing: 0,
                line_spacing: 0,
            },
            TextLayout {
                anchor: "bottom".to_owned(),
                x: 0.5,
                y: 0.82,
                max_width: 0.86,
                safe_area: "title_safe".to_owned(),
            },
            Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 180,
                intensity: 0.6,
            }),
            Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 160,
                intensity: 0.5,
            }),
        ),
        "headline_rise" => (
            TextStyle {
                font_key: "jianying_default".to_owned(),
                font_size: 0.10,
                bold: true,
                color: "#FFFFFF".to_owned(),
                stroke_color: None,
                stroke_width: 0.0,
                shadow: false,
                background_color: None,
                alignment: "center".to_owned(),
                letter_spacing: 2,
                line_spacing: 0,
            },
            TextLayout {
                anchor: "center".to_owned(),
                x: 0.5,
                y: 0.40,
                max_width: 0.80,
                safe_area: "action_safe".to_owned(),
            },
            Some(TextAnimation {
                template_id: "slide_up".to_owned(),
                duration_ms: 250,
                intensity: 0.7,
            }),
            Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 180,
                intensity: 0.5,
            }),
        ),
        "headline_pop" => (
            TextStyle {
                font_key: "jianying_default".to_owned(),
                font_size: 0.10,
                bold: true,
                color: "#FFFFFF".to_owned(),
                stroke_color: None,
                stroke_width: 0.0,
                shadow: false,
                background_color: None,
                alignment: "center".to_owned(),
                letter_spacing: 2,
                line_spacing: 0,
            },
            TextLayout {
                anchor: "center".to_owned(),
                x: 0.5,
                y: 0.40,
                max_width: 0.80,
                safe_area: "action_safe".to_owned(),
            },
            Some(TextAnimation {
                template_id: "pop".to_owned(),
                duration_ms: 220,
                intensity: 0.7,
            }),
            Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 180,
                intensity: 0.5,
            }),
        ),
        "headline_drop" => (
            TextStyle {
                font_key: "jianying_default".to_owned(),
                font_size: 0.10,
                bold: true,
                color: "#FFFFFF".to_owned(),
                stroke_color: None,
                stroke_width: 0.0,
                shadow: false,
                background_color: None,
                alignment: "center".to_owned(),
                letter_spacing: 2,
                line_spacing: 0,
            },
            TextLayout {
                anchor: "center".to_owned(),
                x: 0.5,
                y: 0.40,
                max_width: 0.80,
                safe_area: "action_safe".to_owned(),
            },
            Some(TextAnimation {
                template_id: "slide_down".to_owned(),
                duration_ms: 250,
                intensity: 0.7,
            }),
            Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 180,
                intensity: 0.5,
            }),
        ),
        "callout_card" => (
            TextStyle {
                font_key: "jianying_sans_bold".to_owned(),
                font_size: 0.065,
                bold: true,
                color: "#FFFFFF".to_owned(),
                stroke_color: None,
                stroke_width: 0.0,
                shadow: true,
                background_color: Some("#101828".to_owned()),
                alignment: "center".to_owned(),
                letter_spacing: 0,
                line_spacing: 0,
            },
            TextLayout {
                anchor: "center".to_owned(),
                x: 0.5,
                y: 0.62,
                max_width: 0.76,
                safe_area: "action_safe".to_owned(),
            },
            Some(TextAnimation {
                template_id: "pop".to_owned(),
                duration_ms: 220,
                intensity: 0.7,
            }),
            None,
        ),
        "cta_card" => (
            TextStyle {
                font_key: "jianying_harmony_bold".to_owned(),
                font_size: 0.07,
                bold: true,
                color: "#FFFFFF".to_owned(),
                stroke_color: None,
                stroke_width: 0.0,
                shadow: true,
                background_color: Some("#E11D48".to_owned()),
                alignment: "center".to_owned(),
                letter_spacing: 1,
                line_spacing: 0,
            },
            TextLayout {
                anchor: "bottom".to_owned(),
                x: 0.5,
                y: 0.74,
                max_width: 0.72,
                safe_area: "title_safe".to_owned(),
            },
            Some(TextAnimation {
                template_id: "pop".to_owned(),
                duration_ms: 240,
                intensity: 0.7,
            }),
            None,
        ),
        _ => unreachable!("template role was checked above"),
    };
    cue.style = style;
    cue.layout = layout;
    cue.entrance = entrance;
    cue.exit = exit;
    cue.loop_animation = None;
    Ok(())
}

pub(crate) fn text_recipe_capabilities() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"templateId": "subtitle_safe", "role": "subtitle", "purpose": "Bottom subtitle with a soft fade in and fade out", "selectionHint": "Use for spoken dialogue, narration, or essential comprehension. Keep it to one subtitle track and do not use it as a decorative title.", "preview": "supported", "jianying": "verified"}),
        serde_json::json!({"templateId": "headline_rise", "role": "headline", "purpose": "Centered headline with an upward entrance and fade exit", "selectionHint": "Use for a positive progression, reveal, or opening statement. Use one concise headline per visual beat.", "preview": "supported", "jianying": "verified"}),
        serde_json::json!({"templateId": "headline_pop", "role": "headline", "purpose": "Centered headline with a pop entrance and fade exit", "selectionHint": "Use for a surprise, key result, contrast, or moment that needs immediate emphasis. Do not repeat it for ordinary narration.", "preview": "supported", "jianying": "verified"}),
        serde_json::json!({"templateId": "headline_drop", "role": "headline", "purpose": "Centered headline with a top-down entrance and fade exit", "selectionHint": "Use for a conclusion, rule, warning, or decisive statement. Do not layer it over another headline in the same beat.", "preview": "supported", "jianying": "verified"}),
        serde_json::json!({"templateId": "callout_card", "role": "callout", "purpose": "Dark callout card with shadow and pop", "selectionHint": "Use only for an optional supporting fact in a local preview; it is not Jianying deliverable yet.", "preview": "supported", "jianying": "local_preview_only"}),
        serde_json::json!({"templateId": "cta_card", "role": "cta", "purpose": "High-contrast CTA card with shadow and pop", "selectionHint": "Use only for a final call to action in a local preview; it is not Jianying deliverable yet.", "preview": "supported", "jianying": "local_preview_only"}),
    ]
}

pub(crate) fn text_track_quality_warnings(tracks: &[TextTrack]) -> Vec<String> {
    let mut warnings = Vec::new();
    for track in tracks {
        for cue in &track.cues {
            let duration_ms = cue.end_ms - cue.start_ms;
            let visible_characters = cue
                .text
                .chars()
                .filter(|character| !character.is_whitespace())
                .count();
            let readable_limit = ((duration_ms / 125).max(8)) as usize;
            if visible_characters > readable_limit {
                warnings.push(format!("{}: readability_density", cue.id));
            }
            if cue.text.lines().count() > 2 {
                warnings.push(format!("{}: more_than_two_lines", cue.id));
            }
            let animation_ms = cue
                .entrance
                .as_ref()
                .map_or(0, |animation| animation.duration_ms)
                + cue
                    .exit
                    .as_ref()
                    .map_or(0, |animation| animation.duration_ms);
            if duration_ms > 0 && animation_ms + 250 >= duration_ms {
                warnings.push(format!("{}: animation_dominates_cue", cue.id));
            }
        }
        for pair in track.cues.windows(2) {
            if pair[0].text.trim() == pair[1].text.trim() {
                warnings.push(format!("{}: repeated_adjacent_text", pair[1].id));
            }
        }
    }
    warnings
}

pub(crate) fn validate_text_tracks(
    tracks: &mut [TextTrack],
    timeline_duration_ms: i64,
) -> Result<(), String> {
    let mut track_ids = std::collections::HashSet::new();
    let mut cue_ids = std::collections::HashSet::new();
    for track in &mut *tracks {
        if !track_ids.insert(track.id.clone()) || track.id.trim().is_empty() {
            return Err("Text tracks need unique IDs.".to_owned());
        }
        if !matches!(
            track.role.as_str(),
            "subtitle" | "headline" | "callout" | "cta" | "label"
        ) {
            return Err("Text track role is unsupported.".to_owned());
        }
        if !(0..=20).contains(&track.layer) {
            return Err("Text track layer is outside supported bounds.".to_owned());
        }
        for cue in &mut track.cues {
            apply_text_template(cue, &track.role)?;
            if !cue_ids.insert(cue.id.clone()) || cue.id.trim().is_empty() {
                return Err("Text cues need unique IDs.".to_owned());
            }
            if cue.text.trim().is_empty() || cue.text.chars().count() > 280 {
                return Err("Text cue content is empty or too long.".to_owned());
            }
            if cue.start_ms < 0 || cue.end_ms <= cue.start_ms || cue.end_ms > timeline_duration_ms {
                return Err("Text cue timing is outside the timeline.".to_owned());
            }
            let is_color = |value: &str| {
                value.len() == 7
                    && value.starts_with('#')
                    && value[1..]
                        .chars()
                        .all(|character| character.is_ascii_hexdigit())
            };
            if cue.style.font_key.trim().is_empty()
                || !(0.01..=0.30).contains(&cue.style.font_size)
                || !(0.0..=10.0).contains(&cue.style.stroke_width)
                || !is_color(&cue.style.color)
                || cue
                    .style
                    .stroke_color
                    .as_deref()
                    .is_some_and(|color| !is_color(color))
                || cue
                    .style
                    .background_color
                    .as_deref()
                    .is_some_and(|color| !is_color(color))
                || !matches!(cue.style.alignment.as_str(), "left" | "center" | "right")
                || !(-100..=100).contains(&cue.style.letter_spacing)
                || !(-100..=100).contains(&cue.style.line_spacing)
                || !(0.20..=1.0).contains(&cue.layout.max_width)
                || !(0.0..=1.0).contains(&cue.layout.x)
                || !(0.0..=1.0).contains(&cue.layout.y)
                || !matches!(cue.layout.anchor.as_str(), "top" | "center" | "bottom")
                || !matches!(cue.layout.safe_area.as_str(), "title_safe" | "action_safe")
            {
                return Err("Text cue style or layout is outside supported bounds.".to_owned());
            }
            for animation in [&cue.entrance, &cue.exit, &cue.loop_animation]
                .into_iter()
                .flatten()
            {
                if !matches!(
                    animation.template_id.as_str(),
                    "fade" | "slide_up" | "slide_down" | "pop" | "wipe"
                ) || animation.duration_ms < 0
                    || animation.duration_ms > cue.end_ms - cue.start_ms
                    || !(0.0..=1.0).contains(&animation.intensity)
                {
                    return Err(
                        "Text animation is unsupported or outside supported bounds.".to_owned()
                    );
                }
            }
            // Jianying delivery support is assigned by the backend from the
            // small, desktop-verified matrix; the model may never self-certify.
            let verified = cue.style.font_key == "jianying_default"
                && cue.style.stroke_width == 0.0
                && !cue.style.shadow
                && cue.style.background_color.is_none()
                && cue.loop_animation.is_none()
                && cue.entrance.as_ref().map_or(true, |animation| {
                    matches!(
                        animation.template_id.as_str(),
                        "fade" | "slide_up" | "slide_down" | "pop"
                    )
                })
                && cue
                    .exit
                    .as_ref()
                    .map_or(true, |animation| animation.template_id == "fade");
            cue.jianying_compatibility = if verified {
                "verified".to_owned()
            } else {
                "local_preview_only".to_owned()
            };
        }
        let mut cue_ranges = track
            .cues
            .iter()
            .map(|cue| (cue.start_ms, cue.end_ms))
            .collect::<Vec<_>>();
        cue_ranges.sort_unstable();
        if cue_ranges
            .windows(2)
            .any(|ranges| ranges[1].0 < ranges[0].1)
        {
            return Err("Text cues on the same track cannot overlap.".to_owned());
        }
    }
    let mut headline_ranges = tracks
        .iter()
        .filter(|track| track.role == "headline")
        .flat_map(|track| track.cues.iter().map(|cue| (cue.start_ms, cue.end_ms)))
        .collect::<Vec<_>>();
    headline_ranges.sort_unstable();
    if headline_ranges
        .windows(2)
        .any(|ranges| ranges[1].0 < ranges[0].1)
    {
        return Err("Headline cues cannot overlap across text tracks.".to_owned());
    }
    Ok(())
}

pub(crate) fn replace_text_tracks(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    timeline: &TimelineVersion,
    mut text_tracks: Vec<TextTrack>,
) -> Result<TimelineVersion, String> {
    if timeline.project_id != project_id {
        return Err("Timeline does not belong to this project.".to_owned());
    }
    let duration_ms = timeline
        .clips
        .iter()
        .map(|clip| clip.timeline_end_ms)
        .max()
        .unwrap_or(0);
    validate_text_tracks(&mut text_tracks, duration_ms)?;
    let mut updated = timeline.clone();
    updated.text_tracks = text_tracks;
    let version = insert_timeline_version_with_log(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        timeline,
        "replace_text_tracks",
        updated.clips,
        updated.text_tracks,
        updated.music_tracks,
        updated.voiceover_tracks,
    )?;
    Ok(version)
}

fn validate_music_tracks(
    connection: &Connection,
    project_id: &str,
    tracks: &[MusicTrack],
    timeline_duration_ms: i64,
) -> Result<(), String> {
    let mut track_ids = std::collections::HashSet::new();
    let mut cue_ids = std::collections::HashSet::new();
    for track in tracks {
        if track.id.trim().is_empty() || !track_ids.insert(&track.id) {
            return Err("Music track IDs must be unique and non-empty.".to_owned());
        }
        for cue in &track.cues {
            if cue.id.trim().is_empty() || !cue_ids.insert(&cue.id) {
                return Err("Music cue IDs must be unique and non-empty.".to_owned());
            }
            if cue.timeline_start_ms < 0
                || cue.timeline_end_ms <= cue.timeline_start_ms
                || cue.timeline_end_ms > timeline_duration_ms
            {
                return Err("Music cue must stay inside the timeline.".to_owned());
            }
            if cue.source_start_ms < 0 || cue.source_end_ms <= cue.source_start_ms {
                return Err("Music cue source range is invalid.".to_owned());
            }
            if !(0.0..=2.0).contains(&cue.volume)
                || cue.fade_in_ms < 0
                || cue.fade_out_ms < 0
                || cue.fade_in_ms + cue.fade_out_ms > cue.timeline_end_ms - cue.timeline_start_ms
            {
                return Err("Music cue volume or fades are outside the supported range.".to_owned());
            }
            let (kind, metadata_json): (String, String) = connection.query_row(
                "SELECT kind, metadata_json FROM assets WHERE id = ?1 AND project_id = ?2 AND analysis_status = 'ready'",
                params![cue.asset_id, project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).map_err(|_| "Music asset is unavailable or has not finished analysis.".to_owned())?;
            let metadata: TechnicalMetadata =
                serde_json::from_str(&metadata_json).unwrap_or_default();
            if kind != "audio"
                || metadata
                    .duration_ms
                    .is_some_and(|duration| cue.source_end_ms > duration)
            {
                return Err(
                    "Music cue must reference a ready audio asset within its duration.".to_owned(),
                );
            }
            if !cue.loop_enabled
                && cue.source_end_ms - cue.source_start_ms
                    != cue.timeline_end_ms - cue.timeline_start_ms
            {
                return Err(
                    "Non-looping music must have equal source and timeline durations.".to_owned(),
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn replace_music_tracks(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    timeline: &TimelineVersion,
    mut music_tracks: Vec<MusicTrack>,
) -> Result<TimelineVersion, String> {
    if timeline.project_id != project_id {
        return Err("Timeline does not belong to this project.".to_owned());
    }
    for track in &mut music_tracks {
        for cue in &mut track.cues {
            cue.jianying_compatibility = "not_deliverable".to_owned();
        }
    }
    let duration_ms = timeline
        .clips
        .iter()
        .map(|clip| clip.timeline_end_ms)
        .max()
        .unwrap_or(0);
    validate_music_tracks(connection, project_id, &music_tracks, duration_ms)?;
    insert_timeline_version_with_log(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        timeline,
        "replace_music_tracks",
        timeline.clips.clone(),
        timeline.text_tracks.clone(),
        music_tracks,
        timeline.voiceover_tracks.clone(),
    )
}

/// 替换一个或多个镜头并保持各自槽位时长，在一次事务中写入新版本和审计。
pub(crate) fn replace_clips(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    timeline: &TimelineVersion,
    replacements: &[ClipReplacement],
) -> Result<TimelineVersion, String> {
    if timeline.project_id != project_id {
        return Err("Timeline does not belong to this project.".to_owned());
    }
    if replacements.is_empty() {
        return Err("No replacement shots were provided.".to_owned());
    }
    let indexes = replacements
        .iter()
        .map(|replacement| replacement.shot_index)
        .collect::<Vec<_>>();
    if let Some(shot_index) = duplicate_shot_index(&indexes) {
        return Err(format!(
            "Replacement lists shot {shot_index} more than once."
        ));
    }
    for replacement in replacements {
        let original = timeline
            .clips
            .iter()
            .find(|clip| clip.shot_index == replacement.shot_index)
            .ok_or_else(|| {
                format!(
                    "Requested timeline shot {} does not exist.",
                    replacement.shot_index
                )
            })?;
        let clip_duration = original.timeline_end_ms - original.timeline_start_ms;
        let (kind, metadata) =
            asset_kind_and_metadata(connection, project_id, &replacement.asset_id)?;
        if kind == "video" {
            let duration = metadata
                .duration_ms
                .ok_or_else(|| "Replacement video has no verified duration.".to_owned())?;
            if replacement.source_start_ms < 0
                || replacement.source_end_ms <= replacement.source_start_ms
                || replacement.source_end_ms > duration
                || replacement.source_end_ms - replacement.source_start_ms != clip_duration
            {
                return Err(
                    "Replacement video range must be verified and match the existing shot duration."
                        .to_owned(),
                );
            }
        } else if kind == "image" {
            if replacement.source_start_ms != 0 || replacement.source_end_ms != 0 {
                return Err("Replacement images must use a zero source range.".to_owned());
            }
        } else {
            return Err("Replacement asset must be a video or image.".to_owned());
        }
    }
    let clips = timeline
        .clips
        .iter()
        .cloned()
        .map(|mut clip| {
            if let Some(replacement) = replacements
                .iter()
                .find(|replacement| replacement.shot_index == clip.shot_index)
            {
                clip.asset_id = replacement.asset_id.clone();
                clip.source_start_ms = replacement.source_start_ms;
                clip.source_end_ms = replacement.source_end_ms;
            }
            clip
        })
        .collect::<Vec<_>>();
    insert_timeline_version_with_log(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        timeline,
        "replace_clips",
        clips,
        timeline.text_tracks.clone(),
        timeline.music_tracks.clone(),
        timeline.voiceover_tracks.clone(),
    )
}

/// 在已验证源范围内重定时镜头并生成一个新版本；后续片段平移以保持连续。
pub(crate) fn change_clip_duration(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    timeline: &TimelineVersion,
    adjustments: &[ClipAdjustment],
) -> Result<TimelineVersion, String> {
    if timeline.project_id != project_id {
        return Err("Timeline does not belong to this project.".to_owned());
    }
    if adjustments.is_empty() {
        return Err("No duration adjustments were provided.".to_owned());
    }
    let indexes = adjustments
        .iter()
        .map(|adjustment| adjustment.shot_index)
        .collect::<Vec<_>>();
    if let Some(shot_index) = duplicate_shot_index(&indexes) {
        return Err(format!(
            "Duration adjustment lists shot {shot_index} more than once."
        ));
    }
    let mut clips = timeline.clips.clone();
    for adjustment in adjustments {
        let original = clips
            .iter()
            .find(|clip| clip.shot_index == adjustment.shot_index)
            .ok_or_else(|| {
                format!(
                    "Requested timeline shot {} does not exist.",
                    adjustment.shot_index
                )
            })?
            .clone();
        let source_start_ms = adjustment
            .new_source_start_ms
            .unwrap_or(original.source_start_ms);
        let duration_ms = adjustment
            .new_duration_ms
            .unwrap_or(original.timeline_end_ms - original.timeline_start_ms);
        if source_start_ms < 0 {
            return Err("New source start time must not be negative.".to_owned());
        }
        if duration_ms <= 0 {
            return Err("New clip duration must be positive.".to_owned());
        }
        let (kind, metadata) = asset_kind_and_metadata(connection, project_id, &original.asset_id)?;
        if kind == "video" {
            let duration = metadata
                .duration_ms
                .ok_or_else(|| "Retimed video has no verified duration.".to_owned())?;
            if source_start_ms < original.source_start_ms
                || source_start_ms + duration_ms > original.source_end_ms
                || source_start_ms + duration_ms > duration
            {
                return Err("Retimed clip must stay within its verified source range.".to_owned());
            }
        } else if kind == "image" {
            if source_start_ms != 0 {
                return Err("Image clips must use a zero source range.".to_owned());
            }
        } else {
            return Err("Retimed asset must be a video or image.".to_owned());
        }
        let mut updated = original.clone();
        updated.source_start_ms = source_start_ms;
        updated.source_end_ms = if kind == "video" {
            source_start_ms + duration_ms
        } else {
            0
        };
        updated.timeline_end_ms = updated.timeline_start_ms + duration_ms;
        if let Some(clip) = clips
            .iter_mut()
            .find(|clip| clip.shot_index == adjustment.shot_index)
        {
            *clip = updated;
        }
    }
    recompute_timeline_positions(&mut clips);
    insert_timeline_version_with_log(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        timeline,
        "change_clip_duration",
        clips,
        timeline.text_tracks.clone(),
        timeline.music_tracks.clone(),
        timeline.voiceover_tracks.clone(),
    )
}

/// 仅接受全部镜头索引的完整排列，并在一次事务中写入新版本和审计。
pub(crate) fn reorder_clips(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    timeline: &TimelineVersion,
    order: &[i64],
) -> Result<TimelineVersion, String> {
    if timeline.project_id != project_id {
        return Err("Timeline does not belong to this project.".to_owned());
    }
    let existing = timeline
        .clips
        .iter()
        .map(|clip| clip.shot_index)
        .collect::<Vec<_>>();
    if order.len() != existing.len() {
        return Err("Reorder must include every shot exactly once.".to_owned());
    }
    if let Some(shot_index) = duplicate_shot_index(order) {
        return Err(format!("Reorder lists shot {shot_index} more than once."));
    }
    if order.iter().any(|shot| !existing.contains(shot)) {
        return Err("Reorder references a shot that does not exist.".to_owned());
    }
    let mut clips = order
        .iter()
        .map(|shot| {
            timeline
                .clips
                .iter()
                .find(|clip| clip.shot_index == *shot)
                .cloned()
                .expect("reorder references only existing shots")
        })
        .collect::<Vec<_>>();
    recompute_timeline_positions(&mut clips);
    insert_timeline_version_with_log(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        timeline,
        "reorder_clips",
        clips,
        timeline.text_tracks.clone(),
        timeline.music_tracks.clone(),
        timeline.voiceover_tracks.clone(),
    )
}

pub(crate) fn load_timeline_version(
    connection: &Connection,
    timeline_version_id: &str,
) -> Result<TimelineVersion, String> {
    connection.query_row(
        "SELECT id, project_id, storyboard_version_id, version_number, content_json, created_at FROM timeline_versions WHERE id = ?1",
        params![timeline_version_id],
        |row| {
            let content: TimelineContent = serde_json::from_str(&row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(TimelineVersion {
                id: row.get(0)?, project_id: row.get(1)?, storyboard_version_id: row.get(2)?, version_number: row.get(3)?, clips: content.clips, text_tracks: content.text_tracks, music_tracks: content.music_tracks, voiceover_tracks: content.voiceover_tracks, quality_report: content.quality_report, created_at: row.get(5)?,
            })
        },
    ).map_err(|_| "Timeline version could not be read.".to_owned())
}

pub(crate) fn timeline_candidates_for_storyboard(
    connection: &Connection,
    project_id: &str,
    storyboard_version_id: &str,
) -> Result<Vec<TimelineVersion>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, project_id, storyboard_version_id, version_number, content_json, created_at FROM timeline_versions WHERE project_id = ?1 AND storyboard_version_id = ?2 ORDER BY version_number DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id, storyboard_version_id], |row| {
            let content: TimelineContent = serde_json::from_str(&row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(TimelineVersion {
                id: row.get(0)?,
                project_id: row.get(1)?,
                storyboard_version_id: row.get(2)?,
                version_number: row.get(3)?,
                clips: content.clips,
                text_tracks: content.text_tracks,
                music_tracks: content.music_tracks,
                voiceover_tracks: content.voiceover_tracks,
                quality_report: content.quality_report,
                created_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn timeline_candidates_for_editing_task(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
) -> Result<Vec<TimelineVersion>, String> {
    let mut statement = connection
        .prepare(
            "SELECT timeline.id, timeline.project_id, timeline.storyboard_version_id, timeline.version_number, timeline.content_json, timeline.created_at FROM timeline_versions timeline JOIN storyboard_versions storyboard ON storyboard.id = timeline.storyboard_version_id WHERE timeline.project_id = ?1 AND storyboard.editing_task_id = ?2 ORDER BY timeline.created_at DESC, timeline.version_number DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id, editing_task_id], |row| {
            let content: TimelineContent = serde_json::from_str(&row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(TimelineVersion {
                id: row.get(0)?,
                project_id: row.get(1)?,
                storyboard_version_id: row.get(2)?,
                version_number: row.get(3)?,
                clips: content.clips,
                text_tracks: content.text_tracks,
                music_tracks: content.music_tracks,
                voiceover_tracks: content.voiceover_tracks,
                quality_report: content.quality_report,
                created_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn select_timeline_candidate(
    timelines: &[TimelineVersion],
    decision_timeline_id: Option<&str>,
    requested_timeline_id: Option<&str>,
) -> Option<TimelineVersion> {
    if let Some(timeline_id) = decision_timeline_id {
        return timelines
            .iter()
            .find(|timeline| timeline.id == timeline_id)
            .cloned();
    }
    if let Some(timeline_id) = requested_timeline_id {
        return timelines
            .iter()
            .find(|timeline| timeline.id == timeline_id)
            .cloned();
    }
    timelines.first().cloned()
}

#[tauri::command]
pub fn get_latest_timeline(
    app: AppHandle,
    project_id: String,
    storyboard_version_id: String,
) -> Result<Option<LatestTimeline>, String> {
    let connection = open_connection(&app)?;
    let latest = connection.query_row(
        "SELECT id, project_id, storyboard_version_id, version_number, content_json, created_at, status FROM timeline_versions WHERE project_id = ?1 AND storyboard_version_id = ?2 ORDER BY version_number DESC LIMIT 1",
        params![project_id, storyboard_version_id],
        |row| {
            let content: TimelineContent = serde_json::from_str(&row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok((
                TimelineVersion {
                    id: row.get(0)?, project_id: row.get(1)?, storyboard_version_id: row.get(2)?, version_number: row.get(3)?, clips: content.clips, text_tracks: content.text_tracks, music_tracks: content.music_tracks, voiceover_tracks: content.voiceover_tracks, quality_report: content.quality_report, created_at: row.get(5)?,
                },
                row.get::<_, String>(6)?,
            ))
        },
    ).optional().map_err(|_| "Latest timeline could not be read.".to_owned())?;
    let Some((timeline, status)) = latest else {
        return Ok(None);
    };
    let preview_path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("previews")
        .join(&timeline.id)
        .join("preview.mp4");
    let preview = (status == "preview_ready" && preview_path.is_file()).then(|| PreviewResult {
        timeline_version_id: timeline.id.clone(),
        preview_path: preview_path.to_string_lossy().into_owned(),
        quality_report: timeline
            .quality_report
            .clone()
            .unwrap_or(PreviewQualityReport { checks: Vec::new() }),
    });
    Ok(Some(LatestTimeline { timeline, preview }))
}

#[tauri::command]
pub fn list_timeline_versions(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    storyboard_version_id: String,
) -> Result<Vec<TimelineVersion>, String> {
    let connection = open_connection(&app)?;
    let storyboard_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM storyboard_versions WHERE id = ?1 AND project_id = ?2 AND editing_task_id = ?3)",
        params![storyboard_version_id, project_id, editing_task_id],
        |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    if !storyboard_exists {
        return Err("Storyboard does not belong to the current editing task.".to_owned());
    }
    timeline_candidates_for_storyboard(&connection, &project_id, &storyboard_version_id)
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
