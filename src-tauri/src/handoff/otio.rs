//! 将 HandoffPlan 写成 OTIO JSON，供 DaVinci Resolve 导入。
//! 切点和源窗保真；慢放用 LinearTimeWarp；叠化写成 SMPTE_Dissolve；品牌卡是 V2 轨图片；
//! OTIO 没有文字轨，文字写成 V1 上的 marker（名称即文字），需要在 Resolve 里手动加字。

use super::{file_export_transitions, media_file_url, HandoffClip, HandoffPlan};
use serde_json::{json, Value};

pub(super) fn render_otio(plan: &HandoffPlan, project_name: &str) -> Value {
    let rate = plan.canvas.fps.max(1) as f64;
    let mut video_children = Vec::new();
    let transitions = file_export_transitions(plan).collect::<Vec<_>>();
    for (index, clip) in plan.clips.iter().enumerate() {
        video_children.push(otio_clip(clip, rate, true));
        if let Some(transition) = transitions.iter().find(|item| item.after_clip == index) {
            let half = transition.duration_ms / 2;
            video_children.push(json!({
                "OTIO_SCHEMA": "Transition.1",
                "name": "Cross Dissolve",
                "transition_type": "SMPTE_Dissolve",
                "in_offset": rational_time(half, rate),
                "out_offset": rational_time(transition.duration_ms - half, rate),
            }));
        }
    }
    let markers = plan
        .text_tracks
        .iter()
        .filter(|track| track.enabled)
        .flat_map(|track| track.cues.iter().map(move |cue| (track, cue)))
        .map(|(track, cue)| {
            json!({
                "OTIO_SCHEMA": "Marker.2",
                "name": cue.text,
                "comment": cue.text,
                "color": "PURPLE",
                "marked_range": time_range(cue.start_ms, cue.end_ms - cue.start_ms, rate),
                "metadata": {"voycut": {"role": track.role, "kind": "text"}},
            })
        })
        .collect::<Vec<_>>();
    let mut tracks = vec![json!({
        "OTIO_SCHEMA": "Track.1",
        "name": "V1",
        "kind": "Video",
        "children": video_children,
        "markers": markers,
    })];
    // 品牌卡按时间不重叠分轨（角标与信息卡会同时出现），每轨用 Gap 补齐空档。
    let mut graphics = plan.graphic_overlays.iter().collect::<Vec<_>>();
    graphics.sort_by_key(|graphic| graphic.start_ms);
    let mut lanes: Vec<(i64, Vec<Value>)> = Vec::new();
    for graphic in graphics {
        let lane = match lanes.iter().position(|(end, _)| *end <= graphic.start_ms) {
            Some(lane) => lane,
            None => {
                lanes.push((0, Vec::new()));
                lanes.len() - 1
            }
        };
        let (cursor, children) = &mut lanes[lane];
        if graphic.start_ms > *cursor {
            children.push(json!({
                "OTIO_SCHEMA": "Gap.1",
                "name": "gap",
                "source_range": time_range(0, graphic.start_ms - *cursor, rate),
            }));
        }
        let span = graphic.end_ms - graphic.start_ms;
        children.push(json!({
            "OTIO_SCHEMA": "Clip.1",
            "name": format!("{} (image, text not editable)", graphic.template_id),
            "source_range": time_range(0, span, rate),
            "media_reference": {
                "OTIO_SCHEMA": "ExternalReference.1",
                "name": graphic.template_id,
                "target_url": media_file_url(&graphic.png_path),
                "available_range": time_range(0, span, rate),
            },
        }));
        *cursor = graphic.end_ms;
    }
    for (index, (_, children)) in lanes.into_iter().enumerate() {
        tracks.push(json!({
            "OTIO_SCHEMA": "Track.1",
            "name": format!("V{} brand cards", index + 2),
            "kind": "Video",
            "children": children,
        }));
    }
    // 旁白与音乐各占一条音轨：OTIO 轨内子项首尾相接，起点靠 Gap 补齐，否则音乐会排在旁白后面。
    let voice = plan
        .voiceover_tracks
        .iter()
        .filter(|track| track.enabled)
        .flat_map(|track| track.cues.iter())
        .filter_map(|cue| {
            cue.source_reference.as_deref().map(|path| AudioCue {
                path,
                source_start_ms: cue.cue.source_start_ms,
                source_end_ms: cue.cue.source_end_ms,
                timeline_start_ms: cue.cue.timeline_start_ms,
                timeline_end_ms: cue.cue.timeline_end_ms,
                loop_enabled: false,
            })
        })
        .collect::<Vec<_>>();
    let music = plan
        .music_tracks
        .iter()
        .filter(|track| track.enabled)
        .flat_map(|track| track.cues.iter())
        .map(|cue| AudioCue {
            path: &cue.source_reference,
            source_start_ms: cue.cue.source_start_ms,
            source_end_ms: cue.cue.source_end_ms,
            timeline_start_ms: cue.cue.timeline_start_ms,
            timeline_end_ms: cue.cue.timeline_end_ms,
            loop_enabled: cue.cue.loop_enabled,
        })
        .collect::<Vec<_>>();
    for (name, label, cues) in [("A1", "voiceover", voice), ("A2", "music", music)] {
        if cues.is_empty() {
            continue;
        }
        tracks.push(json!({
            "OTIO_SCHEMA": "Track.1",
            "name": name,
            "kind": "Audio",
            "children": audio_track_children(label, cues, rate),
        }));
    }    json!({
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
        "source_range": span_range(clip.source_start_ms, clip.timeline_start_ms, clip.timeline_end_ms, rate),
        "media_reference": {
            "OTIO_SCHEMA": "ExternalReference.1",
            "name": clip.asset_id,
            "target_url": media_file_url(&clip.source_reference),
            "available_range": time_range(0, clip.source_end_ms.max(source_span), rate),
        },
    });
    if with_warp && (source_span - timeline_span).abs() > 50 {
        item["source_range"] = time_range(clip.source_start_ms, source_span, rate);
        item["effects"] = json!([{
            "OTIO_SCHEMA": "LinearTimeWarp.1",
            "name": "Speed",
            "time_scalar": source_span as f64 / timeline_span as f64,
        }]);
    }
    item
}

