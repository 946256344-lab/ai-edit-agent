//! Studio 工作台持久化：把前端 mash diff 一次性落为新的 timeline version。
//! 与 agentloop 技能复用一套校验，但以 user 身份写 audit，不依赖 agent task。

use crate::db::{now_millis, open_connection};
use crate::models::{TextTrack, TimelineClip, TimelineContent, TimelineVersion};
use crate::timeline::{load_timeline_version, validate_text_tracks};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct StudioDurationAdjustment {
    #[serde(rename = "shotIndex")]
    pub shot_index: i64,
    #[serde(rename = "newDurationMs")]
    pub new_duration_ms: i64,
    #[serde(rename = "newSourceStartMs")]
    pub new_source_start_ms: i64,
}

#[derive(Deserialize)]
pub struct StudioInsertedClip {
    #[serde(rename = "assetId")]
    pub asset_id: String,
    #[serde(rename = "sourceStartMs")]
    pub source_start_ms: i64,
    #[serde(rename = "sourceEndMs")]
    pub source_end_ms: i64,
    #[serde(rename = "timelineStartMs")]
    pub timeline_start_ms: i64,
    #[serde(rename = "timelineEndMs")]
    pub timeline_end_ms: i64,
    #[serde(rename = "onScreenText")]
    pub on_screen_text: String,
    #[serde(rename = "derivedFromShotIndex")]
    pub derived_from_shot_index: Option<i64>,
}

#[derive(Deserialize)]
pub struct StudioOverlayInsertedClip {
    #[serde(rename = "assetId")]
    pub asset_id: String,
    #[serde(rename = "sourceStartMs")]
    pub source_start_ms: i64,
    #[serde(rename = "sourceEndMs")]
    pub source_end_ms: i64,
    #[serde(rename = "timelineStartMs")]
    pub timeline_start_ms: i64,
    #[serde(rename = "timelineEndMs")]
    pub timeline_end_ms: i64,
    #[serde(rename = "onScreenText")]
    pub on_screen_text: String,
}

#[derive(Deserialize)]
pub struct StudioOverlayAdjustment {
    #[serde(rename = "shotIndex")]
    pub shot_index: i64,
    #[serde(rename = "newDurationMs")]
    pub new_duration_ms: i64,
    #[serde(rename = "newSourceStartMs")]
    pub new_source_start_ms: i64,
    #[serde(rename = "newTimelineStartMs")]
    pub new_timeline_start_ms: i64,
}

#[derive(Deserialize)]
pub struct StudioClipReplacement {
    #[serde(rename = "shotIndex")]
    pub shot_index: i64,
    #[serde(rename = "assetId")]
    pub asset_id: String,
    #[serde(rename = "sourceStartMs")]
    pub source_start_ms: i64,
    #[serde(rename = "sourceEndMs")]
    pub source_end_ms: i64,
    #[serde(default, rename = "cropFocus")]
    pub crop_focus: Option<[f64; 2]>,
}

#[derive(Deserialize)]
pub struct StudioAudioTrackPayload {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "enabled")]
    pub enabled: bool,
    #[serde(rename = "cues")]
    pub cues: Vec<crate::models::MusicCue>,
}

