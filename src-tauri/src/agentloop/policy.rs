//! Agent 循环的纯策略层。
//!
//! 工具可见性与执行授权不再解析用户文本关键词；意图由模型选择工具，
//! Rust 只提供观察/编辑工具名清单供白名单与契约使用。
//! 本模块不得持有数据库、文件系统、Tauri、Provider 或外部进程句柄。

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
    "transcribe_asset",
];

/// 会创建、下载或修改可审计产物的技能。
/// 仅供测试与工具契约使用；执行门由全局白名单与领域校验负责。
#[allow(dead_code)]
pub(super) const EDIT_TOOLS: &[&str] = &[
    "download_music",
    "use_online_music",
    "request_asset_analysis",
    "retry_failed_asset_analysis",
    "generate_storyboard",
    "create_timeline_draft",
    "replace_clips",
    "insert_clips",
    "change_clip_duration",
    "reorder_clips",
    "replace_text_tracks",
    "replace_music_tracks",
    "synthesize_voiceover",
    "render_preview",
    "create_jianying_draft",
];