struct AudioCue<'a> {
    path: &'a str,
    source_start_ms: i64,
    source_end_ms: i64,
    timeline_start_ms: i64,
    timeline_end_ms: i64,
    loop_enabled: bool,
}

/// 按时间线起点排好，前面空出的地方放 Gap；循环音乐按源区间重复铺满，不用变速凑长度。
fn audio_track_children(name: &str, mut cues: Vec<AudioCue<'_>>, rate: f64) -> Vec<Value> {
    cues.sort_by_key(|cue| cue.timeline_start_ms);
    let mut children = Vec::new();
    let mut cursor = 0_i64;
    for cue in cues {
        if cue.timeline_start_ms > cursor {
            children.push(json!({
                "OTIO_SCHEMA": "Gap.1",
                "name": "gap",
                "source_range": span_range(0, cursor, cue.timeline_start_ms, rate),
            }));
        }
        let source_span = (cue.source_end_ms - cue.source_start_ms).max(1);
        let mut start = cue.timeline_start_ms.max(cursor);
        while start < cue.timeline_end_ms {
            let piece = if cue.loop_enabled {
                source_span.min(cue.timeline_end_ms - start)
            } else {
                cue.timeline_end_ms - start
            };
            let end = start + piece;
            let mut item = json!({
                "OTIO_SCHEMA": "Clip.1",
                "name": name,
                "source_range": span_range(cue.source_start_ms, start, end, rate),
                "media_reference": {
                    "OTIO_SCHEMA": "ExternalReference.1",
                    "name": name,
                    "target_url": media_file_url(cue.path),
                    "available_range": time_range(0, cue.source_end_ms.max(source_span), rate),
                },
            });
            if !cue.loop_enabled && (source_span - piece).abs() > 50 {
                item["source_range"] = time_range(cue.source_start_ms, source_span, rate);
                item["effects"] = json!([{
                    "OTIO_SCHEMA": "LinearTimeWarp.1",
                    "name": "Speed",
                    "time_scalar": source_span as f64 / piece as f64,
                }]);
            }
            children.push(item);
            start = end;
            if !cue.loop_enabled {
                break;
            }
        }
        cursor = cue.timeline_end_ms.max(cursor);
    }
    children
}

/// 源起点 + 按时间线绝对帧位相减得到的时长：逐段取整不累积漂移，音乐与画面切点保持对齐。
fn span_range(source_start_ms: i64, timeline_start_ms: i64, timeline_end_ms: i64, rate: f64) -> Value {
    let frame = |ms: i64| ((ms.max(0) as f64) * rate / 1000.0).round();
    json!({
        "OTIO_SCHEMA": "TimeRange.1",
        "start_time": rational_time(source_start_ms, rate),
        "duration": {
            "OTIO_SCHEMA": "RationalTime.1",
            "rate": rate,
            "value": (frame(timeline_end_ms) - frame(timeline_start_ms)).max(1.0),
        },
    })
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
        let plan = build_handoff_plan(&sample_timeline(), &sample_sources(), crate::media_options::AspectRatio::Portrait.canvas()).expect("plan");
        let otio = render_otio(&plan, "试片");
        let json = serde_json::to_string(&otio).expect("json");
        assert!(json.contains("file:///D:/media/a.mp4"));
        assert!(json.contains("LinearTimeWarp.1"));
        assert!(json.contains("file:///D:/media/vo.wav"));
        assert_eq!(otio["name"], "试片");
    }
}
