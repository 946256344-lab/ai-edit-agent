// preview.rs 的独立测试模块，通过 #[path] 挂载。
// 覆盖：render_timeline_clip 源范围截断、FFmpeg 片段渲染/拼接、音乐混音、ASS 文字轨渲染。
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

// 源时长短于时间线槽位时，渲染时长必须被收敛到源范围上限，
// 而不是静默地用黑帧或静帧填满剩余槽位。
#[test]
fn render_timeline_clip_clamps_duration_to_source_range() {
    let directory = std::env::temp_dir().join(format!(
        "assembly-video-agent-clamp-test-{}",
        Uuid::new_v4()
    ));
    fs::create_dir_all(&directory).expect("create temporary clamp test directory");
    let source = directory.join("source.mp4");
    // 生成 2 秒合成源
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
        "ffmpeg must generate a 2-second test source"
    );
    let destination = directory.join("clip.mp4");
    // 源范围 500 ms（250–750 ms），时间线槽位 3000 ms；渲染应截止于源范围。
    let clip = TimelineClip {
        shot_index: 1,
        asset_id: "test".to_owned(),
        source_start_ms: 250,
        source_end_ms: 750,
        timeline_start_ms: 0,
        timeline_end_ms: 3_000,
        on_screen_text: String::new(),
    };
    render_timeline_clip(&source, "video", &clip, &destination)
        .expect("render source-bound clip");
    assert!(
        destination.is_file(),
        "timeline render must create an MP4 clip"
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
        .arg(&destination)
        .output()
        .expect("run ffprobe on clamped clip");
    assert!(probe.status.success(), "ffprobe must read the clamped clip");
    let rendered_duration = String::from_utf8_lossy(&probe.stdout)
        .trim()
        .parse::<f64>()
        .expect("parse clamped clip duration");
    // 渲染时长必须约等于 0.5 s（源范围），而不是 3.0 s（时间线槽位）。
    assert!(
        (0.4..=0.7).contains(&rendered_duration),
        "rendered clip must be clamped to the 500 ms source range, got {rendered_duration:.3}s"
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
