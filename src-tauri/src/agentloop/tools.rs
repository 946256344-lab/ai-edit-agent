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
const REQUEST_ASSET_ANALYSIS: &str = "request_asset_analysis";
const GENERATE_STORYBOARD: &str = "generate_storyboard";
const CREATE_TIMELINE_DRAFT: &str = "create_timeline_draft";
const REPLACE_CLIPS: &str = "replace_clips";
const CHANGE_CLIP_DURATION: &str = "change_clip_duration";
const REORDER_CLIPS: &str = "reorder_clips";
const REPLACE_TEXT_TRACKS: &str = "replace_text_tracks";
const DOWNLOAD_MUSIC: &str = "download_music";
const USE_ONLINE_MUSIC: &str = "use_online_music";
const REPLACE_MUSIC_TRACKS: &str = "replace_music_tracks";
const CREATE_JIANYING_DRAFT: &str = "create_jianying_draft";

/// 只读原生工具保持独立入口，供 NativeToolLoop 的默认观察集合复用。
#[allow(dead_code)]
pub(crate) fn native_observation_function_tools() -> Vec<Value> {
    native_function_tools(false)
}

pub(crate) fn native_function_tools(include_render_preview: bool) -> Vec<Value> {
    native_function_tools_for_request(include_render_preview, false)
}

pub(crate) fn native_function_tools_for_request(
    include_render_preview: bool,
    include_main_chain: bool,
) -> Vec<Value> {
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
    if include_main_chain {
        tools.extend(main_chain_function_tools());
    }
    tools
}

fn main_chain_function_tools() -> Vec<Value> {
    let mut tools = vec![
        function_tool(
            REQUEST_ASSET_ANALYSIS,
            "Request bounded analysis for selected imported assets in the current project.",
            json!({
                "assetIds": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 100,
                    "items": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 200
                    }
                }
            }),
            vec!["assetIds"],
        ),
        function_tool(
            GENERATE_STORYBOARD,
            "Generate an evidence-bound storyboard from analyzed media and the current editing brief.",
            json!({
                "brief": {
                    "type": ["string", "null"],
                    "minLength": 1,
                    "maxLength": 4_000,
                    "description": "Optional storyboard brief; null uses the current task brief."
                }
            }),
            vec!["brief"],
        ),
        function_tool(
            CREATE_TIMELINE_DRAFT,
            "Create a new scoped timeline version from the current storyboard.",
            json!({}),
            Vec::new(),
        ),
        function_tool(
            REPLACE_CLIPS,
            "Create a new timeline version by replacing clips with verified source ranges.",
            json!({
                "timelineVersionId": {
                    "type": ["string", "null"],
                    "description": "Optional scoped timeline version; null selects the current version."
                },
                "shots": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 100,
                    "items": {
                        "type": "object",
                        "properties": {
                            "shotIndex": {"type": "integer", "minimum": 0},
                            "assetId": {"type": "string", "minLength": 1, "maxLength": 200},
                            "sourceStartMs": {"type": "integer", "minimum": 0},
                            "sourceEndMs": {"type": "integer", "minimum": 0}
                        },
                        "required": ["shotIndex", "assetId", "sourceStartMs", "sourceEndMs"],
                        "additionalProperties": false
                    }
                }
            }),
            vec!["timelineVersionId", "shots"],
        ),
        function_tool(
            CHANGE_CLIP_DURATION,
            "Create a new timeline version with verified clip duration or source-start adjustments.",
            json!({
                "timelineVersionId": {
                    "type": ["string", "null"],
                    "description": "Optional scoped timeline version; null selects the current version."
                },
                "adjustments": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 100,
                    "items": {
                        "type": "object",
                        "properties": {
                            "shotIndex": {"type": "integer", "minimum": 0},
                            "newDurationMs": {"type": ["integer", "null"], "minimum": 1},
                            "newSourceStartMs": {"type": ["integer", "null"], "minimum": 0}
                        },
                        "required": ["shotIndex", "newDurationMs", "newSourceStartMs"],
                        "additionalProperties": false
                    }
                }
            }),
            vec!["timelineVersionId", "adjustments"],
        ),
        function_tool(
            REORDER_CLIPS,
            "Create a new timeline version using a complete order of the existing clips.",
            json!({
                "timelineVersionId": {
                    "type": ["string", "null"],
                    "description": "Optional scoped timeline version; null selects the current version."
                },
                "order": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 100,
                    "items": {"type": "integer", "minimum": 0}
                }
            }),
            vec!["timelineVersionId", "order"],
        ),
    ];
    tools.extend(delivery_function_tools());
    tools
}

