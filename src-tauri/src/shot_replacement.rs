//! 保存每轮候选池、读取目标镜头推荐，并只为用户试选的一个候选准备画面。
//! 试选不写 timeline；保存仍使用 Studio 提交，预览和剪映使用返回的新时间线。
use crate::db::open_connection;
use crate::models::{
    StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardVersion, TechnicalMetadata,
    TimelineClip, TimelineVersion,
};
use crate::provider::ModelAccess;
use crate::storyboard::{
    load_storyboard_version,
    phases::{self, BeatCandidatePool, NarrativeStructure, RoughStoryboard},
    storyboard_sources,
    timing::{BeatTiming, SpeechTiming, SpeechTimingKind},
};
use crate::timeline::load_timeline_version;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{collections::HashMap, fs, path::Path};
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotRecommendation {
    asset_id: String,
    display_name: String,
    thumbnail_path: Option<String>,
    duration_ms: Option<i64>,
    current: bool,
    used_in_timeline: bool,
    unavailable_reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotRecommendations {
    saved: bool,
    beat_purpose: String,
    candidates: Vec<ShotRecommendation>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedShotReplacement {
    pub timeline_version_id: String,
    pub shot_index: i64,
    pub asset_id: String,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub crop_focus: Option<[f64; 2]>,
    pub preview_path: String,
}

pub(crate) fn store_pools(
    connection: &Connection,
    storyboard_id: &str,
    pools: &[BeatCandidatePool],
) -> Result<(), String> {
    let json = serde_json::to_string(pools).map_err(|e| e.to_string())?;
    connection.execute("INSERT INTO storyboard_recommendations (storyboard_version_id, pools_json) VALUES (?1, ?2) ON CONFLICT(storyboard_version_id) DO UPDATE SET pools_json=excluded.pools_json", params![storyboard_id, json]).map_err(|e| e.to_string())?;
    Ok(())
}

fn load_pools(
    connection: &Connection,
    storyboard_id: &str,
) -> Result<Option<Vec<BeatCandidatePool>>, String> {
    let json: Option<String> = connection
        .query_row(
            "SELECT pools_json FROM storyboard_recommendations WHERE storyboard_version_id=?1",
            [storyboard_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    json.map(|json| serde_json::from_str(&json).map_err(|e| e.to_string()))
        .transpose()
}

struct ShotContext {
    timeline: TimelineVersion,
    storyboard: StoryboardVersion,
    clip: TimelineClip,
    shot: StoryboardShot,
    beat: StoryboardBeat,
}

fn context(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    timeline_id: &str,
    shot_index: i64,
) -> Result<ShotContext, String> {
    let timeline = load_timeline_version(connection, timeline_id)?;
    let storyboard = load_storyboard_version(connection, &timeline.storyboard_version_id)?;
    if timeline.project_id != project_id || storyboard.editing_task_id != editing_task_id {
        return Err("该镜头不属于当前剪辑会话。".to_owned());
    }
    let clip = timeline
        .clips
        .iter()
        .find(|clip| clip.shot_index == shot_index)
        .cloned()
        .ok_or("当前镜头不存在。")?;
    let original_index = clip.derived_from_shot_index.unwrap_or(clip.shot_index);
    let shot = storyboard
        .shots
        .iter()
        .find(|shot| shot.order_index == original_index)
        .cloned()
        .ok_or("该镜头没有可关联的生成依据，请通过对话调整。")?;
    let beat = storyboard
        .beats
        .iter()
        .find(|beat| beat.id == shot.beat_id)
        .cloned()
        .ok_or("该镜头没有可关联的叙事内容。")?;
    Ok(ShotContext {
        timeline,
        storyboard,
        clip,
        shot,
        beat,
    })
}

fn recommendations(
    connection: &Connection,
    ctx: &ShotContext,
) -> Result<ShotRecommendations, String> {
    let pools = load_pools(connection, &ctx.storyboard.id)?;
    let pool = pools
        .as_ref()
        .and_then(|pools| pools.iter().find(|pool| pool.beat_id == ctx.beat.id));
    let mut candidates = Vec::new();
    if let Some(pool) = pool {
        let (available, _) = storyboard_sources(connection, &ctx.timeline.project_id)?;
        for source in &pool.candidates {
            let asset: Option<(String, String)> = connection
                .query_row(
                    "SELECT display_name, metadata_json FROM assets WHERE id=?1 AND project_id=?2",
                    params![source.asset_id, ctx.timeline.project_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            let Some((display_name, metadata_json)) = asset else {
                continue;
            };
            let metadata: TechnicalMetadata =
                serde_json::from_str(&metadata_json).map_err(|e| e.to_string())?;
            let duration = ctx.clip.timeline_end_ms - ctx.clip.timeline_start_ms;
            let unavailable_reason = if !available
                .iter()
                .any(|item| item.asset_id == source.asset_id)
            {
                Some("素材当前不可用，请检查来源或分析状态".to_owned())
            } else if metadata.duration_ms.unwrap_or(0) < duration {
                Some(format!("可用时长不足 {:.1} 秒", duration as f64 / 1000.0))
            } else {
                None
            };
            candidates.push(ShotRecommendation {
                asset_id: source.asset_id.clone(),
                display_name,
                thumbnail_path: metadata.thumbnail_path,
                duration_ms: metadata.duration_ms,
                current: source.asset_id == ctx.clip.asset_id,
                used_in_timeline: ctx.timeline.clips.iter().any(|clip| {
                    clip.shot_index != ctx.clip.shot_index && clip.asset_id == source.asset_id
                }),
                unavailable_reason,
            });
        }
    }
    Ok(ShotRecommendations {
        saved: pools.is_some(),
        beat_purpose: ctx.beat.purpose.clone(),
        candidates,
    })
}

#[tauri::command]
pub fn list_shot_recommendations(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    timeline_version_id: String,
    shot_index: i64,
) -> Result<ShotRecommendations, String> {
    let connection = open_connection(&app)?;
    let ctx = context(
        &connection,
        &project_id,
        &editing_task_id,
        &timeline_version_id,
        shot_index,
    )?;
    recommendations(&connection, &ctx)
}

#[tauri::command]
pub async fn generate_shot_recommendations(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    timeline_version_id: String,
    shot_index: i64,
) -> Result<ShotRecommendations, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let connection = open_connection(&app)?;
        let ctx = context(
            &connection,
            &project_id,
            &editing_task_id,
            &timeline_version_id,
            shot_index,
        )?;
        let (sources, _) = storyboard_sources(&connection, &project_id)?;
        let narrative = NarrativeStructure {
            title: ctx.storyboard.title.clone(),
            summary: ctx.storyboard.summary.clone(),
            target_duration_ms: ctx.storyboard.target_duration_ms,
            script_mode: ctx.storyboard.script_mode.clone(),
            spoken_script: String::new(),
            beats: ctx.storyboard.beats.clone(),
        };
        let rough = phases::phase2_rough_shot_selection(
            &narrative,
            &sources,
            &HashMap::new(),
            &[],
            SpeechTiming::default(),
        )?;
        store_pools(&connection, &ctx.storyboard.id, &rough.candidate_pools)?;
        recommendations(&connection, &ctx)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn prepare_shot_replacement(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    timeline_version_id: String,
    shot_index: i64,
    asset_id: String,
) -> Result<PreparedShotReplacement, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let connection = open_connection(&app)?;
        let ctx = context(
            &connection,
            &project_id,
            &editing_task_id,
            &timeline_version_id,
            shot_index,
        )?;
        let pools = load_pools(&connection, &ctx.storyboard.id)?.ok_or("尚未生成推荐镜头。")?;
        if !pools.iter().any(|pool| {
            pool.beat_id == ctx.beat.id
                && pool
                    .candidates
                    .iter()
                    .any(|candidate| candidate.asset_id == asset_id)
        }) {
            return Err("请选择此镜头的推荐候选。".to_owned());
        }
        let (sources, _) = storyboard_sources(&connection, &project_id)?;
        let source = sources
            .into_iter()
            .find(|source| source.asset_id == asset_id)
            .ok_or("该素材当前不可用。")?;
        let duration = ctx.clip.timeline_end_ms - ctx.clip.timeline_start_ms;
        if source.duration_ms.unwrap_or(0) < duration {
            return Err("候选素材不足以保持原镜头时长。".to_owned());
        }
        let mut shot = ctx.shot.clone();
        shot.order_index = 1;
        shot.asset_id = asset_id.clone();
        shot.duration_ms = duration;
        shot.source_start_ms = 0;
        shot.source_end_ms = duration;
        shot.crop_focus = None;
        shot.beat_part_index = 1;
        shot.beat_part_count = 1;
        let selected = StoryboardContent {
            brief: ctx.storyboard.brief.clone(),
            title: ctx.storyboard.title.clone(),
            summary: ctx.storyboard.summary.clone(),
            target_duration_ms: duration,
            script_mode: ctx.storyboard.script_mode.clone(),
            beats: vec![ctx.beat.clone()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot],
        };
        let rough = RoughStoryboard {
            speech_timing: SpeechTiming {
                kind: SpeechTimingKind::Voice,
                beats: vec![BeatTiming {
                    beat_id: ctx.beat.id.clone(),
                    start_ms: 0,
                    end_ms: duration,
                }],
                pauses_ms: Vec::new(),
            },
            title: selected.title.clone(),
            summary: selected.summary.clone(),
            target_duration_ms: duration,
            script_mode: selected.script_mode.clone(),
            beats: selected.beats.clone(),
            uncovered_beat_ids: Vec::new(),
            shots: selected.shots.clone(),
            candidate_pools: Vec::new(),
        };
        let access = ModelAccess::resolve()?;
        let (refined, issues) = phases::phase4_refine_ranges(
            &app,
            &access,
            &ctx.storyboard.brief,
            &selected,
            &rough,
            std::slice::from_ref(&source),
            None,
        )?;
        if !issues.is_empty() {
            return Err("该候选未能形成符合原时长的片段，请选择其他候选。".to_owned());
        }
        let refined = refined.shots.first().ok_or("未能准备替换片段。")?;
        if refined.source_end_ms - refined.source_start_ms != duration {
            return Err("候选片段无法保持原镜头时长。".to_owned());
        }
        let mut preview_clip = ctx.clip.clone();
        preview_clip.asset_id = asset_id.clone();
        preview_clip.source_start_ms = refined.source_start_ms;
        preview_clip.source_end_ms = refined.source_end_ms;
        preview_clip.crop_focus = refined.crop_focus;
        preview_clip.clip_kind = "source".to_owned();
        let directory = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("previews")
            .join(&timeline_version_id);
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let destination = directory.join(format!("replacement-{}.mp4", uuid::Uuid::new_v4()));
        crate::preview::render_timeline_clip(
            Path::new(source.source_path.as_deref().ok_or("素材来源不可用。")?),
            "video",
            &preview_clip,
            &destination,
        )?;
        Ok(PreparedShotReplacement {
            timeline_version_id,
            shot_index,
            asset_id,
            source_start_ms: refined.source_start_ms,
            source_end_ms: refined.source_end_ms,
            crop_focus: refined.crop_focus,
            preview_path: destination.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendations_survive_migration_and_follow_storyboard_deletion() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        crate::db::migrate(&connection).unwrap();
        connection.execute_batch("INSERT INTO projects (id,name,created_at,updated_at) VALUES ('p','test',1,1);
            INSERT INTO storyboard_versions (id,project_id,version_number,status,content_json,created_at) VALUES ('s','p',1,'ready','{}',1);").unwrap();
        assert!(load_pools(&connection, "s").unwrap().is_none());
        let pools = vec![BeatCandidatePool {
            beat_id: "beat-1".to_owned(),
            beat_purpose: "展示产品".to_owned(),
            candidates: (0..12)
                .map(|index| {
                    serde_json::from_value(serde_json::json!({
                        "assetId": format!("asset-{index}"), "kind": "video", "durationMs": 5000,
                        "sceneSegments": [], "ocrEvidence": [], "visualEvidence": [],
                        "sourcePath": "private-local-source", "evidenceEmbedding": [0.2,0.4]
                    }))
                    .unwrap()
                })
                .collect(),
        }];
        store_pools(&connection, "s", &pools).unwrap();
        crate::db::migrate(&connection).unwrap();
        let loaded = load_pools(&connection, "s").unwrap().unwrap();
        assert_eq!(loaded[0].candidates.len(), 12);
        assert_eq!(loaded[0].candidates[11].asset_id, "asset-11");
        assert!(loaded[0].candidates[0].source_path.is_none());
        assert!(loaded[0].candidates[0].evidence_embedding.is_none());
        connection
            .execute("DELETE FROM storyboard_versions WHERE id='s'", [])
            .unwrap();
        assert!(load_pools(&connection, "s").unwrap().is_none());
    }
}
