// 原生进程入口只启动 library crate；插件、命令注册和恢复流程都由 lib.rs 统一拥有。
// release 构建必须保留此属性，否则 Windows 会额外弹出控制台窗口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    app_lib::run();
}
