// 第三方声明只读入口：固定随包路径，不接受前端文件路径，不读取用户数据。
use tauri::Manager;

#[tauri::command]
pub async fn get_third_party_notices(app: tauri::AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = if cfg!(debug_assertions) {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/third-party/ALL.txt")
        } else {
            app.path()
                .resource_dir()
                .map_err(|_| "notices_unavailable: resource directory unavailable".to_string())?
                .join("resources/third-party/ALL.txt")
        };
        std::fs::read_to_string(path)
            .map_err(|_| "notices_unavailable: bundled notices could not be read".to_string())
    })
    .await
    .map_err(|_| "notices_unavailable: reader failed".to_string())?
}
