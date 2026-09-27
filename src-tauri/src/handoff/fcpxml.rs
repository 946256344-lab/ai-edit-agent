//! 将 HandoffPlan 写成可导入 DaVinci Resolve / Final Cut Pro 的 FCPXML（Premiere Pro 只认 FCP 7 XML，不支持）。
//! 只保证切点和源窗；变速用 timeMap，构图和字幕不承诺保真。

use super::{media_file_url, posix_media_path, HandoffClip, HandoffPlan};

pub(super) fn render_fcpxml(plan: &HandoffPlan, project_name: &str) -> String {
    let fps = plan.canvas.fps.max(1);
    let mut resources = String::new();
    let mut next_id = 2;
    resources.push_str(&format!(
        "        <format id=\"r1\" name=\"FFVideoFormat{}x{}p{}\" frameDuration=\"100/{}s\" width=\"{}\" height=\"{}\"/>\n",
        plan.canvas.width,
        plan.canvas.height,
        fps,
        fps * 100,
        plan.canvas.width,
        plan.canvas.height
    ));
    let mut seen = std::collections::HashMap::new();
    for clip in plan.clips.iter().chain(plan.overlay_clips.iter()) {
        if seen.contains_key(&clip.asset_id) {
            continue;
        }
        let id = format!("r{next_id}");
        next_id += 1;
        seen.insert(clip.asset_id.clone(), id.clone());
        resources.push_str(&asset_xml(&id, clip, true, fps));
    }
    for track in &plan.music_tracks {
        for cue in &track.cues {
            if seen.contains_key(&cue.cue.asset_id) {
                continue;
            }
            let id = format!("r{next_id}");
            next_id += 1;
            seen.insert(cue.cue.asset_id.clone(), id.clone());
            resources.push_str(&audio_asset_xml(
                &id,
                &cue.cue.asset_id,
                &cue.source_reference,
                cue.cue.source_end_ms,
                fps,
            ));
        }
    }
    for track in &plan.voiceover_tracks {
        for cue in &track.cues {
            let Some(path) = cue.source_reference.as_deref() else {
                continue;
            };
            if seen.contains_key(&cue.cue.asset_id) {
                continue;
            }
            let id = format!("r{next_id}");
            next_id += 1;
            seen.insert(cue.cue.asset_id.clone(), id.clone());
            resources.push_str(&audio_asset_xml(
                &id,
                &cue.cue.asset_id,
                path,
                cue.cue.source_end_ms,
                fps,
            ));
        }
    }

    // 旁白和音乐是挂在第一镜下的连接片段（FCPXML 规定 lane 只能出现在锚定片段上），
    // offset 按父片段的本地时间（从它的 start 起算），这样音乐起点与各切点在成片时间上对齐。
    let mut connected = String::new();
    if let Some(first) = plan.clips.first() {
        let voice = plan.voiceover_tracks.iter().filter(|track| track.enabled).flat_map(|track| {
            track.cues.iter().map(|cue| (&cue.cue.asset_id, cue.cue.timeline_start_ms, cue.cue.timeline_end_ms, cue.cue.source_start_ms, -1))
        });
        let music = plan.music_tracks.iter().filter(|track| track.enabled).flat_map(|track| {
            track.cues.iter().map(|cue| (&cue.cue.asset_id, cue.cue.timeline_start_ms, cue.cue.timeline_end_ms, cue.cue.source_start_ms, -2))
        });
        for (asset_id, timeline_start, timeline_end, source_start, lane) in voice.chain(music) {
            let Some(ref_id) = seen.get(asset_id) else {
                continue;
            };
            connected.push_str(&connected_audio_xml(first, timeline_start, timeline_end, source_start, ref_id, fps, lane));
        }
    }
    let mut spine = String::new();
    for (index, clip) in plan.clips.iter().enumerate() {
        let ref_id = seen.get(&clip.asset_id).map(String::as_str).unwrap_or("r2");
        let children = if index == 0 { connected.as_str() } else { "" };
        spine.push_str(&asset_clip_xml(clip, ref_id, fps, None, children));
    }
    let name = xml_escape(project_name);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE fcpxml>
<fcpxml version="1.9">
    <resources>
{resources}    </resources>
    <library>
        <event name="Voycut">
            <project name="{name}">
                <sequence format="r1" tcStart="0s" duration="{}">
                    <spine>
{spine}                    </spine>
                </sequence>
            </project>
        </event>
    </library>
</fcpxml>
"#,
        xml_time(plan.duration_ms, fps)
    )
}

