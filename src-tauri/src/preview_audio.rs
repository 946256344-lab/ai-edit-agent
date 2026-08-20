//! Preview 音频合成：旁白完整保留，BGM 铺到画面时长，禁止 `-shortest` 截断口播。

use crate::models::{MusicCue, MusicTrack, VoiceoverCue, VoiceoverTrack};
use crate::process::hidden_command;
use rusqlite::params;
use std::path::Path;

struct AudioInput {
    source: String,
    filter: String,
    label: String,
}

pub(crate) fn music_filter_for_cue(input_index: usize, cue: &MusicCue, label: &str) -> String {
    let source_duration_ms = (cue.source_end_ms - cue.source_start_ms).max(1);
    let timeline_duration_ms = (cue.timeline_end_ms - cue.timeline_start_ms).max(1);
    let mut filter = format!(
        "[{input_index}:a]aresample=48000,aformat=channel_layouts=stereo,atrim=start={:.3}:end={:.3},asetpts=PTS-STARTPTS",
        cue.source_start_ms as f64 / 1000.0,
        cue.source_end_ms as f64 / 1000.0,
    );
    if cue.loop_enabled {
        let loop_count = (timeline_duration_ms + source_duration_ms - 1) / source_duration_ms - 1;
        filter.push_str(&format!(
            ",aloop=loop={loop_count}:size={},atrim=duration={:.3},asetpts=PTS-STARTPTS",
            source_duration_ms * 48,
            timeline_duration_ms as f64 / 1000.0,
        ));
    } else {
        filter.push_str(&format!(
            ",atrim=duration={:.3}",
            timeline_duration_ms as f64 / 1000.0,
        ));
    }
    filter.push_str(&format!(",volume={:.3}", cue.volume));
    if cue.fade_in_ms > 0 {
        filter.push_str(&format!(
            ",afade=t=in:st=0:d={:.3}",
            cue.fade_in_ms as f64 / 1000.0
        ));
    }
    if cue.fade_out_ms > 0 {
        filter.push_str(&format!(
            ",afade=t=out:st={:.3}:d={:.3}",
            (timeline_duration_ms - cue.fade_out_ms).max(0) as f64 / 1000.0,
            cue.fade_out_ms as f64 / 1000.0
        ));
    }
    filter.push_str(&format!(",adelay={}:all=1[{label}]", cue.timeline_start_ms));
    filter
}

fn voiceover_filter_for_cue(input_index: usize, cue: &VoiceoverCue, label: &str) -> String {
    let source_duration_ms = (cue.source_end_ms - cue.source_start_ms).max(1);
    let mut filter = format!(
        "[{input_index}:a]aresample=48000,aformat=channel_layouts=stereo,atrim=start={:.3}:end={:.3},asetpts=PTS-STARTPTS,volume={:.3}",
        cue.source_start_ms as f64 / 1000.0,
        cue.source_end_ms as f64 / 1000.0,
        cue.volume,
    );
    if cue.fade_in_ms > 0 {
        filter.push_str(&format!(
            ",afade=t=in:st=0:d={:.3}",
            cue.fade_in_ms as f64 / 1000.0
        ));
    }
    if cue.fade_out_ms > 0 {
        filter.push_str(&format!(
            ",afade=t=out:st={:.3}:d={:.3}",
            (source_duration_ms - cue.fade_out_ms).max(0) as f64 / 1000.0,
            cue.fade_out_ms as f64 / 1000.0
        ));
    }
    filter.push_str(&format!(",adelay={}:all=1[{label}]", cue.timeline_start_ms));
    filter
}

fn lookup_audio_source(
    connection: &rusqlite::Connection,
    project_id: &str,
    asset_id: &str,
    kind: &str,
) -> Result<String, String> {
    let source: String = connection
        .query_row(
            "SELECT source_reference FROM assets WHERE id = ?1 AND project_id = ?2 AND kind = 'audio'",
            params![asset_id, project_id],
            |row| row.get(0),
        )
        .map_err(|_| format!("{kind} cue references an unavailable audio asset."))?;
    if !Path::new(&source).is_file() {
        return Err(format!("{kind} source media is no longer available."));
    }
    Ok(source)
}