fn delivery_function_tools() -> Vec<Value> {
    vec![
        function_tool(
            REPLACE_TEXT_TRACKS,
            "Create a scoped timeline version by replacing text tracks with backend-validated recipes.",
            json!({
                "timelineVersionId": nullable_timeline_version("Optional scoped timeline version; null selects the current version."),
                "textTracks": {
                    "type": "array", "minItems": 0, "maxItems": 21,
                    "items": text_track_schema()
                }
            }),
            vec!["timelineVersionId", "textTracks"],
        ),
        function_tool(
            DOWNLOAD_MUSIC,
            "Download exactly one eligible catalog music track into the current project for normal local analysis.",
            json!({
                "trackId": bounded_required_string_schema("Eligible catalog track identifier returned by search_music.", 200)
            }),
            vec!["trackId"],
        ),
        function_tool(
            USE_ONLINE_MUSIC,
            "Download one eligible catalog track, complete local analysis, and create a scoped timeline version using it.",
            json!({
                "trackId": bounded_required_string_schema("Eligible catalog track identifier returned by search_music.", 200),
                "timelineVersionId": nullable_timeline_version("Optional scoped timeline version; null selects the current version.")
            }),
            vec!["trackId", "timelineVersionId"],
        ),
        function_tool(
            REPLACE_MUSIC_TRACKS,
            "Create a scoped timeline version using ready local audio assets and validated music cues.",
            json!({
                "timelineVersionId": nullable_timeline_version("Optional scoped timeline version; null selects the current version."),
                "musicTracks": {
                    "type": "array", "minItems": 0, "maxItems": 100,
                    "items": music_track_schema()
                }
            }),
            vec!["timelineVersionId", "musicTracks"],
        ),
        function_tool(
            CREATE_JIANYING_DRAFT,
            "Create a new local Jianying draft from the selected scoped timeline without replacing an existing draft.",
            json!({
                "timelineVersionId": nullable_timeline_version("Optional scoped timeline version; null selects the current version.")
            }),
            vec!["timelineVersionId"],
        ),
    ]
}

fn nullable_timeline_version(description: &str) -> Value {
    json!({
        "type": ["string", "null"],
        "minLength": 1,
        "maxLength": 200,
        "description": description
    })
}

fn bounded_required_string_schema(description: &str, max_length: usize) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": max_length,
        "description": description
    })
}

fn text_track_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": bounded_required_string_schema("Stable text track identifier.", 200),
            "role": {"type": "string", "enum": ["subtitle", "headline", "callout", "cta", "label"]},
            "layer": {"type": "integer", "minimum": 0, "maximum": 20},
            "enabled": {"type": "boolean"},
            "cues": {"type": "array", "minItems": 0, "maxItems": 100, "items": text_cue_schema()}
        },
        "required": ["id", "role", "layer", "enabled", "cues"],
        "additionalProperties": false
    })
}

fn text_cue_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": bounded_required_string_schema("Stable text cue identifier.", 200),
            "templateId": {"type": ["string", "null"], "enum": ["subtitle_safe", "headline_rise", "headline_pop", "headline_drop", "callout_card", "cta_card", null]},
            "startMs": {"type": "integer", "minimum": 0},
            "endMs": {"type": "integer", "minimum": 1},
            "text": bounded_required_string_schema("Visible text content.", 280),
            "style": nullable_text_style_schema(),
            "layout": nullable_text_layout_schema(),
            "entrance": nullable_text_animation_schema(),
            "exit": nullable_text_animation_schema(),
            "loopAnimation": nullable_text_animation_schema()
        },
        "required": ["id", "templateId", "startMs", "endMs", "text", "style", "layout", "entrance", "exit", "loopAnimation"],
        "additionalProperties": false
    })
}

