//! 本地 stdio MCP 进程入口。无窗口、不启动桌面 GUI。
fn main() -> Result<(), String> {
    app_lib::mcp::run()
}