pub(crate) fn mix_preview_audio(
    connection: &rusqlite::Connection,
    project_id: &str,
    video_path: &Path,
    output_path: &Path,
    music_tracks: &[MusicTrack],
    voiceover_tracks: &[VoiceoverTrack],
    preview_duration_ms: i64,
) -> Result<bool, String> {
    let mut inputs = Vec::new();
    for cue in voiceover_tracks
        .iter()
        .filter(|track| track.enabled)
        .flat_map(|track| track.cues.iter())
    {
        let source = lookup_audio_source(connection, project_id, &cue.asset_id, "Voiceover")?;
        let label = format!("voice{}", inputs.len());
        let filter = voiceover_filter_for_cue(inputs.len() + 1, cue, &label);
        inputs.push(AudioInput {
            source,
            filter,
            label,
        });
    }
    for cue in music_tracks
        .iter()
        .filter(|track| track.enabled)
        .flat_map(|track| track.cues.iter())
    {
        let source = lookup_audio_source(connection, project_id, &cue.asset_id, "Music")?;
        let label = format!("music{}", inputs.len());
        let filter = music_filter_for_cue(inputs.len() + 1, cue, &label);
        inputs.push(AudioInput {
            source,
            filter,
            label,
        });
    }
    if inputs.is_empty() {
        return Ok(false);
    }
    let preview_seconds = preview_duration_ms.max(1) as f64 / 1000.0;
    let mut command = hidden_command("ffmpeg");
    command
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(video_path);
    for input in &inputs {
        command.arg("-i").arg(&input.source);
    }
    let mut filters: Vec<String> = inputs.iter().map(|input| input.filter.clone()).collect();
    let labels = inputs
        .iter()
        .map(|input| format!("[{}]", input.label))
        .collect::<String>();
    if inputs.len() == 1 {
        filters.push(format!(
            "[{}]apad,atrim=duration={preview_seconds:.3},alimiter=limit=0.89[aout]",
            inputs[0].label
        ));
    } else {
        filters.push(format!(
            "{labels}amix=inputs={}:duration=longest:dropout_transition=0:normalize=0,alimiter=limit=0.89,atrim=duration={preview_seconds:.3}[aout]",
            inputs.len()
        ));
    }
    command
        .args([
            "-filter_complex",
            &filters.join(";"),
            "-map",
            "0:v:0",
            "-map",
            "[aout]",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-t",
            &format!("{preview_seconds:.3}"),
            "-movflags",
            "+faststart",
        ])
        .arg(output_path);
    let status = command
        .status()
        .map_err(|_| "FFmpeg is not available on this computer.".to_owned())?;
    status
        .success()
        .then_some(true)
        .ok_or_else(|| "FFmpeg could not mix voiceover or music into the preview.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{music_filter_for_cue, voiceover_filter_for_cue};
    use crate::models::{MusicCue, VoiceoverCue};

    #[test]
    fn voiceover_filter_keeps_full_source_window() {
        let cue = VoiceoverCue {
            id: "v1".to_owned(),
            asset_id: "a1".to_owned(),
            generation_id: "g1".to_owned(),
            source_start_ms: 0,
            source_end_ms: 3_200,
            timeline_start_ms: 0,
            timeline_end_ms: 3_200,
            volume: 1.0,
            fade_in_ms: 0,
            fade_out_ms: 80,
            provider: "ElevenLabs".to_owned(),
            voice_id: "voice".to_owned(),
            voice_name: "Charlie".to_owned(),
        };
        let filter = voiceover_filter_for_cue(1, &cue, "voice0");
        assert!(filter.contains("atrim=start=0.000:end=3.200"));
        assert!(!filter.contains("-shortest"));
        assert!(filter.contains("[voice0]"));
    }

    #[test]
    fn music_loop_still_trims_to_timeline_slot() {
        let cue = MusicCue {
            id: "m1".to_owned(),
            asset_id: "a1".to_owned(),
            source_start_ms: 0,
            source_end_ms: 1_000,
            timeline_start_ms: 0,
            timeline_end_ms: 3_000,
            loop_enabled: true,
            volume: 0.35,
            fade_in_ms: 0,
            fade_out_ms: 0,
            jianying_compatibility: "not_deliverable".to_owned(),
            provider: None,
            license_url: None,
            attribution: None,
        };
        let filter = music_filter_for_cue(1, &cue, "music0");
        assert!(filter.contains("aloop"));
        assert!(filter.contains("atrim=duration=3.000"));
    }
}
