//! 将 HandoffPlan 写成 OTIO JSON，供 DaVinci Resolve 导入。
//! 切点和源窗保真；慢放用 LinearTimeWarp，字幕不写入。

use super::{media_file_url, HandoffClip, HandoffPlan};
use serde_json::{json, Value};

pub(super) fn render_otio(plan: &HandoffPlan, project_name: &str) -> Value {
    let rate = plan.canvas.fps.max(1) as f64;
    let mut video_children = Vec::new();
    for clip in &plan.clips {
        video_children.push(otio_clip(clip, rate, true));
    }
    let mut tracks = vec![json!({
        "OTIO_SCHEMA": "Track.1",
        "name": "V1",
        "kind": "Video",
        "children": video_children,
    })];
    let mut audio_children = Vec::new();
    for track in &plan.voiceover_tracks {
        if !track.enabled {
            continue;
        }
        for cue in &track.cues {
            let Some(path) = cue.source_reference.as_deref() else {
                continue;
            };
            audio_children.push(otio_audio_clip(
                "voiceover",
                path,
                cue.cue.source_start_ms,
                cue.cue.source_end_ms,
                cue.cue.timeline_end_ms - cue.cue.timeline_start_ms,
                rate,
            ));
        }
    }
    for track in &plan.music_tracks {
        if !track.enabled {
            continue;
        }
        for cue in &track.cues {
            audio_children.push(otio_audio_clip(
                "music",
                &cue.source_reference,
                cue.cue.source_start_ms,
                cue.cue.source_end_ms,
                cue.cue.timeline_end_ms - cue.cue.timeline_start_ms,
                rate,
            ));
        }
    }
    if !audio_children.is_empty() {
        tracks.push(json!({
            "OTIO_SCHEMA": "Track.1",
            "name": "A1",
            "kind": "Audio",
            "children": audio_children,
        }));
    }
    json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": project_name,
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "name": "tracks",
            "children": tracks,
        },
    })
}

fn otio_clip(clip: &HandoffClip, rate: f64, with_warp: bool) -> Value {
    let source_span = (clip.source_end_ms - clip.source_start_ms).max(1);
    let timeline_span = (clip.timeline_end_ms - clip.timeline_start_ms).max(1);
    let mut item = json!({
        "OTIO_SCHEMA": "Clip.1",
        "name": format!("shot-{}", clip.shot_index),
        "source_range": time_range(clip.source_start_ms, source_span, rate),
        "media_reference": {
            "OTIO_SCHEMA": "ExternalReference.1",
            "name": clip.asset_id,
            "target_url": media_file_url(&clip.source_reference),
            "available_range": time_range(0, clip.source_end_ms.max(source_span), rate),
        },
    });
    if with_warp && (source_span - timeline_span).abs() > 50 {
        item["effects"] = json!([{
            "OTIO_SCHEMA": "LinearTimeWarp.1",
            "name": "Speed",
            "time_scalar": source_span as f64 / timeline_span as f64,
        }]);
    }
    item
}

fn otio_audio_clip(
    name: &str,
    path: &str,
    source_start_ms: i64,
    source_end_ms: i64,
    timeline_span_ms: i64,
    rate: f64,
) -> Value {
    let source_span = (source_end_ms - source_start_ms).max(1);
    let timeline_span = timeline_span_ms.max(1);
    let mut item = json!({
        "OTIO_SCHEMA": "Clip.1",
        "name": name,
        "source_range": time_range(source_start_ms, source_span, rate),
        "media_reference": {
            "OTIO_SCHEMA": "ExternalReference.1",
            "name": name,
            "target_url": media_file_url(path),
            "available_range": time_range(0, source_end_ms.max(source_span), rate),
        },
    });
    if (source_span - timeline_span).abs() > 50 {
        item["effects"] = json!([{
            "OTIO_SCHEMA": "LinearTimeWarp.1",
            "name": "Speed",
            "time_scalar": source_span as f64 / timeline_span as f64,
        }]);
    }
    item
}

fn time_range(start_ms: i64, duration_ms: i64, rate: f64) -> Value {
    json!({
        "OTIO_SCHEMA": "TimeRange.1",
        "start_time": rational_time(start_ms, rate),
        "duration": rational_time(duration_ms, rate),
    })
}

fn rational_time(ms: i64, rate: f64) -> Value {
    json!({
        "OTIO_SCHEMA": "RationalTime.1",
        "rate": rate,
        "value": ((ms.max(0) as f64) * rate / 1000.0).round(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::{build_handoff_plan, tests::sample_sources, tests::sample_timeline};

    #[test]
    fn otio_keeps_media_url_and_slow_warp() {
        let plan = build_handoff_plan(&sample_timeline(), &sample_sources()).expect("plan");
        let otio = render_otio(&plan, "试片");
        let json = serde_json::to_string(&otio).expect("json");
        assert!(json.contains("file:///D:/media/a.mp4"));
        assert!(json.contains("LinearTimeWarp.1"));
        assert!(json.contains("file:///D:/media/vo.wav"));
        assert_eq!(otio["name"], "试片");
    }
}
