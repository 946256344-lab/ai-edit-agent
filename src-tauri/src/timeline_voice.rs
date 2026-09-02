//! 把已合成的旁白写入新时间线版本：口播是时钟，画面只可补不可截音频。

use crate::models::{
    PreviewQualityCheck, TextTrack, TimelineClip, TimelineVersion, VoiceoverCue, VoiceoverTrack,
};
use crate::timeline::{insert_timeline_version_with_log, validate_text_tracks};
use rusqlite::Connection;

pub(crate) const VOICEOVER_TAIL_MS: i64 = 500;
pub(crate) const EXCESSIVE_VISUAL_TAIL_MS: i64 = 1_500;

pub(crate) fn fit_visual_to_voiceover(
    clips: Vec<TimelineClip>,
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
    let deficit = (voice_duration_ms + VOICEOVER_TAIL_MS) - visual;
    Err(format!(
        "voiceover_longer_than_picture: visual={visual} voice={voice_duration_ms} deficit={deficit} hint=freeze_frame is forbidden. Use search_asset_segments and then insert_clips or change_clip_duration/replace_clips to add {deficit}ms within verified source ranges."
    ))
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
    fn short_picture_returns_deficit_error_and_never_uses_freeze_frame() {
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
        let error = fit_visual_to_voiceover(clips, 3_000).unwrap_err();
        assert!(error.starts_with("voiceover_longer_than_picture:"));
        assert!(error.contains("deficit="));
        assert!(error.contains("freeze_frame is forbidden"));
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
