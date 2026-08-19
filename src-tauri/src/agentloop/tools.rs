//! 原生 Function Tool 定义与 schema 合约。
//!
//! 本模块只拥有发送给 Provider 的稳定工具描述，不拥有工具执行权限；真实观察仍由
//! `skills::apply_skill` 负责，作用域由 LoopState 注入，避免模型参数伪造项目边界。

use serde_json::{json, Value};

const GET_EDIT_STATUS: &str = "get_edit_status";
#[allow(dead_code)]
const GET_ASSET_HEALTH_SUMMARY: &str = "get_asset_health_summary";
#[allow(dead_code)]
const LIST_ASSETS: &str = "list_assets";
const SEARCH_ASSETS: &str = "search_assets";
const SEARCH_ASSET_SEGMENTS: &str = "search_asset_segments";
const SEARCH_MUSIC: &str = "search_music";
const GET_STORYBOARD: &str = "get_storyboard";
#[allow(dead_code)]
const GET_TIMELINE: &str = "get_timeline";
const GET_TEXT_CAPABILITIES: &str = "get_text_capabilities";
#[allow(dead_code)]
const RENDER_PREVIEW: &str = "render_preview";

/// 只读原生工具保持独立入口，供 NativeToolLoop 的默认观察集合复用。
#[allow(dead_code)]
pub(crate) fn native_observation_function_tools() -> Vec<Value> {
    native_function_tools(false)
}

pub(crate) fn native_function_tools(include_render_preview: bool) -> Vec<Value> {
    let mut tools = vec![
        function_tool(
            GET_EDIT_STATUS,
            "Read the latest verified edit task and artifact status in the current conversation scope.",
            json!({}),
            Vec::new(),
        ),
        function_tool(
            GET_ASSET_HEALTH_SUMMARY,
            "Read persisted asset source-health counts and safe reason codes for the current project.",
            json!({}),
            Vec::new(),
        ),
        function_tool(
            LIST_ASSETS,
            "List the current project's persisted asset status summaries without starting analysis.",
            json!({}),
            Vec::new(),
        ),
        function_tool(
            SEARCH_ASSETS,
            "Search current-project assets with bounded metadata filters without starting analysis.",
            json!({
                "query": nullable_bounded_string("Optional text matched against persisted searchable metadata.", 200),
                "kind": {
                    "type": ["string", "null"],
                    "enum": ["video", "image", "audio", "other", null],
                    "description": "Optional persisted asset kind."
                },
                "minDurationMs": nullable_non_negative_integer("Optional minimum verified duration in milliseconds."),
                "maxDurationMs": nullable_non_negative_integer("Optional maximum verified duration in milliseconds."),
                "minRating": {
                    "type": ["integer", "null"],
                    "minimum": 0,
                    "maximum": 5,
                    "description": "Optional minimum user rating from 0 through 5."
                },
                "favoriteOnly": {
                    "type": "boolean",
                    "description": "Whether to return only user-favorited assets."
                },
                "tag": nullable_bounded_string("Optional exact user tag.", 200),
                "collectionId": nullable_bounded_string("Optional collection identifier resolved inside the current project.", 200),
                "offset": bounded_page_integer("Zero-based result offset.", 0, 10_000),
                "limit": bounded_page_integer("Maximum candidates to return.", 1, 20)
            }),
            vec![
                "query",
                "kind",
                "minDurationMs",
                "maxDurationMs",
                "minRating",
                "favoriteOnly",
                "tag",
                "collectionId",
                "offset",
                "limit",
            ],
        ),
        function_tool(
            SEARCH_ASSET_SEGMENTS,
            "Search verified scene and source-time evidence within current-project video or image assets.",
            json!({
                "query": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 200,
                    "description": "Required evidence search text."
                },
                "assetId": nullable_bounded_string("Optional asset identifier resolved inside the current project.", 200),
                "offset": bounded_page_integer("Zero-based result offset.", 0, 10_000),
                "limit": bounded_page_integer("Maximum source ranges to return.", 1, 20)
            }),
            vec!["query", "assetId", "offset", "limit"],
        ),
        function_tool(
            SEARCH_MUSIC,
            "Search the configured Jamendo catalog for downloadable CC0 or CC-BY tracks without downloading them.",
            json!({
                "query": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 200,
                    "description": "Music title, artist, mood, or style search text."
                }
            }),
            vec!["query"],
        ),
        function_tool(
            GET_STORYBOARD,
            "Read the current scoped storyboard version and its evidence-bound shot selections, if present.",
            json!({}),
            Vec::new(),
        ),
        function_tool(
            GET_TIMELINE,
            "Read a scoped timeline snapshot, optionally selecting a timeline version.",
            json!({
                "timelineVersionId": {
                    "type": ["string", "null"],
                    "description": "Optional timeline version identifier; use null to select the scoped default."
                }
            }),
            vec!["timelineVersionId"],
        ),
        function_tool(
            GET_TEXT_CAPABILITIES,
            "Read backend-verified local preview and Jianying text capability recipes.",
            json!({}),
            Vec::new(),
        ),
    ];
    if include_render_preview {
        tools.push(function_tool(
            RENDER_PREVIEW,
            "Render a low-resolution local preview from the current project's selected timeline.",
            json!({
                "timelineVersionId": {
                    "type": ["string", "null"],
                    "description": "Optional scoped timeline version identifier; use null to select the current timeline."
                }
            }),
            vec!["timelineVersionId"],
        ));
    }
    tools
}

fn nullable_bounded_string(description: &str, max_length: usize) -> Value {
    json!({
        "type": ["string", "null"],
        "minLength": 1,
        "maxLength": max_length,
        "description": description
    })
}

