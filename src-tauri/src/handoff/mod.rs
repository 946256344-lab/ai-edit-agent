//! 编辑器无关交接计划。内部时间线仍是事实源；链接器只消费本结构并写出目标格式。
//! 不覆盖已有工程，也不反向同步编辑器内的改动。

pub mod deliver;
mod fcpxml;
mod otio;

use crate::models::{
    MusicCue, MusicTrack, TextTrack, TimelineClip, TimelineVersion, VoiceoverCue, VoiceoverTrack,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub(crate) const HANDOFF_FORMAT_VERSION: i64 = 1;
const JIANYING_ADAPTER_FORMAT_VERSION: i64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorId {
    Jianying,
    CapCut,
    Fcpxml,
    Otio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeliveryKind {
    DropInDraft,
    ImportFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Support {
    Full,
    Restricted,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EditorCapabilities {
    pub editor_id: EditorId,
    pub implemented: bool,
    pub delivery: DeliveryKind,
    pub video_cuts: Support,
    pub speed_change: Support,
    pub crop_focus: Support,
    pub overlays: Support,
    pub music: Support,
    pub voiceover: Support,
    pub text: Support,
    pub images: Support,
}

impl EditorId {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Jianying => "jianying",
            Self::CapCut => "capcut",
            Self::Fcpxml => "fcpxml",
            Self::Otio => "otio",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Jianying => "剪映",
            Self::CapCut => "CapCut",
            Self::Fcpxml => "Premiere / Resolve / Final Cut",
            Self::Otio => "DaVinci Resolve (OTIO)",
        }
    }

    pub(crate) fn summary(self) -> &'static str {
        match self {
            Self::Jianying => "写入本机剪映草稿箱，可继续微调",
            Self::CapCut => "写入本机 CapCut 草稿箱，可继续微调",
            Self::Fcpxml => "写出 FCPXML，可导入 Premiere、DaVinci Resolve 或 Final Cut",
            Self::Otio => "写出 OTIO，Resolve 可直接导入",
        }
    }

    pub(crate) fn implemented(self) -> bool {
        editor_capabilities(self).implemented
    }

    pub(crate) fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "jianying" => Ok(Self::Jianying),
            "capcut" => Ok(Self::CapCut),
            "fcpxml" | "premiere" | "finalcut" => Ok(Self::Fcpxml),
            "otio" | "resolve" => Ok(Self::Otio),
            _ => Err("未知的输出编辑器。".to_owned()),
        }
    }
}

pub(crate) fn all_editor_ids() -> [EditorId; 4] {
    [
        EditorId::Jianying,
        EditorId::CapCut,
        EditorId::Fcpxml,
        EditorId::Otio,
    ]
}

