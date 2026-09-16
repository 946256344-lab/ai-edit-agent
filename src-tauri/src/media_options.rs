//! 本轮媒体选择与分镜生成快照；旧分镜无快照时保持原有行为。
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaOptions {
    pub voiceover: bool,
    pub subtitles: bool,
    pub bgm: bool,
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
            }
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
