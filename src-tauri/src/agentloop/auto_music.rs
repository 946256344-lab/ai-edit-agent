//! BGM 开启时 generate_storyboard 的自动配乐，分两步：
//! 1. 生成分镜前选曲（先用素材库里用户导入的音频，没有再按情绪标签找 Jamendo 器乐曲），并按需补节拍分析，
//!    配音关时分镜据此定音乐窗口、把切点吸附到拍上（见 `music_plan.rs`）；
//! 2. 时间线生成后按分镜的音乐窗口铺音乐（同一起点偏移、结尾落在乐句结束并淡出）；配音开或没有窗口时，
//!    选起点让结尾落在乐句边界，切点只在 ±120ms 内挪到拍上。
//! 只写一条新时间线版本；两条选曲路都不可用时返回真实原因，由 Agent 如实转告，不静默跳过。
use crate::models::{MusicCue, MusicTrack, TimelineClip, TimelineVersion};
use crate::music_plan::{nudge_cuts_to_beats, plan_for_fixed_length, MusicChoice, MusicPlan};
use crate::music_provider::{attribution_for, download_track, search_instrumental_by_tags};
use crate::timeline::replace_music_tracks;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;
use tauri::{AppHandle, Manager};

/// 配音开时切点吸附到拍上的容差：不到半拍，口播对位基本不受影响。
const VOICEOVER_SNAP_TOLERANCE_MS: i64 = 120;
const MIN_NUDGED_SHOT_MS: i64 = 500;

pub(super) enum AutoMusic {
    Attached {
        timeline: TimelineVersion,
        source: &'static str,
        title: String,
        /// 给 Agent 解释卡点的事实（BPM、音乐起点、切点在拍上的数量）。
        timing: Value,
    },
    Unavailable(String),
}

pub(super) struct MusicScope<'a> {
    pub app: &'a AppHandle,
    pub connection: &'a Connection,
    pub project_id: &'a str,
    pub editing_task_id: &'a str,
    pub conversation_id: &'a str,
    pub agent_task_id: &'a str,
}

struct LibraryAudio {
    asset_id: String,
    name: String,
    duration_ms: i64,
}

/// 情绪关键词 → Jamendo 标签与本地文件名匹配词；按简报先命中的一组为准。
const MOODS: &[(&[&str], &str)] = &[
    (&["upbeat", "energetic", "fun", "party", "dance", "happy", "欢快", "活力", "动感", "热闹"], "energetic happy"),
    (&["calm", "relax", "chill", "peaceful", "gentle", "warm", "舒缓", "安静", "温暖", "治愈"], "chillout relaxing"),
    (&["cinematic", "epic", "inspiring", "dramatic", "tech", "史诗", "大气", "震撼", "科技"], "cinematic inspiring"),
    (&["romantic", "love", "wedding", "浪漫", "婚礼"], "romantic acoustic"),
];

fn mood_tags(brief: &str) -> &'static str {
    let lower = brief.to_lowercase();
    MOODS
        .iter()
        .find(|(words, _)| words.iter().any(|word| lower.contains(word)))
        .map_or("happy pop", |(_, tags)| tags)
}

