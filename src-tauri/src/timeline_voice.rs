//! 把已合成的旁白写入新时间线版本：口播是时钟，画面只可补不可截音频。

use crate::models::{
    PreviewQualityCheck, TextTrack, TimelineClip, TimelineVersion, VoiceoverCue, VoiceoverTrack,
};
use crate::timeline::{insert_timeline_version_with_log, validate_text_tracks};
use rusqlite::Connection;

pub(crate) const VOICEOVER_TAIL_MS: i64 = 500;
pub(crate) const EXCESSIVE_VISUAL_TAIL_MS: i64 = 1_500;

pub(crate) fn fit_visual_to_voiceover(
    mut clips: Vec<TimelineClip>,
    voice_duration_ms: i64,
) -> Result<(Vec<TimelineClip>, Option<PreviewQualityCheck>), String> {
    if clips.is_empty() {
        return Err("Timeline has no clips to fit to the voiceover.".to_owned());
    }
    if voice_duration_ms <= 0 {
        return Err("Voiceover duration is invalid.".to_owned());
    }
    let visual = clips
        .iter()
        .map(|clip| clip.timeline_end_ms)
        .max()
        .unwrap_or(0);
    if visual >= voice_duration_ms {
        let warning = (visual > voice_duration_ms + EXCESSIVE_VISUAL_TAIL_MS).then(|| {
            PreviewQualityCheck {
                category: "excessive_visual_tail".to_owned(),
                severity: "warning".to_owned(),
                message: "Picture is more than 1.5 seconds longer than the voiceover; the extra tail was kept.".to_owned(),
                shot_indices: Vec::new(),
            }
        });
        return Ok((clips, warning));
    }
    let last = clips
        .last()
        .cloned()
        .ok_or_else(|| "Timeline has no clips to fit to the voiceover.".to_owned())?;
    let pad_ms = (voice_duration_ms + VOICEOVER_TAIL_MS) - visual;
    let freeze_start = last
        .source_end_ms
        .saturating_sub(40)
        .max(last.source_start_ms);
    let freeze_end = last.source_end_ms.max(freeze_start);
    if freeze_end <= freeze_start {
        return Err("Last clip has no verified source range for a freeze-frame fill.".to_owned());
    }
    let next_index = clips.iter().map(|clip| clip.shot_index).max().unwrap_or(0) + 1;
    clips.push(TimelineClip {
        shot_index: next_index,
        asset_id: last.asset_id.clone(),
        source_start_ms: freeze_start,
        source_end_ms: freeze_end,
        timeline_start_ms: last.timeline_end_ms,
        timeline_end_ms: last.timeline_end_ms + pad_ms,
        on_screen_text: String::new(),
        clip_kind: "freeze_frame".to_owned(),
        derived_from_shot_index: Some(last.shot_index),
        fit_reason: Some("voiceover_tail_fill".to_owned()),
    });
    Ok((clips, None))
}

pub(crate) fn replace_generated_subtitle_tracks(
    tracks: Vec<TextTrack>,
    generated: TextTrack,
) -> Vec<TextTrack> {
    let mut kept: Vec<TextTrack> = tracks
        .into_iter()
        .filter(|track| {
            if track.role != "subtitle" || track.locked {
                return true;
            }
            !matches!(
                track.origin.as_str(),
                "storyboard_generated" | "voice_alignment"
            )
        })
        .collect();
    kept.push(generated);
    kept
}

