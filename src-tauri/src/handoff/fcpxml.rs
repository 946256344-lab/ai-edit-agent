//! 将 HandoffPlan 写成可导入 DaVinci Resolve / Final Cut Pro 的 FCPXML（Premiere Pro 只认 FCP 7 XML，不支持）。
//! 只保证切点和源窗；变速用 timeMap，构图不承诺保真。
//! 文字写成 Basic Title（文字、时间、字体、字号、颜色可编辑；位置与动画不带）；
//! 叠化写成 Cross Dissolve；品牌卡是连接的图片片段（文字不可编辑）。

use super::{file_export_transitions, media_file_url, posix_media_path, HandoffClip, HandoffPlan};

const CROSS_DISSOLVE_UID: &str = "FxPlug:4731E73A-8DAC-4113-9A30-AE85B1761265";
const BASIC_TITLE_UID: &str =
    ".../Titles.localized/Bumper:Opener.localized/Basic Title.localized/Basic Title.moti";

/// 连接片段挂在它开始时刻所在的主线片段下，offset 用父片段的本地时间。
fn anchor_clip(plan: &HandoffPlan, time_ms: i64) -> Option<(usize, i64)> {
    let index = plan
        .clips
        .iter()
        .position(|clip| clip.timeline_start_ms <= time_ms && time_ms < clip.timeline_end_ms)
        .or_else(|| plan.clips.len().checked_sub(1))?;
    let clip = &plan.clips[index];
    Some((index, clip.source_start_ms + (time_ms - clip.timeline_start_ms).max(0)))
}

