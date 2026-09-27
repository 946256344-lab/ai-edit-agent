//! 预览里的镜头转场与品牌图层：转场以切点为中心、不改总时长，品牌卡 PNG 按时间淡入淡出叠加。
//! 转场两侧需要的余量优先取源素材，源素材不够时冻结首 / 尾帧补齐（只影响本地预览）。
use crate::models::{GraphicOverlay, TimelineClip};
use crate::process::hidden_command;
use crate::timeline_graphics::ResolvedTransition;
use std::path::{Path, PathBuf};

/// 某个镜头为转场渲染时的扩展：新的源 / 时间区间，以及源素材不够时要冻结补齐的毫秒数。
pub(crate) struct ClipHandles {
    pub clip: TimelineClip,
    pub freeze_head_ms: i64,
    pub freeze_tail_ms: i64,
}

/// 每个镜头前后各需要多少余量：前一刀转场的一半、后一刀转场的一半。
pub(crate) fn handle_needs(clip_count: usize, transitions: &[ResolvedTransition]) -> Vec<(i64, i64)> {
    let mut needs = vec![(0_i64, 0_i64); clip_count];
    for transition in transitions {
        let half = transition.duration_ms / 2;
        needs[transition.after_clip].1 = half;
        if let Some(next) = needs.get_mut(transition.after_clip + 1) {
            next.0 = transition.duration_ms - half;
        }
    }
    needs
}

/// 按源素材余量扩展镜头；变速镜头按同一速度取余量。源素材不够时整段余量改为冻结补齐，镜头本身速度不变。
pub(crate) fn extend_clip(
    clip: &TimelineClip,
    kind: &str,
    head_ms: i64,
    tail_ms: i64,
    file_duration_ms: Option<i64>,
) -> ClipHandles {
    if head_ms == 0 && tail_ms == 0 {
        return ClipHandles { clip: clip.clone(), freeze_head_ms: 0, freeze_tail_ms: 0 };
    }
    let slot = (clip.timeline_end_ms - clip.timeline_start_ms).max(1);
    let source = clip.source_end_ms - clip.source_start_ms;
    let mut extended = clip.clone();
    // 图片与定格镜头的「源」就是同一帧，直接加长时长即可。
    if kind != "video" || clip.clip_kind == "freeze_frame" || source <= 0 {
        extended.timeline_start_ms -= head_ms;
        extended.timeline_end_ms += tail_ms;
        return ClipHandles { clip: extended, freeze_head_ms: 0, freeze_tail_ms: 0 };
    }
    let speed = source as f64 / slot as f64;
    let source_head = (head_ms as f64 * speed).round() as i64;
    let source_tail = (tail_ms as f64 * speed).round() as i64;
    let head_fits = clip.source_start_ms - source_head >= 0;
    let tail_fits = file_duration_ms.is_some_and(|duration| clip.source_end_ms + source_tail <= duration);
    let (mut freeze_head_ms, mut freeze_tail_ms) = (0, 0);
    if head_fits {
        extended.source_start_ms -= source_head;
        extended.timeline_start_ms -= head_ms;
    } else {
        freeze_head_ms = head_ms;
    }
    if tail_fits {
        extended.source_end_ms += source_tail;
        extended.timeline_end_ms += tail_ms;
    } else {
        freeze_tail_ms = tail_ms;
    }
    ClipHandles { clip: extended, freeze_head_ms, freeze_tail_ms }
}

fn seconds(ms: i64) -> String {
    format!("{:.3}", ms.max(0) as f64 / 1000.0)
}