pub(crate) fn editor_capabilities(id: EditorId) -> EditorCapabilities {
    match id {
        EditorId::Jianying => EditorCapabilities {
            editor_id: id,
            implemented: true,
            delivery: DeliveryKind::DropInDraft,
            video_cuts: Support::Full,
            speed_change: Support::Full,
            crop_focus: Support::Full,
            overlays: Support::Full,
            music: Support::Restricted,
            voiceover: Support::Unsupported,
            text: Support::Restricted,
            images: Support::Unsupported,
        },
        EditorId::CapCut => EditorCapabilities {
            editor_id: id,
            implemented: true,
            delivery: DeliveryKind::DropInDraft,
            video_cuts: Support::Full,
            speed_change: Support::Full,
            crop_focus: Support::Full,
            overlays: Support::Full,
            music: Support::Restricted,
            voiceover: Support::Unsupported,
            text: Support::Restricted,
            images: Support::Unsupported,
        },
        EditorId::Fcpxml | EditorId::Otio => EditorCapabilities {
            editor_id: id,
            implemented: true,
            delivery: DeliveryKind::ImportFile,
            video_cuts: Support::Full,
            speed_change: Support::Restricted,
            crop_focus: Support::Restricted,
            overlays: Support::Restricted,
            music: Support::Full,
            voiceover: Support::Full,
            text: Support::Unsupported,
            images: Support::Restricted,
        },
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HandoffSource {
    pub kind: String,
    pub path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffCanvas {
    pub width: i64,
    pub height: i64,
    pub fps: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffClip {
    pub asset_id: String,
    pub shot_index: i64,
    pub kind: String,
    pub source_reference: String,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub timeline_start_ms: i64,
    pub timeline_end_ms: i64,
    pub crop_focus: Option<[f64; 2]>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffMusicCue {
    #[serde(flatten)]
    pub cue: MusicCue,
    pub source_reference: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffMusicTrack {
    pub id: String,
    pub enabled: bool,
    pub cues: Vec<HandoffMusicCue>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffVoiceoverCue {
    #[serde(flatten)]
    pub cue: VoiceoverCue,
    pub source_reference: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffVoiceoverTrack {
    pub id: String,
    pub enabled: bool,
    pub cues: Vec<HandoffVoiceoverCue>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffPlan {
    pub format_version: i64,
    pub timeline_version_id: String,
    pub project_id: String,
    pub duration_ms: i64,
    pub canvas: HandoffCanvas,
    pub clips: Vec<HandoffClip>,
    pub overlay_clips: Vec<HandoffClip>,
    pub text_tracks: Vec<TextTrack>,
    pub music_tracks: Vec<HandoffMusicTrack>,
    pub voiceover_tracks: Vec<HandoffVoiceoverTrack>,
}

pub(crate) struct JianyingDraftDestination {
    pub draft_root: String,
    pub draft_name: String,
    pub draft_registry_path: String,
}

pub(crate) fn posix_media_path(path: &str) -> String {
    path.replace('\\', "/")
}

pub(super) fn media_file_url(path: &str) -> String {
    let posix = posix_media_path(path);
    if posix.starts_with('/') {
        format!("file://{posix}")
    } else {
        format!("file:///{posix}")
    }
}

pub(crate) fn build_handoff_plan(
    timeline: &TimelineVersion,
    sources: &HashMap<String, HandoffSource>,
) -> Result<HandoffPlan, String> {
    Ok(HandoffPlan {
        format_version: HANDOFF_FORMAT_VERSION,
        timeline_version_id: timeline.id.clone(),
        project_id: timeline.project_id.clone(),
        duration_ms: timeline
            .clips
            .iter()
            .chain(timeline.overlay_clips.iter())
            .map(|clip| clip.timeline_end_ms)
            .max()
            .unwrap_or(0),
        canvas: HandoffCanvas {
            width: 540,
            height: 960,
            fps: 30,
        },
        clips: map_clips(&timeline.clips, sources)?,
        overlay_clips: map_clips(&timeline.overlay_clips, sources)?,
        text_tracks: timeline.text_tracks.clone(),
        music_tracks: map_music_tracks(&timeline.music_tracks, sources)?,
        voiceover_tracks: map_voiceover_tracks(&timeline.voiceover_tracks, sources),
    })
}

/// 剪映适配器 JSON。能力表里旁白为 Unsupported 时不写入草稿，避免改变已交付行为。
pub(crate) fn jianying_create_draft_input(
    plan: &HandoffPlan,
    dest: &JianyingDraftDestination,
) -> Value {
    let caps = editor_capabilities(EditorId::Jianying);
    let mut payload = json!({
        "inputFormatVersion": JIANYING_ADAPTER_FORMAT_VERSION,
        "operation": "createDraft",
        "editor": "jianying",
        "draftRoot": dest.draft_root,
        "draftName": dest.draft_name,
        "draftRegistryPath": dest.draft_registry_path,
        "clips": plan.clips.iter().map(jianying_clip_json).collect::<Vec<_>>(),
        "overlayClips": plan.overlay_clips.iter().map(jianying_clip_json).collect::<Vec<_>>(),
        "textTracks": plan.text_tracks,
        "musicTracks": plan.music_tracks,
    });
    if caps.voiceover != Support::Unsupported {
        payload["voiceoverTracks"] = json!(plan.voiceover_tracks);
    }
    payload
}

pub(crate) fn capcut_create_draft_input(
    plan: &HandoffPlan,
    dest: &JianyingDraftDestination,
) -> Value {
    let mut payload = jianying_create_draft_input(plan, dest);
    payload["editor"] = json!("capcut");
    payload
}

fn map_clips(
    clips: &[TimelineClip],
    sources: &HashMap<String, HandoffSource>,
) -> Result<Vec<HandoffClip>, String> {
    clips
        .iter()
        .map(|clip| {
            let source = require_source(sources, &clip.asset_id)?;
            Ok(HandoffClip {
                asset_id: clip.asset_id.clone(),
                shot_index: clip.shot_index,
                kind: source.kind.clone(),
                source_reference: source.path.clone(),
                source_start_ms: clip.source_start_ms,
                source_end_ms: clip.source_end_ms,
                timeline_start_ms: clip.timeline_start_ms,
                timeline_end_ms: clip.timeline_end_ms,
                crop_focus: clip.crop_focus,
            })
        })
        .collect()
}

fn map_music_tracks(
    tracks: &[MusicTrack],
    sources: &HashMap<String, HandoffSource>,
) -> Result<Vec<HandoffMusicTrack>, String> {
    tracks
        .iter()
        .map(|track| {
            let cues = track
                .cues
                .iter()
                .map(|cue| {
                    let source = require_source(sources, &cue.asset_id)?;
                    Ok(HandoffMusicCue {
                        cue: cue.clone(),
                        source_reference: source.path.clone(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(HandoffMusicTrack {
                id: track.id.clone(),
                enabled: track.enabled,
                cues,
            })
        })
        .collect()
}

fn map_voiceover_tracks(
    tracks: &[VoiceoverTrack],
    sources: &HashMap<String, HandoffSource>,
) -> Vec<HandoffVoiceoverTrack> {
    tracks
        .iter()
        .map(|track| HandoffVoiceoverTrack {
            id: track.id.clone(),
            enabled: track.enabled,
            cues: track
                .cues
                .iter()
                .map(|cue| HandoffVoiceoverCue {
                    cue: cue.clone(),
                    source_reference: sources.get(&cue.asset_id).map(|source| source.path.clone()),
                })
                .collect(),
        })
        .collect()
}

fn require_source<'a>(
    sources: &'a HashMap<String, HandoffSource>,
    asset_id: &str,
) -> Result<&'a HandoffSource, String> {
    sources
        .get(asset_id)
        .ok_or_else(|| "Timeline references an unavailable asset.".to_owned())
}

fn jianying_clip_json(clip: &HandoffClip) -> Value {
    json!({
        "sourceReference": clip.source_reference,
        "sourceStartMs": clip.source_start_ms,
        "sourceEndMs": clip.source_end_ms,
        "timelineStartMs": clip.timeline_start_ms,
        "timelineEndMs": clip.timeline_end_ms,
        "cropFocus": clip.crop_focus,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::models::{TextCue, TextLayout, TextStyle};

    fn source(path: &str, kind: &str) -> HandoffSource {
        HandoffSource {
            kind: kind.to_owned(),
            path: posix_media_path(path),
        }
    }

    fn clip(asset_id: &str, source_end_ms: i64, timeline_end_ms: i64) -> TimelineClip {
        TimelineClip {
            asset_id: asset_id.to_owned(),
            shot_index: 0,
            source_start_ms: 1_000,
            source_end_ms,
            timeline_start_ms: 0,
            timeline_end_ms,
            crop_focus: Some([0.4, 0.6]),
            ..TimelineClip::default()
        }
    }

    pub(crate) fn sample_timeline() -> TimelineVersion {
        TimelineVersion {
            id: "timeline-1".to_owned(),
            project_id: "project-1".to_owned(),
            storyboard_version_id: "storyboard-1".to_owned(),
            version_number: 1,
            clips: vec![clip("video-1", 2_000, 4_000)],
            text_tracks: vec![TextTrack {
                id: "text-1".to_owned(),
                role: "subtitle".to_owned(),
                layer: 1,
                enabled: true,
                origin: "storyboard_generated".to_owned(),
                generation_id: None,
                editable: true,
                locked: false,
                cues: vec![TextCue {
                    id: "cue-1".to_owned(),
                    template_id: None,
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "字幕".to_owned(),
                    style: TextStyle::default(),
                    layout: TextLayout::default(),
                    entrance: None,
                    exit: None,
                    loop_animation: None,
                    jianying_compatibility: "verified".to_owned(),
                }],
            }],
            music_tracks: vec![MusicTrack {
                id: "music-1".to_owned(),
                enabled: true,
                cues: vec![MusicCue {
                    id: "music-cue-1".to_owned(),
                    asset_id: "audio-1".to_owned(),
                    source_start_ms: 0,
                    source_end_ms: 1_000,
                    timeline_start_ms: 0,
                    timeline_end_ms: 4_000,
                    loop_enabled: true,
                    volume: 0.15,
                    fade_in_ms: 0,
                    fade_out_ms: 0,
                    jianying_compatibility: "verified".to_owned(),
                    provider: None,
                    license_url: None,
                    attribution: None,
                }],
            }],
            voiceover_tracks: vec![VoiceoverTrack {
                id: "voice-1".to_owned(),
                enabled: true,
                cues: vec![VoiceoverCue {
                    id: "voice-cue-1".to_owned(),
                    asset_id: "voice-asset".to_owned(),
                    generation_id: "gen-1".to_owned(),
                    source_start_ms: 0,
                    source_end_ms: 4_000,
                    timeline_start_ms: 0,
                    timeline_end_ms: 4_000,
                    volume: 1.0,
                    fade_in_ms: 0,
                    fade_out_ms: 0,
                    provider: "fish".to_owned(),
                    voice_id: "v1".to_owned(),
                    voice_name: "voice".to_owned(),
                }],
            }],
            overlay_clips: vec![clip("overlay-1", 1_500, 2_000)],
            quality_report: None,
            created_at: 1,
        }
    }

    pub(crate) fn sample_sources() -> HashMap<String, HandoffSource> {
        let mut sources = HashMap::new();
        sources.insert("video-1".to_owned(), source(r"D:\media\a.mp4", "video"));
        sources.insert("overlay-1".to_owned(), source(r"D:\media\b.mp4", "video"));
        sources.insert("audio-1".to_owned(), source(r"D:\media\m.mp3", "audio"));
        sources.insert("voice-asset".to_owned(), source(r"D:\media\vo.wav", "audio"));
        sources
    }

    #[test]
    fn catalog_marks_implemented_editors() {
        assert!(EditorId::Jianying.implemented());
        assert!(EditorId::Fcpxml.implemented());
        assert!(EditorId::Otio.implemented());
        assert!(EditorId::CapCut.implemented());
        assert_eq!(EditorId::parse("premiere").unwrap(), EditorId::Fcpxml);
        assert_eq!(
            editor_capabilities(EditorId::Jianying).voiceover,
            Support::Unsupported
        );
        assert_eq!(
            editor_capabilities(EditorId::Fcpxml).delivery,
            DeliveryKind::ImportFile
        );
    }

    #[test]
    fn plan_keeps_source_window_and_voiceover() {
        let plan = build_handoff_plan(&sample_timeline(), &sample_sources()).expect("plan");
        assert_eq!(plan.clips[0].source_end_ms, 2_000);
        assert_eq!(plan.clips[0].timeline_end_ms, 4_000);
        assert_eq!(plan.clips[0].kind, "video");
        assert_eq!(plan.clips[0].source_reference, "D:/media/a.mp4");
        assert_eq!(plan.voiceover_tracks.len(), 1);
        assert_eq!(
            plan.voiceover_tracks[0].cues[0].source_reference.as_deref(),
            Some("D:/media/vo.wav")
        );
        assert_eq!(plan.duration_ms, 4_000);
    }

    #[test]
    fn jianying_payload_includes_source_end_and_omits_voiceover() {
        let plan = build_handoff_plan(&sample_timeline(), &sample_sources()).expect("plan");
        let payload = jianying_create_draft_input(
            &plan,
            &JianyingDraftDestination {
                draft_root: "C:/drafts".to_owned(),
                draft_name: "demo-1".to_owned(),
                draft_registry_path: "C:/registry.json".to_owned(),
            },
        );
        assert_eq!(payload["inputFormatVersion"], 2);
        assert_eq!(payload["operation"], "createDraft");
        assert_eq!(payload["editor"], "jianying");
        assert_eq!(payload["clips"][0]["sourceEndMs"], 2_000);
        assert_eq!(payload["overlayClips"][0]["sourceEndMs"], 1_500);
        assert_eq!(payload["textTracks"][0]["cues"][0]["text"], "字幕");
        assert_eq!(
            payload["musicTracks"][0]["cues"][0]["sourceReference"],
            "D:/media/m.mp3"
        );
        assert!(payload.get("voiceoverTracks").is_none());
        assert!(payload["clips"][0].get("assetId").is_none());
        let capcut = capcut_create_draft_input(
            &plan,
            &JianyingDraftDestination {
                draft_root: "C:/drafts".to_owned(),
                draft_name: "demo-1".to_owned(),
                draft_registry_path: "C:/registry.json".to_owned(),
            },
        );
        assert_eq!(capcut["editor"], "capcut");
        assert_eq!(capcut["clips"][0]["sourceEndMs"], 2_000);
    }
}