fn nullable_text_style_schema() -> Value {
    json!({
        "type": ["object", "null"],
        "properties": {
            "fontKey": bounded_required_string_schema("Verified local font key.", 200),
            "fontSize": {"type": "number", "minimum": 0.01, "maximum": 0.30},
            "bold": {"type": "boolean"},
            "color": {"type": "string", "pattern": "^#[0-9A-Fa-f]{6}$"},
            "strokeColor": {"type": ["string", "null"], "pattern": "^#[0-9A-Fa-f]{6}$"},
            "strokeWidth": {"type": "number", "minimum": 0.0, "maximum": 10.0},
            "shadow": {"type": "boolean"},
            "backgroundColor": {"type": ["string", "null"], "pattern": "^#[0-9A-Fa-f]{6}$"},
            "alignment": {"type": "string", "enum": ["left", "center", "right"]},
            "letterSpacing": {"type": "integer", "minimum": -100, "maximum": 100},
            "lineSpacing": {"type": "integer", "minimum": -100, "maximum": 100}
        },
        "required": ["fontKey", "fontSize", "bold", "color", "strokeColor", "strokeWidth", "shadow", "backgroundColor", "alignment", "letterSpacing", "lineSpacing"],
        "additionalProperties": false
    })
}

fn nullable_text_layout_schema() -> Value {
    json!({
        "type": ["object", "null"],
        "properties": {
            "anchor": {"type": "string", "enum": ["top", "center", "bottom"]},
            "x": {"type": "number", "minimum": 0.0, "maximum": 1.0},
            "y": {"type": "number", "minimum": 0.0, "maximum": 1.0},
            "maxWidth": {"type": "number", "minimum": 0.20, "maximum": 1.0},
            "safeArea": {"type": "string", "enum": ["title_safe", "action_safe"]}
        },
        "required": ["anchor", "x", "y", "maxWidth", "safeArea"],
        "additionalProperties": false
    })
}

fn nullable_text_animation_schema() -> Value {
    json!({
        "type": ["object", "null"],
        "properties": {
            "templateId": {"type": "string", "enum": ["fade", "slide_up", "slide_down", "pop", "wipe"]},
            "durationMs": {"type": "integer", "minimum": 0},
            "intensity": {"type": "number", "minimum": 0.0, "maximum": 1.0}
        },
        "required": ["templateId", "durationMs", "intensity"],
        "additionalProperties": false
    })
}

fn music_track_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": bounded_required_string_schema("Stable music track identifier.", 200),
            "enabled": {"type": "boolean"},
            "cues": {"type": "array", "minItems": 0, "maxItems": 100, "items": music_cue_schema()}
        },
        "required": ["id", "enabled", "cues"],
        "additionalProperties": false
    })
}