fn nullable_non_negative_integer(description: &str) -> Value {
    json!({
        "type": ["integer", "null"],
        "minimum": 0,
        "description": description
    })
}

fn bounded_page_integer(description: &str, minimum: usize, maximum: usize) -> Value {
    json!({
        "type": "integer",
        "minimum": minimum,
        "maximum": maximum,
        "description": description
    })
}

#[allow(dead_code)]
fn function_tool(name: &str, description: &str, properties: Value, required: Vec<&str>) -> Value {
    json!({
        "type": "function",
        "name": name,
        "description": description,
        "parameters": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        },
        "strict": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentloop::policy::{EDIT_TOOLS, OBSERVATION_TOOLS};
    use std::collections::HashSet;

    #[test]
    fn native_catalog_has_every_observation_tool_once_and_no_write_tool() {
        let tools = native_observation_function_tools();
        let names: HashSet<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect();

        assert_eq!(tools.len(), OBSERVATION_TOOLS.len());
        assert_eq!(names.len(), tools.len());
        assert!(OBSERVATION_TOOLS.iter().all(|name| names.contains(name)));
        assert!(EDIT_TOOLS.iter().all(|name| !names.contains(name)));
    }

    #[test]
    fn every_tool_uses_strict_closed_schema_with_complete_required_keys() {
        for tool in native_observation_function_tools() {
            assert_eq!(tool["type"], "function");
            assert_eq!(tool["strict"], true);
            assert!(tool["description"]
                .as_str()
                .is_some_and(|text| !text.is_empty()));

            let parameters = &tool["parameters"];
            assert_eq!(parameters["type"], "object");
            assert_eq!(parameters["additionalProperties"], false);
            let properties = parameters["properties"]
                .as_object()
                .expect("properties object");
            let required = parameters["required"].as_array().expect("required array");
            let required_names: HashSet<&str> = required
                .iter()
                .map(|name| name.as_str().expect("required name"))
                .collect();

            assert_eq!(required_names.len(), required.len());
            assert_eq!(required_names.len(), properties.len());
            assert!(required_names
                .iter()
                .all(|name| properties.contains_key(*name)));

            let definition = tool.to_string();
            for forbidden in ["projectId", "conversationId", "sourcePath", "localPath"] {
                assert!(!definition.contains(forbidden));
            }
        }
    }

    #[test]
    fn optional_timeline_selector_is_nullable_and_scope_free() {
        let tool = native_observation_function_tools()
            .into_iter()
            .find(|tool| tool["name"] == GET_TIMELINE)
            .expect("get_timeline tool");
        let property = &tool["parameters"]["properties"]["timelineVersionId"];
        assert_eq!(property["type"], json!(["string", "null"]));
        assert_eq!(tool["parameters"]["required"], json!(["timelineVersionId"]));
    }

    #[test]
    fn bounded_search_schemas_are_strict_and_express_optional_values_as_nullable() {
        let tools = native_observation_function_tools();
        let find = |name: &str| {
            tools
                .iter()
                .find(|tool| tool["name"] == name)
                .expect("native observation tool")
        };

        let asset_search = find("search_assets");
        assert_eq!(
            asset_search["parameters"]["required"]
                .as_array()
                .map(Vec::len),
            Some(10)
        );
        assert_eq!(
            asset_search["parameters"]["properties"]["kind"]["enum"],
            json!(["video", "image", "audio", "other", null])
        );
        assert_eq!(
            asset_search["parameters"]["properties"]["query"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(
            asset_search["parameters"]["properties"]["limit"]["maximum"],
            20
        );
        assert_eq!(
            asset_search["parameters"]["properties"]["offset"]["maximum"],
            10_000
        );

        let segment_search = find("search_asset_segments");
        assert_eq!(
            segment_search["parameters"]["required"],
            json!(["query", "assetId", "offset", "limit"])
        );
        assert_eq!(
            segment_search["parameters"]["properties"]["assetId"]["type"],
            json!(["string", "null"])
        );

        let music_search = find("search_music");
        assert_eq!(music_search["parameters"]["required"], json!(["query"]));
        assert_eq!(
            music_search["parameters"]["properties"]["query"]["minLength"],
            1
        );
        assert_eq!(
            music_search["parameters"]["properties"]["query"]["maxLength"],
            200
        );
    }

    #[test]
    fn render_preview_schema_is_strict_and_scope_free() {
        let tool = native_function_tools(true)
            .into_iter()
            .find(|tool| tool["name"] == RENDER_PREVIEW)
            .expect("render_preview tool");
        assert_eq!(tool["strict"], true);
        assert_eq!(tool["parameters"]["additionalProperties"], false);
        assert_eq!(tool["parameters"]["required"], json!(["timelineVersionId"]));
        assert_eq!(
            tool["parameters"]["properties"]["timelineVersionId"]["type"],
            json!(["string", "null"])
        );
        let definition = tool.to_string();
        for forbidden in [
            "projectId",
            "conversationId",
            "sourcePath",
            "localPath",
            "ffmpeg",
        ] {
            assert!(!definition.contains(forbidden));
        }
    }

    #[test]
    fn render_preview_is_only_added_when_policy_allows_it() {
        assert_eq!(native_function_tools(false).len(), OBSERVATION_TOOLS.len());
        assert_eq!(
            native_function_tools(true).len(),
            OBSERVATION_TOOLS.len() + 1
        );
    }

    #[test]
    fn first_batch_names_remain_backed_by_existing_apply_skill_allowlist() {
        for tool in native_observation_function_tools() {
            let name = tool["name"].as_str().expect("tool name");
            assert!(OBSERVATION_TOOLS.contains(&name));
        }
    }
}
