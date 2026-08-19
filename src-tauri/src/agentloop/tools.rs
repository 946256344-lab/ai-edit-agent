//! 原生 Function Tool 定义与 schema 合约。
//!
//! 本模块只拥有发送给 Provider 的稳定工具描述，不拥有工具执行权限；真实观察仍由
//! `skills::apply_skill` 负责，作用域由 LoopState 注入，避免模型参数伪造项目边界。

use serde_json::{json, Value};

#[allow(dead_code)]
const GET_ASSET_HEALTH_SUMMARY: &str = "get_asset_health_summary";
#[allow(dead_code)]
const LIST_ASSETS: &str = "list_assets";
#[allow(dead_code)]
const GET_TIMELINE: &str = "get_timeline";
#[allow(dead_code)]
const RENDER_PREVIEW: &str = "render_preview";

/// 第一批只读原生工具保持独立入口，供 NativeToolLoop 的默认观察集合复用。
#[allow(dead_code)]
pub(crate) fn native_observation_function_tools() -> Vec<Value> {
    native_function_tools(false)
}

pub(crate) fn native_function_tools(include_render_preview: bool) -> Vec<Value> {
    let mut tools = vec![
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
    use crate::agentloop::policy::OBSERVATION_TOOLS;
    use std::collections::HashSet;

    #[test]
    fn first_batch_has_unique_stable_names() {
        let tools = native_observation_function_tools();
        let names: HashSet<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect();

        assert_eq!(tools.len(), 3);
        assert_eq!(names.len(), tools.len());
        assert!(names.contains(GET_ASSET_HEALTH_SUMMARY));
        assert!(names.contains(LIST_ASSETS));
        assert!(names.contains(GET_TIMELINE));
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
        assert_eq!(native_function_tools(false).len(), 3);
        assert_eq!(native_function_tools(true).len(), 4);
    }

    #[test]
    fn first_batch_names_remain_backed_by_existing_apply_skill_allowlist() {
        for tool in native_observation_function_tools() {
            let name = tool["name"].as_str().expect("tool name");
            assert!(OBSERVATION_TOOLS.contains(&name));
        }
    }
}
