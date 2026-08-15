//! 从已持久化 timeline 生成可重建的低清 preview；不负责最终导出或覆盖用户文件。
use crate::db::open_connection;
use crate::models::{
    MusicTrack, PreviewQualityCheck, PreviewQualityReport, PreviewResult, TextAnimation, TextCue,
    TextTrack, TimelineClip, TimelineContent,
};
use crate::process::hidden_command;
use crate::timeline::load_timeline_version;
use rusqlite::params;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

fn preview_directory(app: &AppHandle, timeline_version_id: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("previews")
        .join(timeline_version_id);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

fn render_timeline_clip(
    source: &Path,
    kind: &str,
    clip: &TimelineClip,
    destination: &Path,
) -> Result<(), String> {
    let duration = (clip.timeline_end_ms - clip.timeline_start_ms) as f64 / 1000.0;
    let mut command = hidden_command("ffmpeg");
    command.args(["-y", "-hide_banner", "-loglevel", "error"]);
    if kind == "video" {
        command
            .args([
                "-ss",
                &format!("{:.3}", clip.source_start_ms as f64 / 1000.0),
                "-i",
            ])
            .arg(source)
            .args(["-t", &format!("{duration:.3}")]);
    } else if kind == "image" {
        command
            .args(["-loop", "1", "-i"])
            .arg(source)
            .args(["-t", &format!("{duration:.3}")]);
    } else {
        return Err("Timeline clip uses unsupported media.".to_owned());
    }
    let status = command
        .args([
            "-vf",
            "scale=540:960:force_original_aspect_ratio=increase,crop=540:960,fps=30,format=yuv420p",
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-movflags",
            "+faststart",
        ])
        .arg(destination)
        .status()
        .map_err(|_| "FFmpeg is not available on this computer.".to_owned())?;
    if status.success() {
        Ok(())
    } else {
        Err("FFmpeg could not render a timeline clip.".to_owned())
    }
}

fn visual_signature(path: &Path, duration_ms: i64) -> Option<Vec<u8>> {
    let midpoint = (duration_ms.max(1) as f64 / 2_000.0).to_string();
    let output = hidden_command("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-ss", &midpoint, "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=24:24,format=gray",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .ok()?;
    (output.status.success() && output.stdout.len() == 24 * 24).then_some(output.stdout)
}

fn mean_pixel_difference(first: &[u8], second: &[u8]) -> Option<f64> {
    (first.len() == second.len() && !first.is_empty()).then(|| {
        first
            .iter()
            .zip(second)
            .map(|(left, right)| left.abs_diff(*right) as u64)
            .sum::<u64>() as f64
            / first.len() as f64
    })
}

fn ass_timestamp(milliseconds: i64) -> String {
    let centiseconds = (milliseconds.max(0) + 5) / 10;
    format!(
        "{}:{:02}:{:02}.{:02}",
        centiseconds / 360_000,
        (centiseconds / 6_000) % 60,
        (centiseconds / 100) % 60,
        centiseconds % 100
    )
}

fn ass_color(color: &str) -> String {
    let hex = color.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.bytes().all(|value| value.is_ascii_hexdigit()) {
        return "&H00FFFFFF".to_owned();
    }
    format!("&H00{}{}{}", &hex[4..6], &hex[2..4], &hex[0..2])
}

fn ass_font_name(font_key: &str) -> &'static str {
    match font_key {
        "sans_bold"
        | "jianying_default"
        | "jianying_sans_bold"
        | "jianying_sans_regular"
        | "jianying_harmony_bold" => "Microsoft YaHei",
        "sans_clean" => "Arial",
        "serif_editorial" | "jianying_serif_bold" => "Georgia",
        "jianying_handwritten" => "KaiTi",
        "mono_tech" => "Consolas",
        _ => "Microsoft YaHei",
    }
}

fn ass_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace("\r\n", "\\N")
        .replace('\n', "\\N")
}

