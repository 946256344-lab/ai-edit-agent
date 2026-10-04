//! 无窗口评测进程；不启动桌面应用或开发服务器。
fn main() -> Result<(), String> {
    app_lib::footage_eval::run()
}