#[derive(Deserialize)]
pub struct StudioCommitPayload {
    #[serde(rename = "projectId")]
    pub project_id: String,
    #[serde(rename = "editingTaskId")]
    pub editing_task_id: String,
    #[serde(rename = "timelineVersionId")]
    pub timeline_version_id: String,
    #[serde(default)]
    pub reorder: Option<Vec<i64>>,
    #[serde(default)]
    pub adjustments: Option<Vec<StudioDurationAdjustment>>,
    #[serde(default, rename = "clipReplacements")]
    pub clip_replacements: Option<Vec<StudioClipReplacement>>,
    #[serde(default)]
    pub text_tracks: Option<Vec<TextTrack>>,
    #[serde(default)]
    pub inserted: Option<Vec<StudioInsertedClip>>,
    #[serde(default)]
    pub deleted_shot_indices: Option<Vec<i64>>,
    #[serde(default)]
    pub overlay_inserted: Option<Vec<StudioOverlayInsertedClip>>,
    #[serde(default)]
    pub overlay_deleted_shot_indices: Option<Vec<i64>>,
    #[serde(default)]
    pub overlay_adjustments: Option<Vec<StudioOverlayAdjustment>>,
    #[serde(default)]
    pub overlay_reorder: Option<Vec<i64>>,
    #[serde(default)]
    pub music_tracks: Option<Vec<StudioAudioTrackPayload>>,
    #[serde(default)]
    pub voiceover_tracks: Option<Vec<StudioAudioTrackPayload>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioCommitResult {
    pub timeline: TimelineVersion,
    pub applied: Vec<String>,
}

fn asset_kind_and_duration(
    connection: &Connection,
    project_id: &str,
    asset_id: &str,
) -> Result<(String, Option<i64>), String> {
    let (kind, metadata_json): (String, String) = connection
        .query_row(
            "SELECT kind, metadata_json FROM assets WHERE id = ?1 AND project_id = ?2 AND analysis_status = 'ready'",
            params![asset_id, project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| "素材不存在或尚未完成分析".to_owned())?;
    let v: serde_json::Value =
        serde_json::from_str(&metadata_json).unwrap_or(serde_json::json!({}));
    let duration = v.get("durationMs").and_then(|x| x.as_i64());
    Ok((kind, duration))
}

fn recompute_positions(clips: &mut [TimelineClip]) {
    let mut cursor = 0_i64;
    for clip in clips.iter_mut() {
        let dur = clip.timeline_end_ms - clip.timeline_start_ms;
        clip.timeline_start_ms = cursor;
        clip.timeline_end_ms = cursor + dur;
        cursor += dur;
    }
}

#[tauri::command]
pub fn commit_studio_edits(
    app: AppHandle,
    payload: StudioCommitPayload,
) -> Result<StudioCommitResult, String> {
    let connection = open_connection(&app)?;
    commit_studio_edits_inner(&connection, payload)
}

fn commit_studio_edits_inner(
    connection: &Connection,
    payload: StudioCommitPayload,
) -> Result<StudioCommitResult, String> {
    let base = load_timeline_version(connection, &payload.timeline_version_id)?;
    if base.project_id != payload.project_id {
        return Err("Timeline 不属于该项目".to_owned());
    }
    let task_ok: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM storyboard_versions WHERE id = ?1 AND project_id = ?2 AND editing_task_id = ?3)",
            params![base.storyboard_version_id, payload.project_id, payload.editing_task_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !task_ok {
        return Err("Timeline 不属于当前剪辑任务".to_owned());
    }

    let mut clips = base.clips.clone();
    let mut text_tracks = base.text_tracks.clone();
    let mut applied: Vec<String> = Vec::new();

    // deleted
    if let Some(deleted) = payload.deleted_shot_indices {
        if !deleted.is_empty() {
            if deleted.len() >= clips.len() {
                return Err("至少保留一个镜头".to_owned());
            }
            let existing: std::collections::HashSet<i64> =
                clips.iter().map(|c| c.shot_index).collect();
            for s in &deleted {
                if !existing.contains(s) {
                    return Err(format!("待删除镜头 {s} 不存在"));
                }
            }
            let del_set: std::collections::HashSet<i64> = deleted.into_iter().collect();
            clips.retain(|c| !del_set.contains(&c.shot_index));
            recompute_positions(&mut clips);
            applied.push("delete".to_owned());
        }
    }

    // reorder (only for kept clips)
    if let Some(order) = payload.reorder {
        if order.len() != clips.len() {
            return Err("排序必须包含全部镜头".to_owned());
        }
        let mut seen = std::collections::HashSet::new();
        for s in &order {
            if !seen.insert(*s) {
                return Err(format!("排序中镜头 {s} 重复"));
            }
        }
        let existing: std::collections::HashSet<i64> = clips.iter().map(|c| c.shot_index).collect();
        if order.iter().any(|s| !existing.contains(s)) {
            return Err("排序包含不存在的镜头".to_owned());
        }
        let map: std::collections::HashMap<i64, TimelineClip> =
            clips.into_iter().map(|c| (c.shot_index, c)).collect();
        clips = order
            .iter()
            .map(|s| map.get(s).cloned().expect("existing shot"))
            .collect();
        recompute_positions(&mut clips);
        applied.push("reorder".to_owned());
    }

    // clip replacements (swap asset, reset source window within file)
    if let Some(repls) = payload.clip_replacements {
        if !repls.is_empty() {
            let mut seen = std::collections::HashSet::new();
            for r in &repls {
                if !seen.insert(r.shot_index) {
                    return Err(format!("替换中镜头 {} 重复", r.shot_index));
                }
            }
            for r in &repls {
                let idx = clips
                    .iter()
                    .position(|c| c.shot_index == r.shot_index)
                    .ok_or_else(|| format!("镜头 {} 不存在", r.shot_index))?;
                if r.asset_id.is_empty() {
                    return Err("替换缺少 assetId".to_owned());
                }
                let dur = clips[idx].timeline_end_ms - clips[idx].timeline_start_ms;
                let src_dur = r.source_end_ms - r.source_start_ms;
                if src_dur != dur {
                    return Err(format!("镜头 {} 替换源时长与槽位时长不一致", r.shot_index));
                }
                if r.source_start_ms < 0 {
                    return Err("替换源起点不能为负".to_owned());
                }
                let (kind, file_dur_opt) =
                    asset_kind_and_duration(&connection, &payload.project_id, &r.asset_id)?;
                if kind == "video" {
                    if r.source_end_ms > file_dur_opt.unwrap_or(i64::MAX) {
                        return Err(format!("镜头 {} 替换超出素材时长", r.shot_index));
                    }
                } else if kind == "image" {
                    if r.source_start_ms != 0 || r.source_end_ms != 0 {
                        return Err(format!("镜头 {} 图片源必须为 0", r.shot_index));
                    }
                } else {
                    return Err(format!("镜头 {} 不支持的素材类型", r.shot_index));
                }
                clips[idx].crop_focus = r.crop_focus;
                clips[idx].clip_kind = "source".to_owned();
                clips[idx].fit_reason = None;
                clips[idx].asset_id = r.asset_id.clone();
                clips[idx].source_start_ms = r.source_start_ms;
                clips[idx].source_end_ms = r.source_end_ms;
            }
            applied.push("replace".to_owned());
        }
    }

    // duration adjustments (for kept clips)
    if let Some(adjs) = payload.adjustments {
        if !adjs.is_empty() {
            let mut seen = std::collections::HashSet::new();
            for a in &adjs {
                if !seen.insert(a.shot_index) {
                    return Err(format!("时长调整中镜头 {} 重复", a.shot_index));
                }
            }
            for adj in &adjs {
                let idx = clips
                    .iter()
                    .position(|c| c.shot_index == adj.shot_index)
                    .ok_or_else(|| format!("镜头 {} 不存在", adj.shot_index))?;
                let orig = clips[idx].clone();
                if adj.new_duration_ms < 200 || adj.new_duration_ms > 12000 {
                    return Err("片段时长需在 200–12000ms 之间".to_owned());
                }
                if adj.new_source_start_ms < 0 {
                    return Err("源起点不能为负".to_owned());
                }
                let (kind, duration_opt) =
                    asset_kind_and_duration(&connection, &payload.project_id, &orig.asset_id)?;
                if kind == "video" {
                    let dur = duration_opt.ok_or_else(|| "视频素材无时长".to_owned())?;
                    if adj.new_source_start_ms + adj.new_duration_ms > dur {
                        return Err(format!("镜头 {} 的重定时超出素材时长", adj.shot_index));
                    }
                    // must stay within original verified window extended to file bounds? For Stage 1 allow any within file, since left-handle can move within file
                    clips[idx].source_start_ms = adj.new_source_start_ms;
                    clips[idx].source_end_ms = adj.new_source_start_ms + adj.new_duration_ms;
                } else if kind == "image" {
                    if adj.new_source_start_ms != 0 {
                        return Err("图片片段源起点必须为 0".to_owned());
                    }
                    clips[idx].source_start_ms = 0;
                    clips[idx].source_end_ms = 0;
                } else {
                    return Err("不支持的素材类型".to_owned());
                }
                clips[idx].timeline_end_ms = clips[idx].timeline_start_ms + adj.new_duration_ms;
            }
            recompute_positions(&mut clips);
            applied.push("duration".to_owned());
        }
    }

    // inserted (split / duplicate)
    if let Some(inserted) = payload.inserted {
        if !inserted.is_empty() {
            let mut next_shot_index = clips.iter().map(|c| c.shot_index).max().unwrap_or(-1) + 1;
            for ins in inserted {
                if ins.asset_id.is_empty() {
                    return Err("插入片段缺少 assetId".to_owned());
                }
                let dur = ins.timeline_end_ms - ins.timeline_start_ms;
                if dur < 200 || dur > 12000 {
                    return Err("插入片段时长需在 200–12000ms 之间".to_owned());
                }
                let src_dur = ins.source_end_ms - ins.source_start_ms;
                // For video, source window must equal timeline duration (1:1), for image src must be 0
                let (kind, file_dur_opt) =
                    asset_kind_and_duration(&connection, &payload.project_id, &ins.asset_id)?;
                if kind == "video" {
                    if src_dur != dur {
                        return Err("插入视频片段源时长与时间线时长不一致".to_owned());
                    }
                    if ins.source_start_ms < 0
                        || ins.source_end_ms > file_dur_opt.unwrap_or(i64::MAX)
                    {
                        return Err("插入片段超出素材时长".to_owned());
                    }
                } else if kind == "image" {
                    if ins.source_start_ms != 0 || ins.source_end_ms != 0 {
                        return Err("图片片段源必须为 0".to_owned());
                    }
                }
                let new_clip = TimelineClip {
                    crop_focus: None,
                    shot_index: next_shot_index,
                    asset_id: ins.asset_id,
                    source_start_ms: ins.source_start_ms,
                    source_end_ms: ins.source_end_ms,
                    timeline_start_ms: 0, // will recompute
                    timeline_end_ms: dur, // duration placeholder
                    on_screen_text: ins.on_screen_text,
                    clip_kind: if ins.derived_from_shot_index.is_some() {
                        "derived".to_owned()
                    } else {
                        "source".to_owned()
                    },
                    derived_from_shot_index: ins.derived_from_shot_index,
                    fit_reason: None,
                };
                // Insert after derived parent if found
                if let Some(parent_idx) = ins
                    .derived_from_shot_index
                    .and_then(|p| clips.iter().position(|c| c.shot_index == p))
                {
                    clips.insert(parent_idx + 1, new_clip);
                } else {
                    // Fallback: append in timelineStart order (sorted insert by timelineStartMs)
                    // For Stage 1, just append; recompute will place at end, which matches duplicate-at-end and split-after-parent
                    clips.push(new_clip);
                }
                next_shot_index += 1;
            }
            recompute_positions(&mut clips);
            applied.push("insert".to_owned());
        }
    }

    // text tracks
    if let Some(mut new_tracks) = payload.text_tracks {
        let duration_ms = clips.iter().map(|c| c.timeline_end_ms).max().unwrap_or(0);
        validate_text_tracks(&mut new_tracks, duration_ms)
            .map_err(|e| format!("字幕校验失败: {e}"))?;
        text_tracks = new_tracks;
        applied.push("text".to_owned());
    }

    let mut music_tracks = base.music_tracks.clone();
    let mut voiceover_tracks = base.voiceover_tracks.clone();
    if let Some(new_tracks) = payload.music_tracks {
        for track in &new_tracks {
            if track.cues.iter().any(|c| !(0.0..=2.0).contains(&c.volume)) {
                return Err("音乐音量需在 0–2 之间".to_owned());
            }
        }
        music_tracks = new_tracks
            .into_iter()
            .map(|t| crate::models::MusicTrack {
                id: t.id,
                enabled: t.enabled,
                cues: t.cues,
            })
            .collect();
        applied.push("audio_volume".to_owned());
    }
    if let Some(new_tracks) = payload.voiceover_tracks {
        for track in &new_tracks {
            if track.cues.iter().any(|c| !(0.0..=2.0).contains(&c.volume)) {
                return Err("旁白音量需在 0–2 之间".to_owned());
            }
        }
        // voiceoverTracks uses VoiceoverCue type; MusicCue and VoiceoverCue share volume field but need conversion
        voiceover_tracks = new_tracks
            .into_iter()
            .map(|t| {
                let cues = t
                    .cues
                    .into_iter()
                    .map(|c| crate::models::VoiceoverCue {
                        id: c.id,
                        asset_id: c.asset_id,
                        generation_id: String::new(),
                        source_start_ms: c.source_start_ms,
                        source_end_ms: c.source_end_ms,
                        timeline_start_ms: c.timeline_start_ms,
                        timeline_end_ms: c.timeline_end_ms,
                        volume: c.volume,
                        fade_in_ms: c.fade_in_ms,
                        fade_out_ms: c.fade_out_ms,
                        provider: String::new(),
                        voice_id: String::new(),
                        voice_name: String::new(),
                    })
                    .collect();
                crate::models::VoiceoverTrack {
                    id: t.id,
                    enabled: t.enabled,
                    cues,
                }
            })
            .collect();
        // preserve original generation/provider per cue if exists
        for vt in &mut voiceover_tracks {
            if let Some(orig) = base.voiceover_tracks.iter().find(|o| o.id == vt.id) {
                for cue in &mut vt.cues {
                    if let Some(oc) = orig.cues.iter().find(|o| o.id == cue.id) {
                        cue.generation_id = oc.generation_id.clone();
                        cue.provider = oc.provider.clone();
                        cue.voice_id = oc.voice_id.clone();
                        cue.voice_name = oc.voice_name.clone();
                    }
                }
            }
        }
        if !applied.contains(&"audio_volume".to_owned()) {
            applied.push("audio_volume".to_owned());
        }
    }

    // overlay
    let mut overlay_clips = base.overlay_clips.clone();
    if let Some(deleted) = payload.overlay_deleted_shot_indices {
        if !deleted.is_empty() {
            let existing: std::collections::HashSet<i64> =
                overlay_clips.iter().map(|c| c.shot_index).collect();
            for s in &deleted {
                if !existing.contains(s) {
                    return Err(format!("叠加待删除镜头 {s} 不存在"));
                }
            }
            let del_set: std::collections::HashSet<i64> = deleted.into_iter().collect();
            overlay_clips.retain(|c| !del_set.contains(&c.shot_index));
            applied.push("overlay_delete".to_owned());
        }
    }
    if let Some(order) = payload.overlay_reorder {
        if !order.is_empty() {
            if order.len() != overlay_clips.len() {
                return Err("叠加排序必须包含全部叠加镜头".to_owned());
            }
            let mut seen = std::collections::HashSet::new();
            for s in &order {
                if !seen.insert(*s) {
                    return Err(format!("叠加排序中镜头 {s} 重复"));
                }
            }
            let existing: std::collections::HashSet<i64> =
                overlay_clips.iter().map(|c| c.shot_index).collect();
            if order.iter().any(|s| !existing.contains(s)) {
                return Err("叠加排序包含不存在的镜头".to_owned());
            }
            let map: std::collections::HashMap<i64, TimelineClip> = overlay_clips
                .into_iter()
                .map(|c| (c.shot_index, c))
                .collect();
            overlay_clips = order
                .iter()
                .map(|s| map.get(s).cloned().expect("existing overlay"))
                .collect();
            applied.push("overlay_reorder".to_owned());
        }
    }
    if let Some(adjs) = payload.overlay_adjustments {
        if !adjs.is_empty() {
            let mut seen = std::collections::HashSet::new();
            for a in &adjs {
                if !seen.insert(a.shot_index) {
                    return Err(format!("叠加时长调整中镜头 {} 重复", a.shot_index));
                }
            }
            for adj in &adjs {
                let idx = overlay_clips
                    .iter()
                    .position(|c| c.shot_index == adj.shot_index)
                    .ok_or_else(|| format!("叠加镜头 {} 不存在", adj.shot_index))?;
                if adj.new_duration_ms < 200 || adj.new_duration_ms > 12000 {
                    return Err("叠加片段时长需在 200–12000ms 之间".to_owned());
                }
                if adj.new_source_start_ms < 0 || adj.new_timeline_start_ms < 0 {
                    return Err("叠加源/时间起点不能为负".to_owned());
                }
                let orig = overlay_clips[idx].clone();
                let (kind, dur_opt) =
                    asset_kind_and_duration(&connection, &payload.project_id, &orig.asset_id)?;
                if kind == "video" {
                    let dur = dur_opt.ok_or_else(|| "叠加视频素材无时长".to_owned())?;
                    if adj.new_source_start_ms + adj.new_duration_ms > dur {
                        return Err(format!("叠加镜头 {} 超出素材时长", adj.shot_index));
                    }
                    overlay_clips[idx].source_start_ms = adj.new_source_start_ms;
                    overlay_clips[idx].source_end_ms =
                        adj.new_source_start_ms + adj.new_duration_ms;
                } else if kind == "image" {
                    if adj.new_source_start_ms != 0 {
                        return Err("叠加图片源起点必须为 0".to_owned());
                    }
                    overlay_clips[idx].source_start_ms = 0;
                    overlay_clips[idx].source_end_ms = 0;
                } else {
                    return Err("不支持的叠加素材类型".to_owned());
                }
                overlay_clips[idx].timeline_start_ms = adj.new_timeline_start_ms;
                overlay_clips[idx].timeline_end_ms =
                    adj.new_timeline_start_ms + adj.new_duration_ms;
                overlay_clips[idx].on_screen_text = orig.on_screen_text.clone();
            }
            applied.push("overlay_duration".to_owned());
        }
    }
    if let Some(inserted) = payload.overlay_inserted {
        if !inserted.is_empty() {
            let mut next_shot_index = overlay_clips
                .iter()
                .map(|c| c.shot_index)
                .max()
                .unwrap_or(10000 - 1)
                + 1;
            if next_shot_index < 10000 {
                next_shot_index = 10000;
            }
            for ins in inserted {
                if ins.asset_id.is_empty() {
                    return Err("叠加插入缺少 assetId".to_owned());
                }
                let dur = ins.timeline_end_ms - ins.timeline_start_ms;
                if dur < 200 || dur > 12000 {
                    return Err("叠加插入时长需在 200–12000ms 之间".to_owned());
                }
                if ins.timeline_start_ms < 0 {
                    return Err("叠加插入起点不能为负".to_owned());
                }
                let src_dur = ins.source_end_ms - ins.source_start_ms;
                let (kind, file_dur_opt) =
                    asset_kind_and_duration(&connection, &payload.project_id, &ins.asset_id)?;
                if kind == "video" {
                    if src_dur != dur {
                        return Err("叠加视频源时长与时间线时长不一致".to_owned());
                    }
                    if ins.source_start_ms < 0
                        || ins.source_end_ms > file_dur_opt.unwrap_or(i64::MAX)
                    {
                        return Err("叠加插入超出素材时长".to_owned());
                    }
                } else if kind == "image" {
                    if ins.source_start_ms != 0 || ins.source_end_ms != 0 {
                        return Err("叠加图片源必须为 0".to_owned());
                    }
                } else {
                    return Err("不支持的叠加素材类型".to_owned());
                }
                overlay_clips.push(TimelineClip {
                    crop_focus: None,
                    shot_index: next_shot_index,
                    asset_id: ins.asset_id,
                    source_start_ms: ins.source_start_ms,
                    source_end_ms: ins.source_end_ms,
                    timeline_start_ms: ins.timeline_start_ms,
                    timeline_end_ms: ins.timeline_end_ms,
                    on_screen_text: ins.on_screen_text,
                    clip_kind: "overlay".to_owned(),
                    derived_from_shot_index: None,
                    fit_reason: None,
                });
                next_shot_index += 1;
            }
            applied.push("overlay_insert".to_owned());
        }
    }
    overlay_clips.sort_by_key(|c| c.timeline_start_ms);

    if applied.is_empty() {
        return Err("没有可应用的改动".to_owned());
    }

    let version_number: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(version_number), 0) + 1 FROM timeline_versions WHERE project_id = ?1",
            params![payload.project_id],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())?;
    let new_id = Uuid::new_v4().to_string();
    let created_at = now_millis();
    let new_version = TimelineVersion {
        id: new_id.clone(),
        project_id: payload.project_id.clone(),
        storyboard_version_id: base.storyboard_version_id.clone(),
        version_number,
        clips: clips.clone(),
        text_tracks: text_tracks.clone(),
        music_tracks: music_tracks.clone(),
        voiceover_tracks: voiceover_tracks.clone(),
        overlay_clips: overlay_clips.clone(),
        quality_report: None,
        created_at,
    };
    let content = TimelineContent {
        clips,
        text_tracks,
        music_tracks: music_tracks.clone(),
        voiceover_tracks: voiceover_tracks.clone(),
        overlay_clips,
        quality_report: None,
    };
    let content_json = serde_json::to_string(&content).map_err(|e| e.to_string())?;
    let before_json = serde_json::to_string(&base.to_content()).map_err(|e| e.to_string())?;
    let after_json = serde_json::to_string(&new_version.to_content()).map_err(|e| e.to_string())?;

