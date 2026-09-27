//! BGM 开启时 generate_storyboard 的自动配乐：先用素材库里用户导入的音频，没有再按情绪标签找 Jamendo 器乐曲。
//! 只写一条新时间线版本；两条路都不可用时返回真实原因，由 Agent 如实转告，不静默跳过。
use crate::models::{MusicCue, MusicTrack, TimelineVersion};
use crate::music_provider::{attribution_for, download_track, search_instrumental_by_tags};
use crate::timeline::replace_music_tracks;
use rusqlite::{params, Connection};
use std::path::Path;
use tauri::{AppHandle, Manager};

pub(super) enum AutoMusic {
    Attached {
        timeline: TimelineVersion,
        source: &'static str,
        title: String,
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

fn music_track(
    id: String,
    asset_id: String,
    source_duration_ms: i64,
    timeline: &TimelineVersion,
    timeline_ms: i64,
    license: Option<(String, String)>,
) -> MusicTrack {
    let source_end = source_duration_ms.min(timeline_ms);
    let (provider, license_url, attribution) = match license {
        Some((url, attribution)) => (Some("Jamendo".to_owned()), Some(url), Some(attribution)),
        None => (None, None, None),
    };
    MusicTrack {
        id: id.clone(),
        enabled: true,
        cues: vec![MusicCue {
            id: format!("{id}-cue"),
            asset_id,
            source_start_ms: 0,
            source_end_ms: source_end,
            timeline_start_ms: 0,
            timeline_end_ms: timeline_ms,
            loop_enabled: source_end < timeline_ms,
            volume: if timeline.voiceover_tracks.is_empty() { 0.35 } else { 0.15 },
            fade_in_ms: 250,
            fade_out_ms: 1_200,
            jianying_compatibility: "not_deliverable".to_owned(),
            provider,
            license_url,
            attribution,
        }],
    }
}

pub(super) fn attach_background_music(
    scope: &MusicScope<'_>,
    timeline: &TimelineVersion,
    brief: &str,
) -> AutoMusic {
    let Some(timeline_ms) = timeline.clips.iter().map(|clip| clip.timeline_end_ms).max() else {
        return AutoMusic::Unavailable("The timeline has no shots to score.".to_owned());
    };
    let write = |track: MusicTrack| {
        replace_music_tracks(
            scope.connection,
            scope.project_id,
            scope.editing_task_id,
            scope.conversation_id,
            scope.agent_task_id,
            timeline,
            vec![track],
        )
    };
    let library_reason = match library_audio(scope) {
        Ok(audio) => match pick_library_audio(audio, brief, timeline_ms) {
            Some(pick) => {
                let track = music_track(
                    format!("library-{}", pick.asset_id),
                    pick.asset_id.clone(),
                    pick.duration_ms,
                    timeline,
                    timeline_ms,
                    None,
                );
                match write(track) {
                    Ok(timeline) => {
                        return AutoMusic::Attached { timeline, source: "library", title: pick.name }
                    }
                    Err(error) => format!("the library track could not be used ({error})"),
                }
            }
            None => "the media library has no analyzed audio".to_owned(),
        },
        Err(error) => format!("the media library could not be read ({error})"),
    };
    let tags = mood_tags(brief);
    let jamendo = (|| -> Result<(TimelineVersion, String), String> {
        let track = search_instrumental_by_tags(tags)?
            .into_iter()
            .next()
            .ok_or_else(|| format!("Jamendo found no eligible instrumental for \"{tags}\""))?;
        let asset = download_track(scope.app, scope.project_id, &track.id)?;
        let asset = crate::assets::wait_for_asset_ready(scope.app, scope.project_id, &asset.id)?;
        let duration = asset
            .duration_ms
            .ok_or_else(|| "the downloaded music has no verified duration".to_owned())?;
        let written = write(music_track(
            format!("jamendo-{}", track.id),
            asset.id,
            duration,
            timeline,
            timeline_ms,
            Some((track.license_ccurl.clone(), attribution_for(&track))),
        ))?;
        Ok((written, format!("{} — {}", track.artist_name, track.name)))
    })();
    match jamendo {
        Ok((timeline, title)) => AutoMusic::Attached { timeline, source: "jamendo", title },
        Err(error) => AutoMusic::Unavailable(format!(
            "No background music was added: {library_reason}, and {error}. Import a music file into the media library or add a Jamendo Client ID in Model settings."
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