fn ass_alignment(anchor: &str) -> (i64, i64) {
    match anchor {
        "top_left" => (7, 0),
        "top_center" => (8, 0),
        "top_right" => (9, 0),
        "middle_left" => (4, 0),
        "middle_center" => (5, 0),
        "middle_right" => (6, 0),
        "bottom_left" => (1, 0),
        "bottom_right" => (3, 0),
        _ => (2, 0),
    }
}

fn animation_tags(cue: &TextCue, animation: Option<&TextAnimation>, phase: &str) -> String {
    let Some(animation) = animation else {
        return String::new();
    };
    let distance = (40.0 * animation.intensity).round() as i64;
    let x = (cue.layout.x * 540.0).round() as i64;
    let y = (cue.layout.y * 960.0).round() as i64;
    match (animation.template_id.as_str(), phase) {
        ("fade" | "wipe", "in") => format!("\\fad({},0)", animation.duration_ms),
        ("fade" | "wipe", "out") => format!("\\fad(0,{})", animation.duration_ms),
        ("slide_up", "in") => format!(
            "\\move({x},{},{x},{y},0,{})",
            y + distance,
            animation.duration_ms
        ),
        ("slide_down", "in") => format!(
            "\\move({x},{},{x},{y},0,{})",
            y - distance,
            animation.duration_ms
        ),
        ("pop", "in") => format!(
            "\\fscx70\\fscy70\\t(0,{},\\fscx100\\fscy100)",
            animation.duration_ms
        ),
        _ => String::new(),
    }
}

fn write_text_tracks_ass(path: &Path, tracks: &[TextTrack]) -> Result<(), String> {
    let mut styles = String::new();
    let mut events = String::new();
    let mut style_index = 0_usize;
    for track in tracks.iter().filter(|track| track.enabled) {
        for cue in &track.cues {
            let style_name = format!("cue_{style_index}");
            style_index += 1;
            let (alignment, _) = ass_alignment(&cue.layout.anchor);
            let font_size = (cue.style.font_size * 960.0).round().max(12.0) as i64;
            let outline = cue.style.stroke_width.max(0.0);
            let shadow = if cue.style.shadow { 2 } else { 0 };
            styles.push_str(&format!(
                "Style: {style_name},{},{font_size},{},{},{},&H00000000,{},0,0,0,100,100,0,0,{alignment},0,0,{outline:.1},{shadow},0,0,0,1\n",
                ass_font_name(&cue.style.font_key),
                ass_color(&cue.style.color),
                ass_color(&cue.style.color),
                ass_color(cue.style.stroke_color.as_deref().unwrap_or("#000000")),
                if cue.style.bold { -1 } else { 0 },
            ));
            let x = (cue.layout.x * 540.0).round() as i64;
            let y = (cue.layout.y * 960.0).round() as i64;
            let tags = format!(
                "{{\\an{alignment}\\pos({x},{y}){}{}}}",
                animation_tags(cue, cue.entrance.as_ref(), "in"),
                animation_tags(cue, cue.exit.as_ref(), "out")
            );
            events.push_str(&format!(
                "Dialogue: {},{},{},{style_name},,0,0,0,,{tags}{}\n",
                track.layer,
                ass_timestamp(cue.start_ms),
                ass_timestamp(cue.end_ms),
                ass_escape(&cue.text)
            ));
        }
    }
    let content = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 540\nPlayResY: 960\n\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Alignment,MarginL,MarginR,MarginV,Outline,Shadow,Encoding\n{styles}\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\n{events}"
    );
    fs::write(path, content).map_err(|_| "Could not prepare text preview overlays.".to_owned())
}

fn ass_filter_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace(':', "\\:")
        .replace('\'', "\\'")
}

fn music_filter_for_cue(input_index: usize, cue: &crate::models::MusicCue) -> String {
    let source_duration_ms = cue.source_end_ms - cue.source_start_ms;
    let timeline_duration_ms = cue.timeline_end_ms - cue.timeline_start_ms;
    let mut filter = format!(
        "[{input_index}:a]aresample=48000,atrim=start={:.3}:end={:.3},asetpts=PTS-STARTPTS",
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
            (timeline_duration_ms - cue.fade_out_ms) as f64 / 1000.0,
            cue.fade_out_ms as f64 / 1000.0
        ));
    }
    filter.push_str(&format!(
        ",adelay={}:all=1[music{}]",
        cue.timeline_start_ms,
        input_index - 1
    ));
    filter
}