    let conversation_id: Option<String> = connection
        .query_row(
            "SELECT id FROM conversations WHERE project_id = ?1 AND editing_task_id = ?2 ORDER BY updated_at DESC LIMIT 1",
            params![payload.project_id, payload.editing_task_id],
            |r| r.get(0),
        )
        .ok();

    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    transaction.execute(
        "INSERT INTO timeline_versions (id, project_id, storyboard_version_id, version_number, status, content_json, created_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
        params![new_id, payload.project_id, new_version.storyboard_version_id, version_number, content_json, created_at],
    ).map_err(|e| e.to_string())?;
    transaction.execute(
        "INSERT INTO operation_logs (id, project_id, editing_task_id, conversation_id, agent_task_id, actor, operation_type, entity_type, entity_id, before_json, after_json, created_at) VALUES (?1, ?2, ?3, ?4, NULL, 'user', 'studio_commit', 'timeline_version', ?5, ?6, ?7, ?8)",
        params![Uuid::new_v4().to_string(), payload.project_id, payload.editing_task_id, conversation_id, new_id, before_json, after_json, created_at],
    ).map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;

    Ok(StudioCommitResult {
        timeline: new_version,
        applied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replacement_keeps_tracks_and_slot_and_rolls_back_on_write_failure() {
        let connection = Connection::open_in_memory().unwrap();
        crate::db::migrate(&connection).unwrap();
        connection.execute_batch("INSERT INTO projects (id,name,created_at,updated_at) VALUES ('p','test',1,1);
          INSERT INTO editing_tasks (id,project_id,title,created_at,updated_at) VALUES ('task','p','test',1,1);
          INSERT INTO storyboard_versions (id,project_id,editing_task_id,version_number,status,content_json,created_at) VALUES ('s','p','task',1,'ready','{}',1);
          INSERT INTO assets (id,project_id,kind,display_name,source_reference,analysis_status,metadata_json,created_at,updated_at) VALUES ('b','p','video','candidate','test','ready','{\"durationMs\":9000}',1,1);").unwrap();
        let source = json!({
          "clips": [
            {"shotIndex":1,"assetId":"a","sourceStartMs":0,"sourceEndMs":3000,"timelineStartMs":0,"timelineEndMs":3000,"onScreenText":"保留字幕","cropFocus":[0.3,0.4]},
            {"shotIndex":2,"assetId":"a","sourceStartMs":3000,"sourceEndMs":6000,"timelineStartMs":3000,"timelineEndMs":6000,"onScreenText":"第二镜"}],
          "textTracks":[{"id":"text","role":"subtitle","layer":1,"enabled":true,"cues":[{"id":"cue","startMs":0,"endMs":3000,"text":"保留文案"}]}],
          "musicTracks":[{"id":"music","enabled":true,"cues":[{"id":"mc","assetId":"music-asset","sourceStartMs":0,"sourceEndMs":6000,"timelineStartMs":0,"timelineEndMs":6000,"volume":0.2}]}],
          "voiceoverTracks":[{"id":"voice","enabled":true,"cues":[{"id":"vc","assetId":"voice-asset","generationId":"g","sourceStartMs":0,"sourceEndMs":6000,"timelineStartMs":0,"timelineEndMs":6000,"volume":1.0,"fadeInMs":0,"fadeOutMs":0,"provider":"test","voiceId":"v","voiceName":"test"}]}]
        });
        connection.execute("INSERT INTO timeline_versions (id,project_id,storyboard_version_id,version_number,status,content_json,created_at) VALUES ('t','p','s',1,'draft',?1,1)", [source.to_string()]).unwrap();
        let before = load_timeline_version(&connection, "t").unwrap();
        let payload = || {
            serde_json::from_value(json!({"projectId":"p","editingTaskId":"task","timelineVersionId":"t","clipReplacements":[{"shotIndex":1,"assetId":"b","sourceStartMs":2000,"sourceEndMs":5000,"cropFocus":[0.7,0.5]}]})).unwrap()
        };
        let result = commit_studio_edits_inner(&connection, payload()).unwrap();
        let after = load_timeline_version(&connection, &result.timeline.id).unwrap();
        assert_ne!(after.id, before.id);
        assert_eq!(after.clips[0].asset_id, "b");
        assert_eq!(after.clips[0].crop_focus, Some([0.7, 0.5]));
        assert_eq!(after.clips[0].timeline_end_ms, 3000);
        assert_eq!(
            after.clips[0].source_end_ms - after.clips[0].source_start_ms,
            3000
        );
        let old = serde_json::to_value(before.to_content()).unwrap();
        let new = serde_json::to_value(after.to_content()).unwrap();
        assert_eq!(old["clips"][1], new["clips"][1]);
        for field in [
            "textTracks",
            "musicTracks",
            "voiceoverTracks",
            "overlayClips",
        ] {
            assert_eq!(old[field], new[field]);
        }
        assert_eq!(
            load_timeline_version(&connection, "t").unwrap().clips[0].asset_id,
            "a"
        );
        connection.execute_batch("CREATE TRIGGER fail_operation BEFORE INSERT ON operation_logs BEGIN SELECT RAISE(ABORT,'test write failure'); END;").unwrap();
        assert!(commit_studio_edits_inner(&connection, payload()).is_err());
        let versions: i64 = connection
            .query_row("SELECT COUNT(*) FROM timeline_versions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(versions, 2);
    }
}
