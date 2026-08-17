//! 桌面后端入口与本地可信边界。
//!
//! 阅读一次请求时，从 [`taskrouter`]（任务归属）进入 [`agent`]（会话/运行生命周期），
//! 再到 [`agentloop`]（有界技能循环）。下列模块共同组成可信边界：每个模块只能为自己的
//! 职责访问 SQLite、源媒体、外部进程或产物；`run` 注册前端 bridge 使用的稳定 Tauri 命令面。

/// 对话路由、异步 Agent task 生命周期与原子终态提交。
mod agent;
/// 有界目标循环、请求策略、状态快照、prompt 与技能派发。
mod agentloop;
/// 素材导入、分析 worker、安全素材库投影、健康与恢复。
mod assets;
/// 不含 payload 的 Agent 步骤、诊断、任务状态与操作记录。
mod audit;
/// Storyboard 确认后的自动化流程：timeline + preview 依次执行。
mod confirmation;
/// 自定义 OpenAI 兼容凭据命令与配置。
mod custom_api;
/// SQLite 位置、连接策略与只追加 schema 迁移。
mod db;
/// 单向 Jianying draft 创建与延迟注册。
mod jianying;
/// 可序列化领域/Tauri 边界类型；本模块不放持久化行为。
mod models;
/// Jamendo 凭据、搜索、授权资格与有界下载适配器。
mod music_provider;
/// 实验性 loopback PKCE 流程与 Windows Credential Manager 访问。
mod oauth;
/// FFmpeg preview 渲染、文字/音乐合成与质量检查。
mod preview;
/// 隐藏 Windows 子进程的创建与有界执行。
mod process;
/// Project/task/conversation/message 命令与启动恢复协调。
mod projects;
/// 可替换模型传输、优先级门、超时、回退与熔断。
mod provider;
/// 基于证据的 storyboard 提案、校验、版本与查询。
mod storyboard;
/// 项目内任务归属、快照、pending route 与一次性 receipt。
mod taskrouter;
/// 内部 timeline 创建、校验编辑、文字/音乐轨与版本查询。
mod timeline;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            oauth::get_experimental_openai_oauth_status,
            oauth::start_experimental_openai_oauth,
            oauth::clear_experimental_openai_oauth,
            custom_api::get_custom_api_status,
            custom_api::save_custom_api,
            custom_api::clear_custom_api,
            music_provider::get_jamendo_status,
            music_provider::save_jamendo_client_id,
            projects::initialize_local_store,
            projects::create_project,
            projects::list_projects,
            projects::create_editing_session,
            projects::list_editing_sessions,
            projects::create_editing_task,
            projects::list_editing_tasks,
            projects::update_editing_task_brief,
            projects::create_conversation,
            projects::list_conversations,
            projects::create_message,
            projects::set_conversation_status,
            projects::list_messages,
            taskrouter::resolve_conversation_task,
            assets::import_assets,
            assets::import_asset_folder,
            assets::preview_asset_relink,
            assets::confirm_asset_relink,
            assets::preview_collect_project_media,
            assets::collect_project_media,
            assets::library::list_assets,
            assets::library::list_asset_page,
            assets::analysis::get_asset_task_center,
            assets::health::start_asset_health_scan,
            assets::health::cancel_asset_health_scan,
            assets::health::get_asset_health_scan_summary,
            assets::analysis::retry_asset_analysis_batch,
            assets::visual::skip_asset_visual_analysis_batch,
            assets::library::update_asset_user_metadata_batch,
            assets::library::add_asset_tag_batch,
            assets::library::remove_asset_tag_batch,
            assets::library::create_asset_collection,
            assets::library::list_asset_collections,
            assets::library::add_assets_to_collection,
            assets::library::get_asset_evidence,
            storyboard::generate_storyboard,
            storyboard::get_latest_storyboard,
            timeline::create_timeline_draft,
            timeline::get_latest_timeline,
            timeline::list_timeline_versions,
            audit::list_agent_tasks,
            audit::list_agent_run_steps,
            audit::list_agent_diagnostics,
            audit::list_operation_logs,
            preview::render_preview,
            jianying::create_jianying_draft,
            jianying::get_jianying_registration_status,
            agent::submit_conversation_turn,
            agent::execute_agent_edit,
            confirmation::confirm_storyboard_and_preview,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
