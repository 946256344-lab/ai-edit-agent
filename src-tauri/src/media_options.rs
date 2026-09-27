//! 本轮媒体选择与分镜生成快照；旧分镜无快照时保持原有行为（竖屏 9:16）。
//! 画幅随快照走：预览、裁切和编辑器画布都从时间线所属分镜读取，不单独存。
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::models::TimelineVersion;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum AspectRatio {
    #[default]
    #[serde(rename = "9:16")]
    Portrait,
    #[serde(rename = "16:9")]
    Landscape,
    #[serde(rename = "1:1")]
    Square,
}

/// 预览与编辑器草稿共用的低清画布；像素量与原 540×960 相当。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Canvas {
    pub width: i64,
    pub height: i64,
}

impl AspectRatio {
    pub fn canvas(self) -> Canvas {
        match self {
            AspectRatio::Portrait => Canvas { width: 540, height: 960 },
            AspectRatio::Landscape => Canvas { width: 960, height: 540 },
            AspectRatio::Square => Canvas { width: 720, height: 720 },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaOptions {
    pub voiceover: bool,
    pub subtitles: bool,
    pub bgm: bool,
    #[serde(default)]
    pub aspect_ratio: AspectRatio,
}

const VOICEOVER_WORDS: &[&str] = &["voiceover", "voice-over", "voice over", "narrat", "配音", "旁白", "口播", "解说"];
const SUBTITLE_WORDS: &[&str] = &["subtitle", "caption", "字幕"];
const MUSIC_WORDS: &[&str] = &["music", "bgm", "soundtrack", "song", "音乐", "配乐", "背景乐"];
const RATIO_WORDS: &[&str] = &[
    "9:16", "16:9", "1:1", "9/16", "16/9", "vertical", "portrait", "landscape", "horizontal", "square",
    "竖屏", "竖版", "横屏", "横版", "方形", "正方形",
];

fn mentions(request: &str, words: &[&str]) -> bool {
    let lower = request.to_lowercase();
    words.iter().any(|word| lower.contains(word))
}

/// 输入框选择是本轮的事实：模型只有在用户本轮原话点名某项时才能改它（模型曾在配音关闭、
/// 原话未提配音时自行打开配音与字幕）。漏传画幅按输入框，不退回竖屏。
pub(crate) fn guard_model_options(
    model: MediaOptions,
    composer: MediaOptions,
    request: &str,
) -> MediaOptions {
    let keep = |mentioned: bool, model_value: bool, composer_value: bool| {
        if mentioned { model_value } else { composer_value }
    };
    MediaOptions {
        voiceover: keep(mentions(request, VOICEOVER_WORDS), model.voiceover, composer.voiceover),
        subtitles: keep(
            mentions(request, SUBTITLE_WORDS) || mentions(request, VOICEOVER_WORDS),
            model.subtitles,
            composer.subtitles,
        ),
        bgm: keep(mentions(request, MUSIC_WORDS), model.bgm, composer.bgm),
        aspect_ratio: if mentions(request, RATIO_WORDS) {
            model.aspect_ratio
        } else {
            composer.aspect_ratio
        },
    }
}

/// 时间线画布按所属分镜的快照画幅；旧分镜无快照时为竖屏。
pub(crate) fn timeline_canvas(
    connection: &Connection,
    timeline: &TimelineVersion,
) -> Result<Canvas, String> {
    Ok(storyboard_options(connection, &timeline.storyboard_version_id)?
        .map(|options| options.aspect_ratio)
        .unwrap_or_default()
        .canvas())
}

/// 模型有时把对象再编码成字符串；对象和 JSON 字符串都收下。
pub(crate) fn parse_media_options(value: &serde_json::Value) -> Result<MediaOptions, String> {
    if let Some(text) = value.as_str() {
        return serde_json::from_str(text.trim()).map_err(|error| error.to_string());
    }
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storyboard_snapshot_preserves_all_toggle_combinations_and_legacy_absence() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE storyboard_versions (id TEXT PRIMARY KEY, content_json TEXT NOT NULL);
            INSERT INTO storyboard_versions VALUES ('storyboard', '{}');").unwrap();
        assert_eq!(
            parse_media_options(&serde_json::json!({
                "voiceover": true,
                "subtitles": false,
                "bgm": false
            }))
            .unwrap(),
            MediaOptions {
                voiceover: true,
                subtitles: false,
                bgm: false,
                aspect_ratio: AspectRatio::Portrait,
            }
        );
        assert_eq!(
            parse_media_options(&serde_json::json!({
                "voiceover": false, "subtitles": false, "bgm": true, "aspectRatio": "16:9"
            }))
            .unwrap()
            .aspect_ratio
            .canvas(),
            Canvas { width: 960, height: 540 }
        );
        assert_eq!(
            parse_media_options(&serde_json::json!(
                "{\"voiceover\":true,\"subtitles\":false,\"bgm\":false}"
            ))
            .unwrap()
            .voiceover,
            true
        );
        // 回归：配音关、原话没提配音时，模型自行打开的配音与字幕被拦回；点名时才采纳。
        let composer = MediaOptions {
            voiceover: false, subtitles: false, bgm: true, aspect_ratio: AspectRatio::Landscape,
        };
        let model = MediaOptions {
            voiceover: true, subtitles: true, bgm: true, aspect_ratio: AspectRatio::Portrait,
        };
        let recap = "Turn our weekend road trip into a 30-second recap. Upbeat and cinematic.";
        assert_eq!(guard_model_options(model, composer, recap), composer);
        let asked = guard_model_options(model, composer, "Add a voiceover, vertical please");
        assert!(asked.voiceover && asked.subtitles);
        assert_eq!(asked.aspect_ratio, AspectRatio::Portrait);
        assert_eq!(storyboard_options(&connection, "storyboard").unwrap(), None);
        for mask in 0..8 {
            let options = MediaOptions {
                voiceover: mask & 1 != 0,
                subtitles: mask & 2 != 0,
                bgm: mask & 4 != 0,
                aspect_ratio: AspectRatio::Square,
            };
            connection.execute(
                "UPDATE storyboard_versions SET content_json = json_set(content_json, '$.mediaOptions', json(?1))",
                params![serde_json::to_string(&options).unwrap()],
            ).unwrap();
            assert_eq!(
                storyboard_options(&connection, "storyboard").unwrap(),
                Some(options)
            );
        }
    }
}

pub(crate) fn storyboard_options(
    connection: &Connection,
    storyboard_id: &str,
) -> Result<Option<MediaOptions>, String> {
    let json: Option<String> = connection.query_row(
        "SELECT json_extract(content_json, '$.mediaOptions') FROM storyboard_versions WHERE id = ?1",
        params![storyboard_id], |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    json.map(|json| serde_json::from_str(&json).map_err(|error| error.to_string()))
        .transpose()
}
