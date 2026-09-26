// Tauri 构建脚本入口：把平台资源和权限配置交给 tauri-build 生成。
fn main() {
    // ONNX Runtime 静态库带 DirectML 导入；改为延迟加载，运行时先按完整路径载入随包新版
    // （见 src/onnx_device.rs），避免进程启动时就绑定到系统里过旧的 DirectML.dll。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg=/DELAYLOAD:DirectML.dll");
        println!("cargo:rustc-link-lib=delayimp");
    }
    tauri_build::build()
}
