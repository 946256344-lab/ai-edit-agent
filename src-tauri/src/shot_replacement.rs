//! 保存每轮候选池、读取目标镜头推荐，并只为用户试选的一个候选准备画面。
//! 试选不写 timeline；保存仍使用 Studio 提交，预览和剪映使用返回的新时间线。
use crate::db::open_connection;
use crate::models::{
    CandidateSegment, StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource,
    StoryboardVersion, TechnicalMetadata, TimelineClip, TimelineVersion,
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
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotRecommendation {
    candidate_id: String,
    asset_id: String,
    source_start_ms: i64,
    source_end_ms: i64,
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

// 模型用的 StoryboardSource 隐去 segment；本地候选存储显式保留片段身份和边界。
#[derive(Serialize)]
struct SavedCandidate<'a> {
    #[serde(flatten)]
    source: &'a StoryboardSource,
    segment: &'a Option<CandidateSegment>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedPool<'a> {
    beat_id: &'a str,
    beat_purpose: &'a str,
    candidates: Vec<SavedCandidate<'a>>,
}

fn candidate_key(source: &StoryboardSource) -> String {
    format!(
        "{}:{}",
        source.asset_id,
        source
            .segment
            .as_ref()
            .map_or("whole", |segment| segment.id.as_str())
    )
}

pub(crate) fn store_pools(
    connection: &Connection,
    storyboard_id: &str,
    pools: &[BeatCandidatePool],
) -> Result<(), String> {
    let saved: Vec<_> = pools
        .iter()
        .map(|pool| SavedPool {
            beat_id: &pool.beat_id,
            beat_purpose: &pool.beat_purpose,
            candidates: pool
                .candidates
                .iter()
                .map(|source| SavedCandidate {
                    source,
                    segment: &source.segment,
                })
                .collect(),
        })
        .collect();
    let json = serde_json::to_string(&saved).map_err(|e| e.to_string())?;
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
        let expanded: HashSet<_> = pool
            .candidates
            .iter()
            .filter(|source| source.segment.is_some())
            .map(|source| source.asset_id.clone())
            .collect();
        let (available, _) =
            storyboard_sources(connection, &ctx.timeline.project_id, Some(&expanded))?;
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
            let key = candidate_key(source);
            let live = available.iter().find(|item| candidate_key(item) == key);
            let segment = live
                .and_then(|item| item.segment.as_ref())
                .or(source.segment.as_ref());
            let start = segment.map_or(0, |segment| segment.start_ms);
            let end = segment.map_or(metadata.duration_ms.unwrap_or(0), |segment| segment.end_ms);
            let unavailable_reason = if live.is_none() {
                Some("素材当前不可用，请检查来源或分析状态".to_owned())
            } else if end - start < duration {
                Some(format!("可用时长不足 {:.1} 秒", duration as f64 / 1000.0))
            } else {
                None
            };
            candidates.push(ShotRecommendation {
                candidate_id: key,
                asset_id: source.asset_id.clone(),
                source_start_ms: start,
                source_end_ms: end,
                display_name,
                thumbnail_path: segment
                    .and_then(|segment| segment.frame_paths.first().cloned())
                    .or(metadata.thumbnail_path),
                duration_ms: Some(end - start),
                current: source.asset_id == ctx.clip.asset_id
                    && ctx.clip.source_start_ms >= start
                    && ctx.clip.source_end_ms <= end,
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
        let (whole, _) = storyboard_sources(&connection, &project_id, None)?;
        let expanded = whole.iter().map(|source| source.asset_id.clone()).collect();
        let (sources, _) = storyboard_sources(&connection, &project_id, Some(&expanded))?;
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
            &[],
            SpeechTiming::default(),
            None,
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
    candidate_id: String,
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
        let candidate = pools
            .iter()
            .filter(|pool| pool.beat_id == ctx.beat.id)
            .flat_map(|pool| &pool.candidates)
            .find(|candidate| candidate_key(candidate) == candidate_id)
            .ok_or("请选择此镜头的推荐候选。")?;
        let expanded: HashSet<_> = candidate
            .segment
            .iter()
            .map(|_| candidate.asset_id.clone())
            .collect();
        let (sources, _) = storyboard_sources(&connection, &project_id, Some(&expanded))?;
        let source = sources
            .into_iter()
            .find(|source| candidate_key(source) == candidate_id)
            .ok_or("该素材当前不可用。")?;
        let asset_id = source.asset_id.clone();
        let duration = ctx.clip.timeline_end_ms - ctx.clip.timeline_start_ms;
        let range_start = source
            .segment
            .as_ref()
            .map_or(0, |segment| segment.start_ms);
        let range_end = source
            .segment
            .as_ref()
            .map_or(source.duration_ms.unwrap_or(0), |segment| segment.end_ms);
        if range_end - range_start < duration {
            return Err("候选素材不足以保持原镜头时长。".to_owned());
        }
        let mut shot = ctx.shot.clone();
        shot.order_index = 1;
        shot.asset_id = asset_id.clone();
        shot.duration_ms = duration;
        shot.source_start_ms = range_start;
        shot.source_end_ms = range_end;
        shot.segment_id = source.segment.as_ref().map(|segment| segment.id.clone());
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
        let mut phase4_session = crate::storyboard::phase4::Phase4Session::new();
        let (refined, issues) = phases::phase4_refine_ranges(
            &app,
            &access,
            &ctx.storyboard.brief,
            &selected,
            &rough,
            std::slice::from_ref(&source),
            None,
            &mut phase4_session,
        )?;
        if !issues.is_empty() {
            return Err("该候选未能形成符合原时长的片段，请选择其他候选。".to_owned());
        }
        let refined = refined.shots.first().ok_or("未能准备替换片段。")?;
        if refined.source_end_ms - refined.source_start_ms != duration
            || refined.source_start_ms < range_start
            || refined.source_end_ms > range_end
        {
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
            scores: Vec::new(),
            candidates: (0..12)
                .map(|index| {
                    serde_json::from_value(serde_json::json!({
                        "assetId": if index < 2 { "asset-0".to_owned() } else { format!("asset-{index}") }, "kind": "video", "durationMs": 60000,
                        "sceneSegments": [], "ocrEvidence": [], "visualEvidence": [],
                        "sourcePath": "private-local-source", "evidenceEmbedding": [0.2,0.4],
                        "segment": {"id":format!("segment-{index}"),"startMs":index * 5000,"endMs":(index + 1)*5000,"framePaths":["private-frame"]}
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
        assert_eq!(
            loaded[0].candidates[0].asset_id,
            loaded[0].candidates[1].asset_id
        );
        assert_ne!(
            candidate_key(&loaded[0].candidates[0]),
            candidate_key(&loaded[0].candidates[1])
        );
        let segment = loaded[0].candidates[1].segment.as_ref().unwrap();
        assert_eq!((segment.start_ms, segment.end_ms), (5000, 10000));
        assert!(segment.frame_paths.is_empty());
        assert!(loaded[0].candidates[0].source_path.is_none());
        assert!(loaded[0].candidates[0].evidence_embedding.is_none());
        connection
            .execute("DELETE FROM storyboard_versions WHERE id='s'", [])
            .unwrap();
        assert!(load_pools(&connection, "s").unwrap().is_none());
    }
}