fn asset_xml(id: &str, clip: &HandoffClip, has_video: bool, fps: i64) -> String {
    let src = xml_escape(&media_file_url(&clip.source_reference));
    let name = xml_escape(&posix_media_path(&clip.source_reference));
    let duration = xml_time(clip.source_end_ms.max(clip.source_start_ms + 1), fps);
    format!(
        "        <asset id=\"{id}\" name=\"{name}\" src=\"{src}\" start=\"0s\" duration=\"{duration}\" hasVideo=\"{}\" hasAudio=\"0\">\n            <media-rep kind=\"original-media\" src=\"{src}\"/>\n        </asset>\n",
        if has_video { "1" } else { "0" }
    )
}

fn audio_asset_xml(id: &str, name: &str, path: &str, source_end_ms: i64, fps: i64) -> String {
    let src = xml_escape(&media_file_url(path));
    let duration = xml_time(source_end_ms.max(1), fps);
    format!(
        "        <asset id=\"{id}\" name=\"{}\" src=\"{src}\" start=\"0s\" duration=\"{duration}\" hasVideo=\"0\" hasAudio=\"1\">\n            <media-rep kind=\"original-media\" src=\"{src}\"/>\n        </asset>\n",
        xml_escape(name)
    )
}

fn asset_clip_xml(clip: &HandoffClip, ref_id: &str, fps: i64, lane: Option<i64>, children: &str) -> String {
    let source_span = (clip.source_end_ms - clip.source_start_ms).max(1);
    let timeline_span = (clip.timeline_end_ms - clip.timeline_start_ms).max(1);
    let lane_attr = lane
        .map(|value| format!(" lane=\"{value}\""))
        .unwrap_or_default();
    let mut xml = format!(
        "                        <asset-clip name=\"shot-{}\" ref=\"{ref_id}\" offset=\"{}\" start=\"{}\" duration=\"{}\"{lane_attr}>\n",
        clip.shot_index,
        xml_time(clip.timeline_start_ms, fps),
        xml_time(clip.source_start_ms, fps),
        xml_span(clip.timeline_start_ms, clip.timeline_end_ms, fps),
    );
    if (source_span - timeline_span).abs() > 50 {
        xml.push_str(&format!(
            "                            <timeMap>\n                                <timept time=\"0s\" value=\"{}\" interp=\"linear\"/>\n                                <timept time=\"{}\" value=\"{}\" interp=\"linear\"/>\n                            </timeMap>\n",
            xml_time(clip.source_start_ms, fps),
            xml_time(timeline_span, fps),
            xml_time(clip.source_end_ms, fps),
        ));
    }
    xml.push_str(children);
    xml.push_str("                        </asset-clip>\n");
    xml
}

fn connected_audio_xml(
    anchor: &HandoffClip,
    timeline_start_ms: i64,
    timeline_end_ms: i64,
    source_start_ms: i64,
    ref_id: &str,
    fps: i64,
    lane: i64,
) -> String {
    let offset = anchor.source_start_ms + (timeline_start_ms - anchor.timeline_start_ms);
    format!(
        "                            <asset-clip name=\"audio\" ref=\"{ref_id}\" lane=\"{lane}\" offset=\"{}\" start=\"{}\" duration=\"{}\"/>\n",
        xml_time(offset, fps),
        xml_time(source_start_ms, fps),
        xml_span(timeline_start_ms, timeline_end_ms, fps),
    )
}

/// 按起止点的绝对帧位相减取时长，逐镜取整不累积漂移。
fn xml_span(start_ms: i64, end_ms: i64, fps: i64) -> String {
    let frame = |ms: i64| ((ms.max(0) as f64) * (fps as f64) / 1000.0).round() as i64;
    let frames = (frame(end_ms) - frame(start_ms)).max(1);
    format!("{frames}/{fps}s")
}
pub(super) fn xml_time(ms: i64, fps: i64) -> String {
    let frames = ((ms.max(0) as f64) * (fps as f64) / 1000.0).round() as i64;
    if frames <= 0 {
        "0s".to_owned()
    } else {
        format!("{frames}/{fps}s")
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::{build_handoff_plan, tests::sample_sources, tests::sample_timeline};

    #[test]
    fn fcpxml_keeps_source_window_and_media_url() {
        let plan = build_handoff_plan(&sample_timeline(), &sample_sources(), crate::media_options::AspectRatio::Portrait.canvas()).expect("plan");
        let xml = render_fcpxml(&plan, "工厂试片");
        assert!(xml.contains("file:///D:/media/a.mp4"));
        assert!(xml.contains("start=\"30/30s\""));
        assert!(xml.contains("<timeMap>"));
        assert!(xml.contains("file:///D:/media/vo.wav"));
        assert!(xml.contains("工厂试片") || xml.contains("试片"));
    }
}
