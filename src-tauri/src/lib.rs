mod oauth;
mod store;

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
            store::initialize_local_store,
            store::create_project,
            store::list_projects,
            store::create_editing_session,
            store::list_editing_sessions,
            store::create_editing_task,
            store::list_editing_tasks,
            store::update_editing_task_brief,
            store::create_conversation,
            store::list_conversations,
            store::create_message,
            store::set_conversation_status,
            store::list_messages,
            store::import_assets,
            store::import_asset_folder,
            store::list_assets,
            store::get_asset_evidence,
            store::generate_storyboard,
            store::get_latest_storyboard,
            store::create_timeline_draft,
            store::get_latest_timeline,
            store::render_preview,
            store::create_jianying_draft,
            store::get_jianying_registration_status,
            store::execute_agent_edit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