/// 用 xfade（叠化 fade、黑场过渡 fadeblack）串接已渲染镜头；无转场的切点用 concat 硬接。
/// lengths_ms 是各镜头实际渲染长度（含余量，不含冻结补齐）。
pub(crate) fn assemble_with_transitions(
    rendered: &[PathBuf],
    lengths_ms: &[i64],
    handles: &[(i64, i64)],
    transitions: &[ResolvedTransition],
    out: &Path,
) -> Result<(), String> {
    let mut command = hidden_command("ffmpeg");
    command.args(["-y", "-hide_banner", "-loglevel", "error"]);
    for path in rendered {
        command.arg("-i").arg(path);
    }
    let mut parts = Vec::new();
    let mut lengths = Vec::with_capacity(rendered.len());
    for (index, (length, (freeze_head, freeze_tail))) in lengths_ms.iter().zip(handles).enumerate() {
        let mut chain = format!("[{index}:v]settb=AVTB,setpts=PTS-STARTPTS");
        if *freeze_head > 0 || *freeze_tail > 0 {
            chain.push_str(&format!(
                ",tpad=start_mode=clone:start_duration={}:stop_mode=clone:stop_duration={}",
                seconds(*freeze_head),
                seconds(*freeze_tail)
            ));
        }
        chain.push_str(&format!(",fps=30,format=yuv420p[c{index}]"));
        parts.push(chain);
        lengths.push(length + freeze_head + freeze_tail);
    }
    let mut current = "c0".to_owned();
    let mut current_ms = lengths[0];
    for index in 1..rendered.len() {
        let label = format!("x{index}");
        match transitions.iter().find(|transition| transition.after_clip + 1 == index) {
            Some(transition) => {
                let effect = if transition.kind == "dip_to_black" { "fadeblack" } else { "fade" };
                parts.push(format!(
                    "[{current}][c{index}]xfade=transition={effect}:duration={}:offset={}[{label}]",
                    seconds(transition.duration_ms),
                    seconds(current_ms - transition.duration_ms)
                ));
                current_ms += lengths[index] - transition.duration_ms;
            }
            None => {
                parts.push(format!("[{current}][c{index}]concat=n=2:v=1:a=0[{label}]"));
                current_ms += lengths[index];
            }
        }
        current = label;
    }
    let status = command
        .args(["-filter_complex", &parts.join(";"), "-map", &format!("[{current}]")])
        .args(["-an", "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
        .arg(out)
        .status()
        .map_err(|_| "FFmpeg is not available on this computer.".to_owned())?;
    if status.success() {
        Ok(())
    } else {
        Err("FFmpeg could not join shots with transitions.".to_owned())
    }
}

/// 品牌卡作为全画布透明 PNG 输入：只在自己的时间段出现，带 alpha 淡入淡出。
pub(crate) struct GraphicInput<'a> {
    pub png: PathBuf,
    pub overlay: &'a GraphicOverlay,
}

/// 每张卡的 ffmpeg 输入参数，由调用方接到自己的命令后面。
pub(crate) fn graphic_input_args(graphics: &[GraphicInput]) -> Vec<std::ffi::OsString> {
    let mut args = Vec::new();
    for graphic in graphics {
        for arg in ["-loop", "1", "-framerate", "30", "-t"] {
            args.push(arg.into());
        }
        args.push(seconds(graphic.overlay.end_ms).into());
        args.push("-i".into());
        args.push(graphic.png.clone().into_os_string());
    }
    args
}

/// 追加叠加滤镜，返回最后一层的标签。first_input 是第一张 PNG 的输入序号。
pub(crate) fn graphic_filters(
    parts: &mut Vec<String>,
    mut last: String,
    graphics: &[GraphicInput],
    first_input: usize,
    width: i64,
    height: i64,
) -> String {
    for (index, graphic) in graphics.iter().enumerate() {
        let overlay = graphic.overlay;
        let mut chain = format!("[{}:v]scale={width}:{height}:flags=lanczos,format=rgba", first_input + index);
        if overlay.fade_in_ms > 0 {
            chain.push_str(&format!(
                ",fade=t=in:st={}:d={}:alpha=1",
                seconds(overlay.start_ms),
                seconds(overlay.fade_in_ms)
            ));
        }
        if overlay.fade_out_ms > 0 {
            chain.push_str(&format!(
                ",fade=t=out:st={}:d={}:alpha=1",
                seconds(overlay.end_ms - overlay.fade_out_ms),
                seconds(overlay.fade_out_ms)
            ));
        }
        parts.push(format!("{chain}[g{index}]"));
        parts.push(format!(
            "[{last}][g{index}]overlay=0:0:eof_action=pass:enable='between(t,{},{})'[gl{index}]",
            seconds(overlay.start_ms),
            seconds(overlay.end_ms)
        ));
        last = format!("gl{index}");
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_come_from_source_when_it_has_room_and_freeze_otherwise() {
        let clip = TimelineClip {
            source_start_ms: 1_000,
            source_end_ms: 5_000,
            timeline_start_ms: 0,
            timeline_end_ms: 2_000,
            ..TimelineClip::default()
        };
        let roomy = extend_clip(&clip, "video", 150, 150, Some(10_000));
        assert_eq!((roomy.clip.source_start_ms, roomy.clip.source_end_ms), (700, 5_300));
        assert_eq!((roomy.clip.timeline_start_ms, roomy.clip.timeline_end_ms), (-150, 2_150));
        let tight = extend_clip(&clip, "video", 150, 150, Some(5_100));
        assert_eq!((tight.freeze_head_ms, tight.freeze_tail_ms), (0, 150));
        assert_eq!(tight.clip.source_end_ms, 5_000);
        let needs = handle_needs(3, &[ResolvedTransition { after_clip: 1, kind: "crossfade".to_owned(), duration_ms: 301, cut_ms: 4_000 }]);
        assert_eq!(needs, [(0, 0), (0, 150), (151, 0)]);
    }
}