/// 用户导入的音频：排除应用自己生成或下载的文件（配音、Jamendo），它们在应用数据目录下。
fn library_audio(scope: &MusicScope<'_>) -> Result<Vec<LibraryAudio>, String> {
    let app_data = scope
        .app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut statement = scope
        .connection
        .prepare(
            "SELECT id, display_name, source_reference, json_extract(metadata_json, '$.durationMs') FROM assets \
             WHERE kind = 'audio' AND analysis_status = 'ready' \
             AND id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?1) \
             AND coalesce(json_extract(metadata_json, '$.libraryRemoved'), 0) = 0 \
             AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 0 \
             AND coalesce((SELECT status FROM asset_source_health ash WHERE ash.asset_id = assets.id), 'unchecked') NOT IN ('missing', 'changed', 'unreadable')",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![scope.project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    let mut audio = Vec::new();
    for row in rows {
        let (asset_id, name, source, duration_ms) = row.map_err(|error| error.to_string())?;
        if Path::new(&source).starts_with(&app_data) || !Path::new(&source).is_file() {
            continue;
        }
        if let Some(duration_ms) = duration_ms.filter(|value| *value > 0) {
            audio.push(LibraryAudio { asset_id, name, duration_ms });
        }
    }
    Ok(audio)
}

/// 文件名命中情绪词优先，其次能覆盖全片（不必循环），再其次更长。
fn pick_library_audio(audio: Vec<LibraryAudio>, brief: &str, timeline_ms: i64) -> Option<LibraryAudio> {
    let tags = mood_tags(brief);
    let words = MOODS
        .iter()
        .find(|(_, mood)| *mood == tags)
        .map_or(&[][..], |(words, _)| *words);
    audio.into_iter().max_by_key(|item| {
        let name = item.name.to_lowercase();
        (
            words.iter().any(|word| name.contains(word)),
            item.duration_ms >= timeline_ms,
            item.duration_ms,
        )
    })
}

/// 生成分镜前选曲并补节拍分析。`duration_hint_ms` 只用来偏好能覆盖全片的曲子。
pub(super) fn choose_background_music(
    scope: &MusicScope<'_>,
    brief: &str,
    duration_hint_ms: i64,
) -> Result<MusicChoice, String> {
    let library_reason = match library_audio(scope) {
        Ok(audio) => match pick_library_audio(audio, brief, duration_hint_ms) {
            Some(pick) => {
                return Ok(with_beats(
                    scope,
                    MusicChoice {
                        asset_id: pick.asset_id,
                        title: pick.name,
                        source: "library",
                        duration_ms: pick.duration_ms,
                        license: None,
                        analysis: None,
                        analysis_note: None,
                    },
                ))
            }
            None => "the media library has no analyzed audio".to_owned(),
        },
        Err(error) => format!("the media library could not be read ({error})"),
    };
    let tags = mood_tags(brief);
    let jamendo = (|| -> Result<MusicChoice, String> {
        let track = search_instrumental_by_tags(tags)?
            .into_iter()
            .next()
            .ok_or_else(|| format!("Jamendo found no eligible instrumental for \"{tags}\""))?;
        let asset = download_track(scope.app, scope.project_id, &track.id)?;
        let asset = crate::assets::wait_for_asset_ready(scope.app, scope.project_id, &asset.id)?;
        let duration_ms = asset
            .duration_ms
            .ok_or_else(|| "the downloaded music has no verified duration".to_owned())?;
        Ok(MusicChoice {
            asset_id: asset.id,
            title: format!("{} — {}", track.artist_name, track.name),
            source: "jamendo",
            duration_ms,
            license: Some((track.license_ccurl.clone(), attribution_for(&track))),
            analysis: None,
            analysis_note: None,
        })
    })();
    match jamendo {
        Ok(choice) => Ok(with_beats(scope, choice)),
        Err(error) => Err(format!(
            "No background music was added: {library_reason}, and {error}. Import a music file into the media library or add a Jamendo Client ID in Model settings."
        )),
    }
}

/// 节拍分析缺失或版本旧时现算；失败只记原因，曲子照样可以当 BGM。
fn with_beats(scope: &MusicScope<'_>, mut choice: MusicChoice) -> MusicChoice {
    match crate::assets::beats::ensure_beat_analysis(scope.connection, &choice.asset_id) {
        Ok(analysis) => choice.analysis = Some(analysis),
        Err(error) => {
            log::warn!("Beat analysis unavailable for music {}: {error}", choice.asset_id);
            choice.analysis_note = Some(format!("beat analysis failed ({error})"));
        }
    }
    choice
}

fn music_cue(
    choice: &MusicChoice,
    source_start_ms: i64,
    timeline_ms: i64,
    volume: f64,
    fades: (i64, i64),
) -> MusicCue {
    let source_end = (source_start_ms + timeline_ms).min(choice.duration_ms);
    let (provider, license_url, attribution) = match &choice.license {
        Some((url, attribution)) => (Some("Jamendo".to_owned()), Some(url.clone()), Some(attribution.clone())),
        None => (None, None, None),
    };
    let id = format!("{}-{}", choice.source, choice.asset_id);
    MusicCue {
        id: format!("{id}-cue"),
        asset_id: choice.asset_id.clone(),
        source_start_ms,
        source_end_ms: source_end,
        timeline_start_ms: 0,
        timeline_end_ms: timeline_ms,
        loop_enabled: source_end - source_start_ms < timeline_ms,
        volume,
        fade_in_ms: fades.0,
        fade_out_ms: fades.1,
        jianying_compatibility: "not_deliverable".to_owned(),
        provider,
        license_url,
        attribution,
    }
}

fn cut_points(clips: &[TimelineClip]) -> Vec<i64> {
    let mut ends = clips.iter().map(|clip| clip.timeline_end_ms).collect::<Vec<_>>();
    ends.sort_unstable();
    ends.pop();
    ends
}

fn cuts_on_beats(cuts: &[i64], beats: &[i64]) -> usize {
    cuts.iter()
        .filter(|cut| beats.iter().any(|beat| (*beat - **cut).abs() <= 15))
        .count()
}

fn asset_duration_ms(connection: &Connection, asset_id: &str) -> Option<i64> {
    connection
        .query_row(
            "SELECT json_extract(metadata_json, '$.durationMs') FROM assets WHERE id = ?1",
            params![asset_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
}

/// 把相邻两镜的分界挪到新切点：前一镜延长 / 缩短，后一镜反向；源区间按各自原倍速同步移动，
/// 素材不够长就不动源区间（倍速变化不到一成）。
fn move_boundaries(connection: &Connection, clips: &mut [TimelineClip], new_cuts: &[i64]) {
    clips.sort_by_key(|clip| clip.timeline_start_ms);
    for (index, cut) in new_cuts.iter().enumerate() {
        if index + 1 >= clips.len() {
            break;
        }
        let delta = cut - clips[index].timeline_end_ms;
        if delta == 0 {
            continue;
        }
        let ratio = |clip: &TimelineClip| {
            (clip.source_end_ms - clip.source_start_ms).max(0) as f64
                / (clip.timeline_end_ms - clip.timeline_start_ms).max(1) as f64
        };
        let (before, after) = clips.split_at_mut(index + 1);
        let (left, right) = (&mut before[index], &mut after[0]);
        let left_shift = (delta as f64 * ratio(left)).round() as i64;
        let right_shift = (delta as f64 * ratio(right)).round() as i64;
        let left_end = left.source_end_ms + left_shift;
        let left_limit = asset_duration_ms(connection, &left.asset_id).unwrap_or(left.source_end_ms);
        if left.source_end_ms > left.source_start_ms && left_end > left.source_start_ms && left_end <= left_limit.max(left.source_end_ms) {
            left.source_end_ms = left_end;
        }
        let right_start = right.source_start_ms + right_shift;
        if right.source_end_ms > right.source_start_ms && right_start >= 0 && right_start < right.source_end_ms {
            right.source_start_ms = right_start;
        }
        left.timeline_end_ms = *cut;
        right.timeline_start_ms = *cut;
    }
}

/// 时间线生成后铺音乐。`plan` 是分镜按音乐先行定下的窗口；没有时按成片长度现选起点并小幅吸附切点。
pub(super) fn attach_background_music(
    scope: &MusicScope<'_>,
    timeline: &TimelineVersion,
    choice: &MusicChoice,
    plan: Option<&MusicPlan>,
    plan_note: Option<&str>,
) -> AutoMusic {
    let Some(timeline_ms) = timeline.clips.iter().map(|clip| clip.timeline_end_ms).max() else {
        return AutoMusic::Unavailable("The timeline has no shots to score.".to_owned());
    };
    let volume = if timeline.voiceover_tracks.is_empty() { 0.35 } else { 0.15 };
    let mut scored = timeline.clone();
    let (cue, timing) = match plan {
        Some(plan) => {
            if plan.duration_ms != timeline_ms {
                log::warn!(
                    "Music-first plan covers {}ms but the timeline is {timeline_ms}ms; keeping the planned start so cuts stay on the beat",
                    plan.duration_ms
                );
            }
            let cuts = cut_points(&timeline.clips);
            let timing = json!({
                "mode": "music_first",
                "tempoBpm": plan.tempo_bpm,
                "musicStartMs": plan.source_start_ms,
                "endsOnPhrase": plan.ends_on_phrase && plan.duration_ms == timeline_ms,
                "cutsOnBeat": cuts_on_beats(&cuts, &plan.beats_ms),
                "cuts": cuts.len(),
            });
            (music_cue(choice, plan.source_start_ms, timeline_ms, volume, (plan.fade_in_ms, plan.fade_out_ms)), timing)
        }
        None => match plan_for_fixed_length(choice, timeline_ms) {
            Ok(fitted) => {
                let cuts = cut_points(&timeline.clips);
                let nudged = nudge_cuts_to_beats(
                    &cuts,
                    &fitted.beats_ms,
                    VOICEOVER_SNAP_TOLERANCE_MS,
                    MIN_NUDGED_SHOT_MS,
                    timeline_ms,
                );
                move_boundaries(scope.connection, &mut scored.clips, &nudged);
                let timing = json!({
                    "mode": if timeline.voiceover_tracks.is_empty() { "content_clock" } else { "voiceover_clock" },
                    "tempoBpm": fitted.tempo_bpm,
                    "musicStartMs": fitted.source_start_ms,
                    "endsOnPhrase": true,
                    "cutsOnBeat": cuts_on_beats(&nudged, &fitted.beats_ms),
                    "cuts": nudged.len(),
                    "snapToleranceMs": VOICEOVER_SNAP_TOLERANCE_MS,
                    "note": plan_note,
                });
                (music_cue(choice, fitted.source_start_ms, timeline_ms, volume, (fitted.fade_in_ms, fitted.fade_out_ms)), timing)
            }
            Err(reason) => {
                let note = plan_note.map_or(reason.clone(), |note| format!("{note}; {reason}"));
                log::info!("Background music is not beat-aligned: {note}");
                let timing = json!({ "mode": "not_beat_aligned", "note": note });
                (music_cue(choice, 0, timeline_ms, volume, (250, 1_200)), timing)
            }
        },
    };
    let track = MusicTrack {
        id: format!("{}-{}", choice.source, choice.asset_id),
        enabled: true,
        cues: vec![cue],
    };
    match replace_music_tracks(
        scope.connection,
        scope.project_id,
        scope.editing_task_id,
        scope.conversation_id,
        scope.agent_task_id,
        &scored,
        vec![track],
    ) {
        Ok(timeline) => AutoMusic::Attached {
            timeline,
            source: choice.source,
            title: choice.title.clone(),
            timing,
        },
        Err(error) => AutoMusic::Unavailable(format!(
            "No background music was added: the {} track could not be used ({error}).",
            choice.source
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_mood_picks_tags_and_matching_library_track() {
        assert_eq!(mood_tags("Upbeat and cinematic road trip"), "energetic happy");
        assert_eq!(mood_tags("一条舒缓的品牌片"), "chillout relaxing");
        assert_eq!(mood_tags("product demo"), "happy pop");
        let audio = vec![
            LibraryAudio { asset_id: "long".into(), name: "ambient.mp3".into(), duration_ms: 200_000 },
            LibraryAudio { asset_id: "mood".into(), name: "Summer Fun.mp3".into(), duration_ms: 20_000 },
            LibraryAudio { asset_id: "short".into(), name: "sting.wav".into(), duration_ms: 5_000 },
        ];
        let pick = pick_library_audio(audio, "an upbeat, fun recap", 30_000).unwrap();
        assert_eq!(pick.asset_id, "mood");
        let audio = vec![
            LibraryAudio { asset_id: "short".into(), name: "a.mp3".into(), duration_ms: 10_000 },
            LibraryAudio { asset_id: "covers".into(), name: "b.mp3".into(), duration_ms: 40_000 },
        ];
        assert_eq!(pick_library_audio(audio, "demo", 30_000).unwrap().asset_id, "covers");
    }
}