fn mix_music_tracks(
    connection: &rusqlite::Connection,
    project_id: &str,
    video_path: &Path,
    output_path: &Path,
    tracks: &[MusicTrack],
) -> Result<bool, String> {
    let cues = tracks
        .iter()
        .filter(|track| track.enabled)
        .flat_map(|track| track.cues.iter())
        .collect::<Vec<_>>();
    if cues.is_empty() {
        return Ok(false);
    }
    let mut command = hidden_command("ffmpeg");
    command
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(video_path);
    let mut filters = Vec::new();
    for (index, cue) in cues.iter().enumerate() {
        let source: String = connection.query_row(
            "SELECT source_reference FROM assets WHERE id = ?1 AND project_id = ?2 AND kind = 'audio'",
            params![cue.asset_id, project_id], |row| row.get(0),
        ).map_err(|_| "Music cue references an unavailable audio asset.".to_owned())?;
        if !Path::new(&source).is_file() {
            return Err("Music source media is no longer available.".to_owned());
        }
        command.arg("-i").arg(source);
        filters.push(music_filter_for_cue(index + 1, cue));
    }
    let labels = (0..cues.len())
        .map(|index| format!("[music{index}]"))
        .collect::<String>();
    if cues.len() == 1 {
        filters.push("[music0]anull[aout]".to_owned());
    } else {
        filters.push(format!(
            "{labels}amix=inputs={}:normalize=0[aout]",
            cues.len()
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
            "-shortest",
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
        .ok_or_else(|| "FFmpeg could not mix music into the preview.".to_owned())
}

fn inspect_preview_quality(
    preview_path: &Path,
    clips: &[TimelineClip],
    rendered: &[PathBuf],
    text_tracks: &[TextTrack],
) -> PreviewQualityReport {
    let mut checks = Vec::new();
    let output = hidden_command("ffmpeg")
        .args(["-hide_banner", "-loglevel", "info", "-i"])
        .arg(preview_path)
        .args([
            "-vf",
            "blackdetect=d=0.10:pix_th=0.10",
            "-an",
            "-f",
            "null",
            "-",
        ])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let black_segments = String::from_utf8_lossy(&output.stderr)
                .lines()
                .filter(|line| line.contains("black_start:"))
                .count();
            if black_segments > 0 {
                checks.push(PreviewQualityCheck {
                    category: "black_frames".to_owned(),
                    severity: "warning".to_owned(),
                    message: format!(
                        "Detected {black_segments} black-frame segment(s) in the rendered preview."
                    ),
                    shot_indices: Vec::new(),
                });
            }
        }
        _ => checks.push(PreviewQualityCheck {
            category: "black_frames".to_owned(),
            severity: "info".to_owned(),
            message: "Black-frame scan could not complete; the preview remains available."
                .to_owned(),
            shot_indices: Vec::new(),
        }),
    }
    for (index, clip) in clips.iter().enumerate() {
        for other in clips.iter().skip(index + 1) {
            if clip.asset_id == other.asset_id
                && clip.source_start_ms == other.source_start_ms
                && clip.source_end_ms == other.source_end_ms
            {
                checks.push(PreviewQualityCheck {
                    category: "duplicate_footage".to_owned(),
                    severity: "warning".to_owned(),
                    message: "Two timeline shots use the same source range.".to_owned(),
                    shot_indices: vec![clip.shot_index, other.shot_index],
                });
            }
        }
    }
    let signatures = clips
        .iter()
        .zip(rendered)
        .map(|(clip, path)| visual_signature(path, clip.timeline_end_ms - clip.timeline_start_ms))
        .collect::<Vec<_>>();
    for (index, clip) in clips.iter().enumerate() {
        for (other_index, other) in clips.iter().enumerate().skip(index + 1) {
            if clip.asset_id == other.asset_id
                && clip.source_start_ms == other.source_start_ms
                && clip.source_end_ms == other.source_end_ms
            {
                continue;
            }
            if let (Some(first), Some(second)) = (&signatures[index], &signatures[other_index]) {
                if mean_pixel_difference(first, second).is_some_and(|difference| difference < 12.0)
                {
                    checks.push(PreviewQualityCheck {
                        category: "visual_similarity".to_owned(), severity: "warning".to_owned(),
                        message: "Two different source ranges have highly similar sampled frames; review for repeated footage.".to_owned(),
                        shot_indices: vec![clip.shot_index, other.shot_index],
                    });
                }
            }
        }
    }
    let caption_shots = clips
        .iter()
        .filter_map(|clip| (!clip.on_screen_text.trim().is_empty()).then_some(clip.shot_index))
        .collect::<Vec<_>>();
    if !caption_shots.is_empty() && text_tracks.is_empty() {
        checks.push(PreviewQualityCheck {
            category: "subtitles".to_owned(),
            severity: "info".to_owned(),
            message: "Storyboard text is not yet rendered as captions in previews.".to_owned(),
            shot_indices: caption_shots,
        });
    }
    for track in text_tracks.iter().filter(|track| track.enabled) {
        for cue in &track.cues {
            if cue.end_ms - cue.start_ms < 600 {
                checks.push(PreviewQualityCheck {
                    category: "text_readability".to_owned(),
                    severity: "warning".to_owned(),
                    message: "A text cue is on screen for less than 600 ms.".to_owned(),
                    shot_indices: Vec::new(),
                });
            }
            if cue.layout.y > 0.91 && cue.layout.safe_area != "allow_bottom" {
                checks.push(PreviewQualityCheck {
                    category: "text_safe_area".to_owned(),
                    severity: "warning".to_owned(),
                    message: "A text cue enters the default 9:16 bottom safe area.".to_owned(),
                    shot_indices: Vec::new(),
                });
            }
        }
    }
    PreviewQualityReport { checks }
}

#[tauri::command]
pub fn render_preview(
    app: AppHandle,
    timeline_version_id: String,
) -> Result<PreviewResult, String> {
    log::info!("Starting local preview render.");
    let connection = open_connection(&app)?;
    let timeline = load_timeline_version(&connection, &timeline_version_id)?;
    if timeline.clips.is_empty() {
        return Err("Timeline has no clips to render.".to_owned());
    }
    let directory = preview_directory(&app, &timeline.id)?;
    let mut rendered = Vec::with_capacity(timeline.clips.len());
    for (index, clip) in timeline.clips.iter().enumerate() {
        let (source_reference, kind): (String, String) = connection
            .query_row(
                "SELECT source_reference, kind FROM assets WHERE id = ?1 AND project_id = ?2",
                params![clip.asset_id, timeline.project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Timeline references an unavailable asset.".to_owned())?;
        if !Path::new(&source_reference).is_file() {
            return Err("Timeline source media is no longer available. Reconnect or replace the missing asset before rendering.".to_owned());
        }
        let destination = directory.join(format!("clip_{index:03}.mp4"));
        render_timeline_clip(Path::new(&source_reference), &kind, clip, &destination)?;
        rendered.push(destination);
    }
    let list_path = directory.join("concat.txt");
    let list = rendered
        .iter()
        .map(|path| {
            format!(
                "file '{}'",
                path.to_string_lossy()
                    .replace('\\', "/")
                    .replace('\'', "\\'")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&list_path, list).map_err(|_| "Could not prepare preview sequence.".to_owned())?;
    let preview_path = directory.join("preview.mp4");
    let assembled_path = if timeline.text_tracks.is_empty() {
        preview_path.clone()
    } else {
        directory.join("preview_video.mp4")
    };
    let status = hidden_command("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&list_path)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(&assembled_path)
        .status()
        .map_err(|_| "FFmpeg is not available on this computer.".to_owned())?;
    if !status.success() {
        return Err("FFmpeg could not assemble the preview.".to_owned());
    }
    if !timeline.text_tracks.is_empty() {
        let ass_path = directory.join("text_tracks.ass");
        write_text_tracks_ass(&ass_path, &timeline.text_tracks)?;
        let filter = format!("ass='{}'", ass_filter_path(&ass_path));
        let status = hidden_command("ffmpeg")
            .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(&assembled_path)
            .args([
                "-vf",
                &filter,
                "-an",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-movflags",
                "+faststart",
            ])
            .arg(&preview_path)
            .status()
            .map_err(|_| "FFmpeg is not available on this computer.".to_owned())?;
        if !status.success() {
            return Err("FFmpeg could not render text tracks in the preview.".to_owned());
        }
    }
    if !timeline.music_tracks.is_empty() {
        let mixed_path = directory.join("preview_mixed.mp4");
        if mix_music_tracks(
            &connection,
            &timeline.project_id,
            &preview_path,
            &mixed_path,
            &timeline.music_tracks,
        )? {
            fs::rename(&mixed_path, &preview_path)
                .map_err(|_| "Could not finalize the music preview.".to_owned())?;
        }
    }
    let quality_report = inspect_preview_quality(
        &preview_path,
        &timeline.clips,
        &rendered,
        &timeline.text_tracks,
    );
    let content_json = serde_json::to_string(&TimelineContent {
        clips: timeline.clips.clone(),
        text_tracks: timeline.text_tracks.clone(),
        music_tracks: timeline.music_tracks.clone(),
        quality_report: Some(quality_report.clone()),
    })
    .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE timeline_versions SET status = 'preview_ready', content_json = ?1 WHERE id = ?2",
            params![content_json, timeline.id],
        )
        .map_err(|error| error.to_string())?;
    log::info!("Completed local preview render.");
    Ok(PreviewResult {
        timeline_version_id: timeline.id,
        preview_path: preview_path.to_string_lossy().into_owned(),
        quality_report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MusicCue, TextLayout, TextStyle};
    use rusqlite::Connection;
    use uuid::Uuid;

    #[test]
    fn visual_signature_comparison_distinguishes_similar_frames() {
        assert_eq!(
            mean_pixel_difference(&[10, 12, 14], &[11, 13, 15]),
            Some(1.0)
        );
        assert_eq!(mean_pixel_difference(&[0, 0], &[255, 255]), Some(255.0));
        assert_eq!(mean_pixel_difference(&[1], &[1, 2]), None);
    }

    #[test]
    fn ffmpeg_renders_a_source_bound_vertical_clip() {
        let directory =
            std::env::temp_dir().join(format!("assembly-video-agent-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create temporary test directory");
        let source = directory.join("source.mp4");
        let source_status = hidden_command("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=640x360:rate=30",
                "-t",
                "2",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .expect("run ffmpeg test source generation");
        assert!(
            source_status.success(),
            "ffmpeg must generate a test source"
        );
        let destination = directory.join("clip.mp4");
        let clip = TimelineClip {
            shot_index: 1,
            asset_id: "test".to_owned(),
            source_start_ms: 0,
            source_end_ms: 1_000,
            timeline_start_ms: 0,
            timeline_end_ms: 1_000,
            on_screen_text: String::new(),
        };
        render_timeline_clip(&source, "video", &clip, &destination)
            .expect("render vertical timeline clip");
        assert!(
            destination.is_file(),
            "timeline render must create an MP4 clip"
        );
        let probe = hidden_command("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0",
            ])
            .arg(&destination)
            .output()
            .expect("run ffprobe on rendered clip");
        assert!(
            probe.status.success(),
            "ffprobe must read the rendered clip"
        );
        assert!(
            String::from_utf8_lossy(&probe.stdout).contains("540,960"),
            "rendered preview clip must be 540 x 960"
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn ffmpeg_assembles_normalized_clips_into_a_preview() {
        let directory = std::env::temp_dir().join(format!(
            "assembly-video-agent-preview-test-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("create temporary preview directory");
        let source = directory.join("source.mp4");
        let source_status = hidden_command("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=30",
                "-t",
                "3",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .status()
            .expect("generate preview source");
        assert!(
            source_status.success(),
            "ffmpeg must generate preview source"
        );
        let first = directory.join("clip_000.mp4");
        let second = directory.join("clip_001.mp4");
        let clip = TimelineClip {
            shot_index: 1,
            asset_id: "test".to_owned(),
            source_start_ms: 0,
            source_end_ms: 1_000,
            timeline_start_ms: 0,
            timeline_end_ms: 1_000,
            on_screen_text: String::new(),
        };
        render_timeline_clip(&source, "video", &clip, &first).expect("render first clip");
        let second_clip = TimelineClip {
            shot_index: 2,
            asset_id: "test".to_owned(),
            source_start_ms: 1_000,
            source_end_ms: 2_000,
            timeline_start_ms: 1_000,
            timeline_end_ms: 2_000,
            on_screen_text: String::new(),
        };
        render_timeline_clip(&source, "video", &second_clip, &second).expect("render second clip");
        let list = directory.join("concat.txt");
        fs::write(
            &list,
            format!(
                "file '{}'\nfile '{}'",
                first.to_string_lossy().replace('\\', "/"),
                second.to_string_lossy().replace('\\', "/")
            ),
        )
        .expect("write concat list");
        let preview = directory.join("preview.mp4");
        let status = hidden_command("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
            ])
            .arg(&list)
            .args(["-c", "copy"])
            .arg(&preview)
            .status()
            .expect("assemble preview");
        assert!(
            status.success() && preview.is_file(),
            "ffmpeg must assemble the preview"
        );
        let probe = hidden_command("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=nw=1:nk=1",
            ])
            .arg(&preview)
            .output()
            .expect("probe preview duration");
        let duration = String::from_utf8_lossy(&probe.stdout)
            .trim()
            .parse::<f64>()
            .expect("parse preview duration");
        assert!(
            (1.8..=2.2).contains(&duration),
            "preview must contain both one-second clips"
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn music_filter_trims_loops_and_delays_in_milliseconds() {
        let cue = MusicCue {
            id: "cue-1".to_owned(),
            asset_id: "audio-1".to_owned(),
            source_start_ms: 1_000,
            source_end_ms: 2_000,
            timeline_start_ms: 500,
            timeline_end_ms: 3_500,
            loop_enabled: true,
            volume: 0.5,
            fade_in_ms: 100,
            fade_out_ms: 200,
            jianying_compatibility: "not_deliverable".to_owned(),
            provider: None,
            license_url: None,
            attribution: None,
        };
        let filter = music_filter_for_cue(1, &cue);
        assert!(filter.contains("atrim=start=1.000:end=2.000"));
        assert!(filter.contains("aloop=loop=2:size=48000,atrim=duration=3.000"));
        assert!(filter.contains("adelay=500:all=1"));
        assert!(!filter.contains("adelay=24000"));
    }

    #[test]
    fn ffmpeg_mixes_a_looped_music_cue_into_a_playable_preview() {
        let directory =
            std::env::temp_dir().join(format!("assembly-music-preview-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create music preview test directory");
        let video = directory.join("video.mp4");
        let audio = directory.join("audio.wav");
        let output = directory.join("mixed.mp4");
        assert!(hidden_command("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=540x960:r=30:d=3",
                "-c:v",
                "libx264"
            ])
            .arg(&video)
            .status()
            .expect("create video")
            .success());
        assert!(hidden_command("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "aevalsrc=if(lt(t\\,1)\\,0\\,0.5*sin(2*PI*440*t)):s=48000:d=2"
            ])
            .arg(&audio)
            .status()
            .expect("create audio")
            .success());
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE assets (id TEXT, project_id TEXT, kind TEXT, source_reference TEXT);",
            )
            .expect("create asset table");
        connection
            .execute(
                "INSERT INTO assets VALUES ('audio-1', 'project-1', 'audio', ?1)",
                params![audio.to_string_lossy()],
            )
            .expect("insert audio asset");
        let cue = MusicCue {
            id: "cue-1".to_owned(),
            asset_id: "audio-1".to_owned(),
            source_start_ms: 1_000,
            source_end_ms: 2_000,
            timeline_start_ms: 1_000,
            timeline_end_ms: 3_000,
            loop_enabled: true,
            volume: 1.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            jianying_compatibility: "not_deliverable".to_owned(),
            provider: None,
            license_url: None,
            attribution: None,
        };
        assert!(mix_music_tracks(
            &connection,
            "project-1",
            &video,
            &output,
            &[MusicTrack {
                id: "music-1".to_owned(),
                enabled: true,
                cues: vec![cue]
            }]
        )
        .expect("mix music"));
        let probe = hidden_command("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,duration",
                "-of",
                "default=nw=1",
            ])
            .arg(&output)
            .output()
            .expect("probe mixed preview");
        assert!(
            probe.status.success()
                && String::from_utf8_lossy(&probe.stdout).contains("codec_type=audio"),
            "mixed preview must retain a playable audio stream"
        );
        fs::remove_dir_all(directory).expect("remove music preview test directory");
    }

    #[test]
    fn disabled_music_tracks_do_not_create_a_replacement_preview() {
        let connection = Connection::open_in_memory().expect("open database");
        assert!(!mix_music_tracks(
            &connection,
            "project-1",
            Path::new("missing.mp4"),
            Path::new("unused.mp4"),
            &[MusicTrack {
                id: "music-1".to_owned(),
                enabled: false,
                cues: Vec::new()
            }]
        )
        .expect("skip disabled music"));
    }

    #[test]
    fn ass_text_tracks_use_the_local_libass_filter() {
        let directory =
            std::env::temp_dir().join(format!("assembly-text-preview-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("create text preview test directory");
        let ass_path = directory.join("text.ass");
        let track = TextTrack {
            id: "track-1".to_owned(),
            role: "headline".to_owned(),
            layer: 0,
            enabled: true,
            cues: vec![TextCue {
                id: "cue-1".to_owned(),
                template_id: None,
                start_ms: 0,
                end_ms: 1_000,
                text: "Preview text".to_owned(),
                style: TextStyle {
                    font_key: "sans_bold".to_owned(),
                    font_size: 0.08,
                    bold: true,
                    color: "#FFFFFF".to_owned(),
                    stroke_color: Some("#000000".to_owned()),
                    stroke_width: 2.0,
                    shadow: true,
                    background_color: None,
                    alignment: "center".to_owned(),
                    letter_spacing: 0,
                    line_spacing: 0,
                },
                layout: TextLayout {
                    anchor: "middle_center".to_owned(),
                    x: 0.5,
                    y: 0.5,
                    max_width: 0.8,
                    safe_area: "default".to_owned(),
                },
                entrance: Some(TextAnimation {
                    template_id: "fade".to_owned(),
                    duration_ms: 200,
                    intensity: 1.0,
                }),
                exit: None,
                loop_animation: None,
                jianying_compatibility: "local_preview_only".to_owned(),
            }],
        };
        write_text_tracks_ass(&ass_path, &[track]).expect("write ASS overlay");
        let source = directory.join("source.mp4");
        let output = directory.join("output.mp4");
        let create = hidden_command("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=540x960:d=1",
                "-c:v",
                "libx264",
            ])
            .arg(&source)
            .status()
            .expect("start source ffmpeg");
        assert!(create.success(), "create source preview video");
        let filter = format!("ass='{}'", ass_filter_path(&ass_path));
        let render = hidden_command("ffmpeg")
            .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(&source)
            .args(["-vf", &filter, "-c:v", "libx264"])
            .arg(&output)
            .status()
            .expect("start ASS ffmpeg");
        assert!(
            render.success() && output.is_file(),
            "render ASS text overlay"
        );
        fs::remove_dir_all(directory).expect("remove text preview test directory");
    }
}