fn music_cue_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": bounded_required_string_schema("Stable music cue identifier.", 200),
            "assetId": bounded_required_string_schema("Ready local audio asset identifier.", 200),
            "sourceStartMs": {"type": "integer", "minimum": 0},
            "sourceEndMs": {"type": "integer", "minimum": 1},
            "timelineStartMs": {"type": "integer", "minimum": 0},
            "timelineEndMs": {"type": "integer", "minimum": 1},
            "loopEnabled": {"type": ["boolean", "null"]},
            "volume": {"type": "number", "minimum": 0.0, "maximum": 2.0},
            "fadeInMs": {"type": ["integer", "null"], "minimum": 0},
            "fadeOutMs": {"type": ["integer", "null"], "minimum": 0}
        },
        "required": ["id", "assetId", "sourceStartMs", "sourceEndMs", "timelineStartMs", "timelineEndMs", "loopEnabled", "volume", "fadeInMs", "fadeOutMs"],
        "additionalProperties": false
    })
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
    fn native_write_catalog_includes_only_migrated_batches() {
        let tools = native_function_tools_for_request(false, true);
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), tools.len());
        for name in [
            "request_asset_analysis",
            "generate_storyboard",
            "create_timeline_draft",
            "replace_clips",
            "change_clip_duration",
            "reorder_clips",
            "replace_text_tracks",
            "download_music",
            "use_online_music",
            "replace_music_tracks",
            "create_jianying_draft",
        ] {
            assert!(names.contains(name), "missing {name}");
        }
    }

    #[test]
    fn main_chain_schemas_are_strict_closed_and_nullable_where_optional() {
        let tools = native_function_tools_for_request(false, true);
        for name in [
            "request_asset_analysis",
            "generate_storyboard",
            "create_timeline_draft",
            "replace_clips",
            "change_clip_duration",
            "reorder_clips",
        ] {
            let tool = tools
                .iter()
                .find(|tool| tool["name"] == name)
                .expect("main chain tool");
            assert_eq!(tool["strict"], true);
            assert_eq!(tool["parameters"]["additionalProperties"], false);
            let properties = tool["parameters"]["properties"]
                .as_object()
                .expect("properties");
            let required = tool["parameters"]["required"].as_array().expect("required");
            assert_eq!(properties.len(), required.len());
            assert!(required
                .iter()
                .all(|key| properties.contains_key(key.as_str().unwrap())));
            assert!(!tool.to_string().contains("projectId"));
            assert!(!tool.to_string().contains("sourcePath"));
        }

        let by_name = |name: &str| tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(
            by_name("generate_storyboard")["parameters"]["properties"]["brief"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(
            by_name("replace_clips")["parameters"]["properties"]["timelineVersionId"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(
            by_name("change_clip_duration")["parameters"]["properties"]["adjustments"]["items"]
                ["additionalProperties"],
            false
        );
        assert_eq!(
            by_name("change_clip_duration")["parameters"]["properties"]["adjustments"]["items"]
                ["properties"]["newDurationMs"]["type"],
            json!(["integer", "null"])
        );

        for name in [
            "replace_text_tracks",
            "replace_music_tracks",
            "download_music",
            "use_online_music",
            "create_jianying_draft",
        ] {
            let tool = by_name(name);
            assert_eq!(tool["strict"], true, "{name}");
            assert_eq!(tool["parameters"]["additionalProperties"], false, "{name}");
            assert!(!tool.to_string().contains("projectId"), "{name}");
            assert!(!tool.to_string().contains("localPath"), "{name}");
        }
        assert_eq!(
            by_name("use_online_music")["parameters"]["properties"]["timelineVersionId"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(
            by_name("replace_text_tracks")["parameters"]["properties"]["textTracks"]["items"]
                ["additionalProperties"],
            false
        );
        assert_eq!(
            by_name("replace_music_tracks")["parameters"]["properties"]["musicTracks"]["items"]
                ["additionalProperties"],
            false
        );
    }

    #[test]
    fn delivery_nested_schemas_are_closed_with_complete_required_keys() {
        let tools = native_function_tools_for_request(false, true);
        for name in ["replace_text_tracks", "replace_music_tracks"] {
            let tool = tools
                .iter()
                .find(|tool| tool["name"] == name)
                .expect("delivery tool");
            assert_closed_object_schema(&tool["parameters"]);
        }
    }

    fn assert_closed_object_schema(schema: &Value) {
        let is_object = schema["type"] == "object"
            || schema["type"]
                .as_array()
                .is_some_and(|types| types.iter().any(|kind| kind == "object"));
        if is_object {
            assert_eq!(schema["additionalProperties"], false);
            let properties = schema["properties"].as_object().expect("properties");
            let required = schema["required"].as_array().expect("required");
            let names = required
                .iter()
                .map(|name| name.as_str().expect("required name"))
                .collect::<HashSet<_>>();
            assert_eq!(names.len(), required.len());
            assert_eq!(names.len(), properties.len());
            for property in properties.values() {
                assert_closed_object_schema(property);
            }
        }
        if let Some(items) = schema.get("items") {
            assert_closed_object_schema(items);
        }
    }

    #[test]
    fn first_batch_names_remain_backed_by_existing_apply_skill_allowlist() {
        for tool in native_observation_function_tools() {
            let name = tool["name"].as_str().expect("tool name");
            assert!(OBSERVATION_TOOLS.contains(&name));
        }
    }
}
