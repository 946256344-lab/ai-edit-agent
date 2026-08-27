//! Agent 循环的纯策略层。
//!
//! 本模块只根据用户文本回答两个问题：本轮是否只读、本轮是否需要先观察项目状态。
//! 它不得持有数据库、文件系统、Tauri、Provider 或外部进程句柄，因此这里的判断
//! 本身不能产生任何副作用。

pub(super) use super::native_policy::request_requires_project_observation;

/// 不创建/修改本地产物的观察技能；`search_music` 是受控外部查询，其余只读本地状态。
pub(super) const OBSERVATION_TOOLS: &[&str] = &[
    "read_logs",
    "get_edit_status",
    "get_asset_health_summary",
    "list_assets",
    "search_assets",
    "search_asset_segments",
    "search_music",
    "list_voices",
    "get_storyboard",
    "get_timeline",
    "get_text_capabilities",
];

/// 会创建、下载或修改可审计产物的技能。只读请求会一次性关闭整组技能。
/// 仅供测试与工具契约使用；执行门由 `native_tool_call_allowed` 统一判断。
#[allow(dead_code)]
pub(super) const EDIT_TOOLS: &[&str] = &[
    "download_music",
    "use_online_music",
    "request_asset_analysis",
    "generate_storyboard",
    "create_timeline_draft",
    "replace_clips",
    "change_clip_duration",
    "reorder_clips",
    "replace_text_tracks",
    "replace_music_tracks",
    "synthesize_voiceover",
    "render_preview",
    "create_jianying_draft",
];

/// 用户请求的唯一策略维度：是否只读。
///
/// 不再解析负向关键词黑名单、敏感能力授权或期望写工具清单；模型默认获得全部
/// 工具，能否执行只由 `native_tool_call_allowed` 按 `read_only` 判断。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RequestToolPolicy {
    pub(super) read_only: bool,
}

impl RequestToolPolicy {
    pub(super) fn from_request(request: &str) -> Self {
        Self {
            read_only: request.contains('只') || request.to_lowercase().contains("only"),
        }
    }
}