pub(crate) fn apply_synthesized_voiceover(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    agent_task_id: &str,
    timeline: &TimelineVersion,
    asset_id: &str,
    generation_id: &str,
    voice_id: &str,
    voice_name: &str,
    audio_duration_ms: i64,
    subtitle_track: Option<TextTrack>,
) -> Result<(TimelineVersion, Vec<PreviewQualityCheck>), String> {
    if timeline.project_id != project_id {
        return Err("Timeline does not belong to this project.".to_owned());
    }
    let (clips, tail_warning) = fit_visual_to_voiceover(timeline.clips.clone(), audio_duration_ms)?;
    let mut warnings = Vec::new();
    if let Some(warning) = tail_warning {
        warnings.push(warning);
    }
    let visual_duration = clips
        .iter()
        .map(|clip| clip.timeline_end_ms)
        .max()
        .unwrap_or(0);
    if visual_duration < audio_duration_ms {
        return Err("Picture duration is shorter than the voiceover.".to_owned());
    }
    let voiceover_tracks = vec![VoiceoverTrack {
        id: format!("voiceover-{generation_id}"),
        enabled: true,
        cues: vec![VoiceoverCue {
            id: format!("voiceover-{generation_id}-cue"),
            asset_id: asset_id.to_owned(),
            generation_id: generation_id.to_owned(),
            source_start_ms: 0,
            source_end_ms: 0 + audio_duration_ms,
            timeline_start_ms: 0,
            timeline_end_ms: 0 + audio_duration_ms,
            volume: 1.0,
            fade_in_ms: 0,
            fade_out_ms: 80,
            provider: "ElevenLabs".to_owned(),
            voice_id: voice_id.to_owned(),
            voice_name: voice_name.to_owned(),
        }],
    }];
    let text_tracks = if let Some(generated) = subtitle_track {
        let mut tracks = replace_generated_subtitle_tracks(timeline.text_tracks.clone(), generated);
        validate_text_tracks(&mut tracks, visual_duration)?;
        tracks
    } else {
        timeline.text_tracks.clone()
    };
    let version = insert_timeline_version_with_log(
        connection,
        project_id,
        editing_task_id,
        conversation_id,
        agent_task_id,
        timeline,
        "synthesize_voiceover",
        clips,
        text_tracks,
        timeline.music_tracks.clone(),
        voiceover_tracks,
    )?;
    Ok((version, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_picture_gets_a_freeze_frame_instead_of_extending_source_range() {
        let clips = vec![TimelineClip {
            shot_index: 1,
            asset_id: "video-1".to_owned(),
            source_start_ms: 0,
            source_end_ms: 2_000,
            timeline_start_ms: 0,
            timeline_end_ms: 2_000,
            on_screen_text: String::new(),
            ..Default::default()
        }];
        let (fitted, warning) = fit_visual_to_voiceover(clips, 3_000).expect("fit");
        assert!(warning.is_none());
        assert_eq!(fitted.len(), 2);
        let freeze = &fitted[1];
        assert_eq!(freeze.clip_kind, "freeze_frame");
        assert_eq!(freeze.fit_reason.as_deref(), Some("voiceover_tail_fill"));
        assert_eq!(freeze.derived_from_shot_index, Some(1));
        assert!(freeze.source_end_ms <= 2_000);
        assert!(freeze.timeline_end_ms >= 3_000);
    }

    #[test]
    fn long_picture_is_kept_with_an_excessive_tail_warning() {
        let clips = vec![TimelineClip {
            shot_index: 1,
            asset_id: "video-1".to_owned(),
            source_start_ms: 0,
            source_end_ms: 8_000,
            timeline_start_ms: 0,
            timeline_end_ms: 8_000,
            on_screen_text: String::new(),
            ..Default::default()
        }];
        let (fitted, warning) = fit_visual_to_voiceover(clips, 3_000).expect("fit");
        assert_eq!(fitted.len(), 1);
        assert_eq!(warning.expect("warning").category, "excessive_visual_tail");
    }

    #[test]
    fn generated_subtitles_replace_storyboard_tracks_but_keep_user_tracks() {
        let storyboard = TextTrack {
            id: "storyboard-subtitles".to_owned(),
            role: "subtitle".to_owned(),
            origin: "storyboard_generated".to_owned(),
            ..Default::default()
        };
        let user = TextTrack {
            id: "user-subs".to_owned(),
            role: "subtitle".to_owned(),
            origin: "user".to_owned(),
            locked: true,
            ..Default::default()
        };
        let generated = TextTrack {
            id: "voice-alignment".to_owned(),
            role: "subtitle".to_owned(),
            origin: "voice_alignment".to_owned(),
            generation_id: Some("gen-1".to_owned()),
            ..Default::default()
        };
        let replaced = replace_generated_subtitle_tracks(vec![storyboard, user], generated);
        assert_eq!(replaced.len(), 2);
        assert_eq!(replaced[0].id, "user-subs");
        assert_eq!(replaced[1].origin, "voice_alignment");
    }
}
