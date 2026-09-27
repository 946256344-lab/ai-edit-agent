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