fn fcp_color(hex: &str) -> String {
    let hex = hex.trim_start_matches('#');
    let channel = |range: std::ops::Range<usize>| {
        hex.get(range)
            .and_then(|value| u8::from_str_radix(value, 16).ok())
            .map_or(1.0, |value| value as f64 / 255.0)
    };
    format!("{:.4} {:.4} {:.4} 1", channel(0..2), channel(2..4), channel(4..6))
}

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

    let transitions = file_export_transitions(plan).collect::<Vec<_>>();
    let dissolve_id = (!transitions.is_empty()).then(|| {
        let id = format!("r{next_id}");
        next_id += 1;
        resources.push_str(&format!(
            "        <effect id=\"{id}\" name=\"Cross Dissolve\" uid=\"{CROSS_DISSOLVE_UID}\"/>
"
        ));
        id
    });
    let mut children = vec![String::new(); plan.clips.len()];
    let text_cues = plan
        .text_tracks
        .iter()
        .filter(|track| track.enabled)
        .enumerate()
        .flat_map(|(lane, track)| track.cues.iter().map(move |cue| (lane as i64 + 1, cue)))
        .collect::<Vec<_>>();
    if !text_cues.is_empty() {
        let title_id = format!("r{next_id}");
        next_id += 1;
        resources.push_str(&format!(
            "        <effect id=\"{title_id}\" name=\"Basic Title\" uid=\"{BASIC_TITLE_UID}\"/>
"
        ));
        let long_edge = plan.canvas.width.max(plan.canvas.height) as f64;
        for (number, (lane, cue)) in text_cues.iter().enumerate() {
            let Some((parent, offset)) = anchor_clip(plan, cue.start_ms) else { continue };
            let style_id = format!("ts{}", number + 1);
            children[parent].push_str(&format!(
                "                            <title ref=\"{title_id}\" lane=\"{lane}\" name=\"{name}\" offset=\"{}\" duration=\"{}\">
                                <text>
                                    <text-style ref=\"{style_id}\">{text}</text-style>
                                </text>
                                <text-style-def id=\"{style_id}\">
                                    <text-style font=\"{}\" fontSize=\"{}\" fontColor=\"{}\" bold=\"{}\" alignment=\"{}\"/>
                                </text-style-def>
                            </title>
",
                xml_time(offset, fps),
                xml_time((cue.end_ms - cue.start_ms).max(1), fps),
                crate::preview::ass_font_name(&cue.style.font_key),
                (cue.style.font_size * long_edge).round().max(8.0),
                fcp_color(&cue.style.color),
                if cue.style.bold { 1 } else { 0 },
                xml_escape(&cue.style.alignment),
                name = xml_escape(&cue.text.chars().take(40).collect::<String>()),
                text = xml_escape(&cue.text),
            ));
        }
    }
    if !plan.graphic_overlays.is_empty() {
        let format_id = format!("r{next_id}");
        next_id += 1;
        resources.push_str(&format!(
            "        <format id=\"{format_id}\" name=\"FFVideoFormatRateUndefined\" width=\"{}\" height=\"{}\"/>
",
            plan.canvas.width * 2,
            plan.canvas.height * 2
        ));
        for (index, graphic) in plan.graphic_overlays.iter().enumerate() {
            let asset_id = format!("r{next_id}");
            next_id += 1;
            let src = xml_escape(&media_file_url(&graphic.png_path));
            resources.push_str(&format!(
                "        <asset id=\"{asset_id}\" name=\"{}\" src=\"{src}\" start=\"0s\" duration=\"0s\" hasVideo=\"1\" format=\"{format_id}\">
            <media-rep kind=\"original-media\" src=\"{src}\"/>
        </asset>
",
                xml_escape(&graphic.template_id)
            ));
            let Some((parent, offset)) = anchor_clip(plan, graphic.start_ms) else { continue };
            children[parent].push_str(&format!(
                "                            <video ref=\"{asset_id}\" lane=\"{}\" name=\"{} (image, text not editable)\" offset=\"{}\" duration=\"{}\" start=\"0s\"/>
",
                20 + index,
                xml_escape(&graphic.template_id),
                xml_time(offset, fps),
                xml_time((graphic.end_ms - graphic.start_ms).max(1), fps),
            ));
        }
    }
    let mut spine = String::new();
    for (index, clip) in plan.clips.iter().enumerate() {
        let ref_id = seen.get(&clip.asset_id).map(String::as_str).unwrap_or("r2");
        spine.push_str(&asset_clip_xml(clip, ref_id, fps, None, &children[index]));
        if let (Some(dissolve_id), Some(transition)) = (
            dissolve_id.as_deref(),
            transitions.iter().find(|item| item.after_clip == index),
        ) {
            spine.push_str(&format!(
                "                        <transition name=\"Cross Dissolve\" offset=\"{}\" duration=\"{}\">
                            <filter-video ref=\"{dissolve_id}\" name=\"Cross Dissolve\"/>
                        </transition>
",
                xml_time(transition.cut_ms - transition.duration_ms / 2, fps),
                xml_time(transition.duration_ms, fps),
            ));
        }
    }
    if let Some(first) = plan.clips.first() {
        for track in &plan.voiceover_tracks {
            if !track.enabled {
                continue;
            }
            for cue in &track.cues {
                let Some(ref_id) = seen.get(&cue.cue.asset_id) else {
                    continue;
                };
                spine.push_str(&connected_audio_xml(
                    first,
                    cue.cue.timeline_start_ms,
                    cue.cue.timeline_end_ms,
                    cue.cue.source_start_ms,
                    ref_id,
                    fps,
                    -1,
                ));
            }
        }
        for track in &plan.music_tracks {
            if !track.enabled {
                continue;
            }
            for cue in &track.cues {
                let Some(ref_id) = seen.get(&cue.cue.asset_id) else {
                    continue;
                };
                spine.push_str(&connected_audio_xml(
                    first,
                    cue.cue.timeline_start_ms,
                    cue.cue.timeline_end_ms,
                    cue.cue.source_start_ms,
                    ref_id,
                    fps,
                    -2,
                ));
            }
        }
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

fn asset_clip_xml(
    clip: &HandoffClip,
    ref_id: &str,
    fps: i64,
    lane: Option<i64>,
    connected: &str,
) -> String {
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
        xml_time(timeline_span, fps),
    );
    if (source_span - timeline_span).abs() > 50 {
        xml.push_str(&format!(
            "                            <timeMap>\n                                <timept time=\"0s\" value=\"{}\" interp=\"linear\"/>\n                                <timept time=\"{}\" value=\"{}\" interp=\"linear\"/>\n                            </timeMap>\n",
            xml_time(clip.source_start_ms, fps),
            xml_time(timeline_span, fps),
            xml_time(clip.source_end_ms, fps),
        ));
    }
    xml.push_str(connected);
    xml.push_str("                        </asset-clip>\n");
    xml
}

fn connected_audio_xml(
    _anchor: &HandoffClip,
    timeline_start_ms: i64,
    timeline_end_ms: i64,
    source_start_ms: i64,
    ref_id: &str,
    fps: i64,
    lane: i64,
) -> String {
    let duration = (timeline_end_ms - timeline_start_ms).max(1);
    format!(
        "                        <asset-clip name=\"audio\" ref=\"{ref_id}\" offset=\"{}\" start=\"{}\" duration=\"{}\" lane=\"{lane}\"/>\n",
        xml_time(timeline_start_ms, fps),
        xml_time(source_start_ms, fps),
        xml_time(duration, fps),
    )
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
