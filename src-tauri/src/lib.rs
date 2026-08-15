//! Desktop backend and local trust boundary.
//!
//! Follow a user request from [`taskrouter`] (task ownership), through
//! [`agent`] (conversation/run lifecycle), into [`agentloop`] (bounded skill
//! selection). The Rust modules below together form the local trusted boundary:
//! each owning module may access SQLite, source media, external processes, or
//! deliverables only for its scoped responsibility. `run` registers the stable
//! Tauri command surface consumed by `src/lib/local-store.ts`.

/// Conversation routing, asynchronous Agent task lifecycle, and atomic finalization.
mod agent;
/// Bounded goal loop, request policy, state snapshots, prompts, and skill dispatch.
mod agentloop;
/// Asset import, analysis workers, safe library projections, health, and recovery.
mod assets;
/// Payload-free Agent steps, diagnostics, task status, and operation records.
mod audit;
/// Custom OpenAI-compatible credential commands and stored configuration.
mod custom_api;
/// SQLite location, connection policy, and append-only schema migrations.
mod db;
/// One-way Jianying draft creation and deferred registration.
mod jianying;
/// Serializable domain and Tauri boundary types; no persistence behavior belongs here.
mod models;
/// Jamendo credential, search, eligibility, and bounded download adapter.
mod music_provider;
/// Experimental loopback PKCE flow and Windows Credential Manager access.
mod oauth;
/// FFmpeg preview rendering, text/music composition, and quality inspection.
mod preview;
/// Hidden Windows child-process construction and bounded process execution.
mod process;
/// Project/task/conversation/message commands and startup recovery coordination.
mod projects;
/// Replaceable model transport, priority gate, timeout, fallback, and circuit breaker.
mod provider;
/// Evidence-backed storyboard proposal, validation, versioning, and lookup.
mod storyboard;
/// Project-level task resolution, snapshots, pending routes, and one-use receipts.
mod taskrouter;
/// Internal timeline creation, validated edits, text/music tracks, and version queries.
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
            assets::list_assets,
            assets::list_asset_page,
            assets::get_asset_task_center,
            assets::start_asset_health_scan,
            assets::cancel_asset_health_scan,
            assets::get_asset_health_scan_summary,
            assets::retry_asset_analysis_batch,
            assets::skip_asset_visual_analysis_batch,
            assets::update_asset_user_metadata_batch,
            assets::add_asset_tag_batch,
            assets::remove_asset_tag_batch,
            assets::create_asset_collection,
            assets::list_asset_collections,
            assets::add_assets_to_collection,
            assets::get_asset_evidence,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
