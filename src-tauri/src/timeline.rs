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
        quality_report: None,
        created_at: now_millis(),
    };
    connection.execute(
        "INSERT INTO timeline_versions (id, project_id, storyboard_version_id, version_number, status, content_json, created_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
        params![version.id, version.project_id, version.storyboard_version_id, version.version_number, serde_json::to_string(&TimelineContent { clips: version.clips.clone(), text_tracks: version.text_tracks.clone(), music_tracks: version.music_tracks.clone(), quality_report: None }).map_err(|error| error.to_string())?, version.created_at],
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

fn insert_timeline_version_with_log(
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
        quality_report: None,
        created_at: now_millis(),
    };
    let content_json = serde_json::to_string(&TimelineContent {
        clips: version.clips.clone(),
        text_tracks: version.text_tracks.clone(),
        music_tracks: version.music_tracks.clone(),
        quality_report: None,
    })
    .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO timeline_versions (id, project_id, storyboard_version_id, version_number, status, content_json, created_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
            params![version.id, version.project_id, version.storyboard_version_id, version.version_number, content_json, version.created_at],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO operation_logs (id, project_id, editing_task_id, conversation_id, agent_task_id, actor, operation_type, entity_type, entity_id, before_json, after_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'agent', ?6, 'timeline_version', ?7, ?8, ?9, ?10)",
            params![Uuid::new_v4().to_string(), project_id, editing_task_id, conversation_id, agent_task_id, operation_type, version.id, serde_json::to_string(&TimelineContent { clips: timeline.clips.clone(), text_tracks: timeline.text_tracks.clone(), music_tracks: timeline.music_tracks.clone(), quality_report: timeline.quality_report.clone() }).map_err(|error| error.to_string())?, serde_json::to_string(&TimelineContent { clips: version.clips.clone(), text_tracks: version.text_tracks.clone(), music_tracks: version.music_tracks.clone(), quality_report: version.quality_report.clone() }).map_err(|error| error.to_string())?, now_millis()],
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

fn validate_text_tracks(tracks: &mut [TextTrack], timeline_duration_ms: i64) -> Result<(), String> {
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
                id: row.get(0)?, project_id: row.get(1)?, storyboard_version_id: row.get(2)?, version_number: row.get(3)?, clips: content.clips, text_tracks: content.text_tracks, music_tracks: content.music_tracks, quality_report: content.quality_report, created_at: row.get(5)?,
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
    (timelines.len() == 1).then(|| timelines[0].clone())
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
                    id: row.get(0)?, project_id: row.get(1)?, storyboard_version_id: row.get(2)?, version_number: row.get(3)?, clips: content.clips, text_tracks: content.text_tracks, music_tracks: content.music_tracks, quality_report: content.quality_report, created_at: row.get(5)?,
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
mod tests {
    use super::*;
    use crate::models::{TextAnimation, TextCue, TextLayout, TextStyle};

    fn text_cue(
        font_key: &str,
        entrance: Option<TextAnimation>,
        exit: Option<TextAnimation>,
    ) -> TextCue {
        TextCue {
            id: "cue-1".to_owned(),
            template_id: None,
            start_ms: 0,
            end_ms: 2_000,
            text: "Verified text".to_owned(),
            style: TextStyle {
                font_key: font_key.to_owned(),
                font_size: 0.06,
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
            layout: TextLayout {
                anchor: "bottom".to_owned(),
                x: 0.5,
                y: 0.82,
                max_width: 0.8,
                safe_area: "title_safe".to_owned(),
            },
            entrance,
            exit,
            loop_animation: None,
            jianying_compatibility: "verified".to_owned(),
        }
    }

    #[test]
    fn text_compatibility_is_assigned_by_the_verified_delivery_matrix() {
        let fade = TextAnimation {
            template_id: "fade".to_owned(),
            duration_ms: 250,
            intensity: 0.6,
        };
        let slide_down = TextAnimation {
            template_id: "slide_down".to_owned(),
            duration_ms: 250,
            intensity: 0.6,
        };
        let mut tracks = vec![TextTrack {
            id: "text-1".to_owned(),
            role: "subtitle".to_owned(),
            layer: 1,
            enabled: true,
            cues: vec![
                text_cue("jianying_default", Some(fade), None),
                text_cue("sans_bold", None, None),
                text_cue("jianying_default", Some(slide_down), None),
            ],
        }];
        tracks[0].cues[1].id = "cue-2".to_owned();
        tracks[0].cues[2].id = "cue-3".to_owned();
        tracks[0].cues[1].start_ms = 2_000;
        tracks[0].cues[1].end_ms = 4_000;
        tracks[0].cues[2].start_ms = 4_000;
        tracks[0].cues[2].end_ms = 6_000;

        validate_text_tracks(&mut tracks, 6_000).expect("validate text tracks");

        assert_eq!(tracks[0].cues[0].jianying_compatibility, "verified");
        assert_eq!(
            tracks[0].cues[1].jianying_compatibility,
            "local_preview_only"
        );
        assert_eq!(tracks[0].cues[2].jianying_compatibility, "verified");
    }

    #[test]
    fn validated_pop_entrance_and_fade_exit_are_jianying_deliverable() {
        let mut cue = text_cue(
            "jianying_default",
            Some(TextAnimation {
                template_id: "pop".to_owned(),
                duration_ms: 250,
                intensity: 0.7,
            }),
            Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 250,
                intensity: 0.7,
            }),
        );
        cue.id = "cue-pop-out".to_owned();
        let mut tracks = vec![TextTrack {
            id: "text-1".to_owned(),
            role: "headline".to_owned(),
            layer: 1,
            enabled: true,
            cues: vec![cue],
        }];

        validate_text_tracks(&mut tracks, 2_000).expect("validate confirmed dynamics");

        assert_eq!(tracks[0].cues[0].jianying_compatibility, "verified");
    }

    #[test]
    fn unicode_text_uses_the_verified_jianying_delivery_matrix() {
        let mut cue = text_cue("jianying_default", None, None);
        cue.text = "\u{5b57}\u{5e55}".to_owned();
        let mut tracks = vec![TextTrack {
            id: "text-1".to_owned(),
            role: "subtitle".to_owned(),
            layer: 1,
            enabled: true,
            cues: vec![cue],
        }];

        validate_text_tracks(&mut tracks, 2_000).expect("validate Unicode cue");

        assert_eq!(tracks[0].cues[0].jianying_compatibility, "verified");
    }

    #[test]
    fn text_recipe_overrides_conflicting_model_style_with_a_verified_delivery_recipe() {
        let mut cue = text_cue(
            "sans_clean",
            Some(TextAnimation {
                template_id: "pop".to_owned(),
                duration_ms: 400,
                intensity: 1.0,
            }),
            Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 400,
                intensity: 1.0,
            }),
        );
        cue.template_id = Some("headline_rise".to_owned());
        let mut tracks = vec![TextTrack {
            id: "text-1".to_owned(),
            role: "headline".to_owned(),
            layer: 1,
            enabled: true,
            cues: vec![cue],
        }];

        validate_text_tracks(&mut tracks, 2_000).expect("apply text recipe");

        let resolved = &tracks[0].cues[0];
        assert_eq!(resolved.style.font_key, "jianying_default");
        assert_eq!(resolved.layout.anchor, "center");
        assert_eq!(
            resolved
                .entrance
                .as_ref()
                .map(|animation| animation.template_id.as_str()),
            Some("slide_up")
        );
        assert_eq!(
            resolved
                .exit
                .as_ref()
                .map(|animation| animation.template_id.as_str()),
            Some("fade")
        );
        assert_eq!(resolved.jianying_compatibility, "verified");
    }

    #[test]
    fn legacy_text_cue_without_template_id_still_deserializes() {
        let cue: TextCue = serde_json::from_str(
            r##"{
                "id":"cue-1","startMs":0,"endMs":1000,"text":"Legacy text",
                "style":{"fontKey":"jianying_default","fontSize":0.06,"bold":true,"color":"#FFFFFF","strokeColor":null,"strokeWidth":0.0,"shadow":false,"backgroundColor":null,"alignment":"center","letterSpacing":0,"lineSpacing":0},
                "layout":{"anchor":"bottom","x":0.5,"y":0.82,"maxWidth":0.8,"safeArea":"title_safe"},
                "entrance":null,"exit":null,"loopAnimation":null,"jianyingCompatibility":"verified"
            }"##,
        )
        .expect("load legacy text cue");

        assert!(cue.template_id.is_none());
    }

    #[test]
    fn legacy_timeline_content_defaults_music_tracks_and_preserves_music_cues() {
        let legacy: TimelineContent = serde_json::from_str(r#"{"clips":[],"textTracks":[]}"#)
            .expect("parse legacy timeline content");
        assert!(legacy.music_tracks.is_empty());
        let content: TimelineContent = serde_json::from_str(r#"{"clips":[],"textTracks":[],"musicTracks":[{"id":"music-1","enabled":true,"cues":[{"id":"cue-1","assetId":"audio-1","sourceStartMs":0,"sourceEndMs":1000,"timelineStartMs":0,"timelineEndMs":3000,"loopEnabled":true,"volume":0.4,"fadeInMs":120,"fadeOutMs":120}]}]}"#)
            .expect("parse music timeline content");
        assert!(content.music_tracks[0].cues[0].loop_enabled);
        assert_eq!(
            content.music_tracks[0].cues[0].jianying_compatibility,
            "not_deliverable"
        );
    }

    #[test]
    fn minimal_agent_text_cue_uses_safe_defaults_before_validation() {
        let mut cue: TextCue = serde_json::from_str(
            r#"{"id":"cue-1","templateId":"subtitle_safe","startMs":0,"endMs":1000,"text":"Minimal cue"}"#,
        )
        .expect("parse minimal agent cue");
        let mut tracks = vec![TextTrack {
            id: "text-1".to_owned(),
            role: "subtitle".to_owned(),
            layer: 1,
            enabled: true,
            cues: vec![cue.clone()],
        }];

        validate_text_tracks(&mut tracks, 1_000).expect("resolve minimal text cue");
        cue = tracks[0].cues[0].clone();
        assert_eq!(cue.style.font_key, "jianying_default");
        assert_eq!(cue.layout.anchor, "bottom");
        assert_eq!(cue.jianying_compatibility, "verified");
    }

    #[test]
    fn every_advertised_text_recipe_is_accepted_by_the_text_track_validator() {
        for recipe in text_recipe_capabilities() {
            let template_id = recipe["templateId"].as_str().expect("recipe ID");
            let role = recipe["role"].as_str().expect("recipe role");
            let mut cue = text_cue("sans_clean", None, None);
            cue.template_id = Some(template_id.to_owned());
            let mut tracks = vec![TextTrack {
                id: format!("track-{template_id}"),
                role: role.to_owned(),
                layer: 1,
                enabled: true,
                cues: vec![cue],
            }];

            validate_text_tracks(&mut tracks, 2_000).unwrap_or_else(|error| {
                panic!("recipe {template_id} was advertised but rejected: {error}")
            });
        }
    }

    #[test]
    fn text_recipe_capabilities_expose_a_semantic_selection_hint() {
        for recipe in text_recipe_capabilities() {
            assert!(recipe["selectionHint"]
                .as_str()
                .is_some_and(|hint| !hint.trim().is_empty()));
        }
        let headline_pop = text_recipe_capabilities()
            .into_iter()
            .find(|recipe| recipe["templateId"] == "headline_pop")
            .expect("headline pop recipe");
        assert!(headline_pop["selectionHint"]
            .as_str()
            .expect("headline pop hint")
            .contains("surprise"));
    }

    #[test]
    fn text_track_quality_warns_about_readability_and_animation_without_rejecting_the_cue() {
        let mut cue = text_cue(
            "jianying_default",
            Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 600,
                intensity: 0.5,
            }),
            Some(TextAnimation {
                template_id: "fade".to_owned(),
                duration_ms: 500,
                intensity: 0.5,
            }),
        );
        cue.end_ms = 1_000;
        cue.text = "这是一段在一秒钟内无法舒适读完的过长文本内容".to_owned();
        let tracks = vec![TextTrack {
            id: "text-1".to_owned(),
            role: "subtitle".to_owned(),
            layer: 1,
            enabled: true,
            cues: vec![cue],
        }];
        let warnings = text_track_quality_warnings(&tracks);
        assert!(warnings
            .iter()
            .any(|warning| warning.ends_with("readability_density")));
        assert!(warnings
            .iter()
            .any(|warning| warning.ends_with("animation_dominates_cue")));
    }

    #[test]
    fn headline_cues_cannot_overlap_across_tracks() {
        let first = text_cue("jianying_default", None, None);
        let mut second = text_cue("jianying_default", None, None);
        second.id = "cue-2".to_owned();
        second.start_ms = 500;
        let mut tracks = vec![
            TextTrack {
                id: "headline-1".to_owned(),
                role: "headline".to_owned(),
                layer: 1,
                enabled: true,
                cues: vec![first],
            },
            TextTrack {
                id: "headline-2".to_owned(),
                role: "headline".to_owned(),
                layer: 2,
                enabled: true,
                cues: vec![second],
            },
        ];
        assert!(validate_text_tracks(&mut tracks, 2_000).is_err());
    }

    #[test]
    fn text_cues_on_the_same_track_cannot_overlap() {
        let mut second_cue = text_cue("jianying_default", None, None);
        second_cue.id = "cue-2".to_owned();
        second_cue.start_ms = 1_500;
        second_cue.end_ms = 2_500;
        let mut tracks = vec![TextTrack {
            id: "text-1".to_owned(),
            role: "subtitle".to_owned(),
            layer: 1,
            enabled: true,
            cues: vec![text_cue("jianying_default", None, None), second_cue],
        }];

        assert!(validate_text_tracks(&mut tracks, 3_000).is_err());
    }

    #[test]
    fn editing_task_candidates_include_only_its_timelines() {
        let connection = Connection::open_in_memory().expect("open test database");
        connection
            .execute_batch(
                "
                CREATE TABLE storyboard_versions (id TEXT, project_id TEXT, editing_task_id TEXT);
                CREATE TABLE timeline_versions (id TEXT, project_id TEXT, storyboard_version_id TEXT, version_number INTEGER, content_json TEXT, created_at INTEGER);
                INSERT INTO storyboard_versions VALUES ('storyboard-a', 'project-1', 'edit-a');
                INSERT INTO storyboard_versions VALUES ('storyboard-b', 'project-1', 'edit-b');
                INSERT INTO storyboard_versions VALUES ('storyboard-other', 'project-2', 'edit-a');
                INSERT INTO timeline_versions VALUES ('timeline-a', 'project-1', 'storyboard-a', 1, '{\"clips\":[]}', 1);
                INSERT INTO timeline_versions VALUES ('timeline-b', 'project-1', 'storyboard-b', 1, '{\"clips\":[]}', 2);
                INSERT INTO timeline_versions VALUES ('timeline-other', 'project-2', 'storyboard-other', 1, '{\"clips\":[]}', 3);
                ",
            )
            .expect("create scoped timeline test data");

        let candidates = timeline_candidates_for_editing_task(&connection, "project-1", "edit-a")
            .expect("load scoped candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "timeline-a");
    }

    #[test]
    fn requested_timeline_is_used_when_multiple_candidates_exist() {
        let timeline = |id: &str, version_number| TimelineVersion {
            id: id.to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number,
            clips: Vec::new(),
            text_tracks: Vec::new(),
            music_tracks: Vec::new(),
            quality_report: None,
            created_at: version_number,
        };
        let timelines = vec![timeline("timeline-2", 2), timeline("timeline-1", 1)];

        let selected =
            select_timeline_candidate(&timelines, None, Some("timeline-1")).expect("selection");
        assert_eq!(selected.id, "timeline-1");
    }

    #[test]
    fn explicit_unknown_timeline_never_falls_back_to_the_only_candidate() {
        let timeline = TimelineVersion {
            id: "timeline-1".to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number: 1,
            clips: Vec::new(),
            text_tracks: Vec::new(),
            music_tracks: Vec::new(),
            quality_report: None,
            created_at: 1,
        };

        assert!(select_timeline_candidate(&[timeline.clone()], Some("unknown"), None).is_none());
        assert!(select_timeline_candidate(&[timeline], None, Some("unknown")).is_none());
    }

    #[test]
    fn replacing_clips_creates_a_new_version_without_moving_timeline_bounds() {
        let connection = Connection::open_in_memory().expect("open test database");
        connection
            .execute_batch(
                "
                CREATE TABLE assets (id TEXT, project_id TEXT, kind TEXT, analysis_status TEXT, metadata_json TEXT);
                CREATE TABLE timeline_versions (id TEXT, project_id TEXT, storyboard_version_id TEXT, version_number INTEGER, status TEXT, content_json TEXT, created_at INTEGER);
                CREATE TABLE operation_logs (id TEXT, project_id TEXT, editing_task_id TEXT, conversation_id TEXT, agent_task_id TEXT, actor TEXT, operation_type TEXT, entity_type TEXT, entity_id TEXT, before_json TEXT, after_json TEXT, created_at INTEGER);
                ",
            )
            .expect("create test tables");
        let replacement_metadata = serde_json::to_string(&TechnicalMetadata {
            duration_ms: Some(10_000),
            ..TechnicalMetadata::default()
        })
        .expect("serialize replacement metadata");
        connection
            .execute(
                "INSERT INTO assets VALUES ('replacement-video', 'project-1', 'video', 'ready', ?1)",
                params![replacement_metadata],
            )
            .expect("insert replacement asset");
        connection
            .execute(
                "INSERT INTO timeline_versions VALUES ('existing', 'project-1', 'storyboard-1', 1, 'draft', '{}', 1)",
                [],
            )
            .expect("insert existing version");
        let existing = TimelineVersion {
            id: "existing".to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number: 1,
            clips: vec![TimelineClip {
                shot_index: 1,
                asset_id: "original-video".to_owned(),
                source_start_ms: 0,
                source_end_ms: 2_000,
                timeline_start_ms: 0,
                timeline_end_ms: 2_000,
                on_screen_text: "Quality".to_owned(),
            }],
            text_tracks: Vec::new(),
            music_tracks: Vec::new(),
            quality_report: None,
            created_at: 1,
        };
        let replacement = replace_clips(
            &connection,
            "project-1",
            "editing-task-1",
            "conversation-1",
            "agent-task-1",
            &existing,
            &[ClipReplacement {
                shot_index: 1,
                asset_id: "replacement-video".to_owned(),
                source_start_ms: 3_000,
                source_end_ms: 5_000,
            }],
        )
        .expect("replace scoped clip");
        assert_eq!(replacement.version_number, 2);
        assert_eq!(replacement.clips[0].asset_id, "replacement-video");
        assert_eq!(replacement.clips[0].source_start_ms, 3_000);
        assert_eq!(replacement.clips[0].source_end_ms, 5_000);
        assert_eq!(replacement.clips[0].timeline_start_ms, 0);
        assert_eq!(replacement.clips[0].timeline_end_ms, 2_000);
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM operation_logs", [], |row| row
                    .get::<_, i64>(0))
                .expect("count operation logs"),
            1
        );
    }

    #[test]
    fn changing_clip_duration_shifts_following_shots_and_stays_in_source_range() {
        let connection = Connection::open_in_memory().expect("open test database");
        connection
            .execute_batch(
                "
                CREATE TABLE assets (id TEXT, project_id TEXT, kind TEXT, analysis_status TEXT, metadata_json TEXT);
                CREATE TABLE timeline_versions (id TEXT, project_id TEXT, storyboard_version_id TEXT, version_number INTEGER, status TEXT, content_json TEXT, created_at INTEGER);
                CREATE TABLE operation_logs (id TEXT, project_id TEXT, editing_task_id TEXT, conversation_id TEXT, agent_task_id TEXT, actor TEXT, operation_type TEXT, entity_type TEXT, entity_id TEXT, before_json TEXT, after_json TEXT, created_at INTEGER);
                ",
            )
            .expect("create test tables");
        let metadata = serde_json::to_string(&TechnicalMetadata {
            duration_ms: Some(10_000),
            ..TechnicalMetadata::default()
        })
        .expect("serialize metadata");
        connection
            .execute(
                "INSERT INTO assets VALUES ('video-a', 'project-1', 'video', 'ready', ?1)",
                params![metadata],
            )
            .expect("insert asset");
        connection
            .execute(
                "INSERT INTO timeline_versions VALUES ('existing', 'project-1', 'storyboard-1', 1, 'draft', '{}', 1)",
                [],
            )
            .expect("insert existing version");
        let existing = TimelineVersion {
            id: "existing".to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number: 1,
            clips: vec![
                TimelineClip {
                    shot_index: 1,
                    asset_id: "video-a".to_owned(),
                    source_start_ms: 0,
                    source_end_ms: 3_000,
                    timeline_start_ms: 0,
                    timeline_end_ms: 3_000,
                    on_screen_text: String::new(),
                },
                TimelineClip {
                    shot_index: 2,
                    asset_id: "video-a".to_owned(),
                    source_start_ms: 3_000,
                    source_end_ms: 5_000,
                    timeline_start_ms: 3_000,
                    timeline_end_ms: 5_000,
                    on_screen_text: String::new(),
                },
            ],
            text_tracks: Vec::new(),
            music_tracks: Vec::new(),
            quality_report: None,
            created_at: 1,
        };
        let adjusted = change_clip_duration(
            &connection,
            "project-1",
            "editing-task-1",
            "conversation-1",
            "agent-task-1",
            &existing,
            &[ClipAdjustment {
                shot_index: 1,
                new_duration_ms: Some(1_000),
                new_source_start_ms: Some(2_000),
            }],
        )
        .expect("retime first clip");
        assert_eq!(adjusted.clips[0].timeline_start_ms, 0);
        assert_eq!(adjusted.clips[0].timeline_end_ms, 1_000);
        assert_eq!(adjusted.clips[1].timeline_start_ms, 1_000);
        assert_eq!(adjusted.clips[1].timeline_end_ms, 3_000);
        assert_eq!(
            adjusted.clips[0].source_start_ms, 2_000,
            "new source start is honored"
        );
        assert_eq!(
            adjusted.clips[0].source_end_ms, 3_000,
            "source end follows the retimed video window"
        );

        let shortened = change_clip_duration(
            &connection,
            "project-1",
            "editing-task-1",
            "conversation-1",
            "agent-task-1",
            &existing,
            &[ClipAdjustment {
                shot_index: 1,
                new_duration_ms: Some(1_000),
                new_source_start_ms: None,
            }],
        )
        .expect("shorten first clip without moving its source start");
        assert_eq!(shortened.clips[0].source_start_ms, 0);
        assert_eq!(shortened.clips[0].source_end_ms, 1_000);
        assert_eq!(shortened.clips[0].timeline_end_ms, 1_000);

        let invalid = change_clip_duration(
            &connection,
            "project-1",
            "editing-task-1",
            "conversation-1",
            "agent-task-1",
            &existing,
            &[ClipAdjustment {
                shot_index: 1,
                new_duration_ms: Some(4_000),
                new_source_start_ms: None,
            }],
        );
        assert!(
            invalid.is_err(),
            "retiming beyond the verified source range must fail"
        );

        let before_verified_start = change_clip_duration(
            &connection,
            "project-1",
            "editing-task-1",
            "conversation-1",
            "agent-task-1",
            &existing,
            &[ClipAdjustment {
                shot_index: 2,
                new_duration_ms: Some(1_000),
                new_source_start_ms: Some(0),
            }],
        );
        assert!(
            before_verified_start.is_err(),
            "retiming before the verified source range must fail"
        );
    }

    #[test]
    fn reordering_clips_requires_a_full_permutation() {
        let connection = Connection::open_in_memory().expect("open test database");
        connection
            .execute_batch(
                "
                CREATE TABLE timeline_versions (id TEXT, project_id TEXT, storyboard_version_id TEXT, version_number INTEGER, status TEXT, content_json TEXT, created_at INTEGER);
                CREATE TABLE operation_logs (id TEXT, project_id TEXT, editing_task_id TEXT, conversation_id TEXT, agent_task_id TEXT, actor TEXT, operation_type TEXT, entity_type TEXT, entity_id TEXT, before_json TEXT, after_json TEXT, created_at INTEGER);
                ",
            )
            .expect("create test tables");
        connection
            .execute(
                "INSERT INTO timeline_versions VALUES ('existing', 'project-1', 'storyboard-1', 1, 'draft', '{}', 1)",
                [],
            )
            .expect("insert existing version");
        let existing = TimelineVersion {
            id: "existing".to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number: 1,
            clips: vec![
                TimelineClip {
                    shot_index: 1,
                    asset_id: "a".to_owned(),
                    source_start_ms: 0,
                    source_end_ms: 1_000,
                    timeline_start_ms: 0,
                    timeline_end_ms: 1_000,
                    on_screen_text: String::new(),
                },
                TimelineClip {
                    shot_index: 2,
                    asset_id: "b".to_owned(),
                    source_start_ms: 0,
                    source_end_ms: 1_000,
                    timeline_start_ms: 1_000,
                    timeline_end_ms: 2_000,
                    on_screen_text: String::new(),
                },
                TimelineClip {
                    shot_index: 3,
                    asset_id: "c".to_owned(),
                    source_start_ms: 0,
                    source_end_ms: 1_000,
                    timeline_start_ms: 2_000,
                    timeline_end_ms: 3_000,
                    on_screen_text: String::new(),
                },
            ],
            text_tracks: Vec::new(),
            music_tracks: Vec::new(),
            quality_report: None,
            created_at: 1,
        };
        let reordered = reorder_clips(
            &connection,
            "project-1",
            "editing-task-1",
            "conversation-1",
            "agent-task-1",
            &existing,
            &[3, 1, 2],
        )
        .expect("reorder shots");
        assert_eq!(reordered.clips[0].shot_index, 3);
        assert_eq!(reordered.clips[0].timeline_start_ms, 0);
        assert_eq!(reordered.clips[1].shot_index, 1);
        assert_eq!(reordered.clips[1].timeline_start_ms, 1_000);
        assert_eq!(reordered.clips[2].shot_index, 2);
        assert_eq!(reordered.clips[2].timeline_start_ms, 2_000);

        let incomplete = reorder_clips(
            &connection,
            "project-1",
            "editing-task-1",
            "conversation-1",
            "agent-task-1",
            &existing,
            &[1, 2],
        );
        assert!(incomplete.is_err(), "a partial order must be rejected");
        let unknown = reorder_clips(
            &connection,
            "project-1",
            "editing-task-1",
            "conversation-1",
            "agent-task-1",
            &existing,
            &[4, 1, 2],
        );
        assert!(unknown.is_err(), "unknown shot indexes must be rejected");
    }
}
