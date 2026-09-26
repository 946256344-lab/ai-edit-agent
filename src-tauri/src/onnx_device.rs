//! 本地 ONNX 推理设备：优先显卡（随包新版 DirectML），不可用时回退 CPU。
//! Windows 自带的 DirectML 版本过旧，推理时会报错，所以只在随包 DirectML.dll 按完整路径加载成功后才试显卡；
//! 显卡会话建好后先试算一次，失败即改用 CPU，日志写明每个模型最终用的设备。
//! DirectML.dll 在构建时设为延迟加载（见 build.rs），没有随包文件时进程不会去碰系统里的旧版本。
//! fastembed 会把一次调用拆成多批并行跑同一会话，DirectML 不支持并发执行，CPU 下也会互相抢核，
//! 所以调用方按模型串行推理、每次只交一批（`sequential_batches`）。

use ort::execution_providers::{
    CPUExecutionProvider, DirectMLExecutionProvider, ExecutionProviderDispatch,
};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};

const DIRECTML_DIRECTORY: &str = "directml";
const DIRECTML_FILE: &str = "DirectML.dll";

/// 先用显卡建会话并试算，失败回退 CPU。`build` 返回的错误只写日志，最终错误来自 CPU 路径。
pub(crate) fn load_with_fallback<T>(
    app: &AppHandle,
    label: &str,
    build: impl Fn(Vec<ExecutionProviderDispatch>) -> Result<T, String>,
    warm_up: impl Fn(&T) -> bool,
) -> Result<T, String> {
    if directml_loaded(app) {
        match build(vec![DirectMLExecutionProvider::default()
            .build()
            .error_on_failure()])
        {
            Ok(model) if warm_up(&model) => {
                log::info!("Local model {label}: DirectML GPU");
                return Ok(model);
            }
            Ok(_) => log::warn!("Local model {label}: DirectML warm-up failed; using CPU"),
            Err(error) => {
                log::warn!("Local model {label}: DirectML unavailable ({error}); using CPU")
            }
        }
    }
    let model = build(vec![CPUExecutionProvider::default().build()])?;
    log::info!("Local model {label}: CPU");
    Ok(model)
}

/// 同一模型串行推理，按固定大小顺序分批，每次调用只交一批。
pub(crate) fn sequential_batches<I, O>(
    lock: &Mutex<()>,
    items: &[I],
    batch_size: usize,
    mut run: impl FnMut(&[I]) -> Result<Vec<O>, String>,
) -> Result<Vec<O>, String> {
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outputs = Vec::with_capacity(items.len());
    for chunk in items.chunks(batch_size.max(1)) {
        outputs.extend(run(chunk)?);
    }
    Ok(outputs)
}

fn directml_loaded(app: &AppHandle) -> bool {
    static LOADED: OnceLock<bool> = OnceLock::new();
    *LOADED.get_or_init(|| preload_directml(app))
}

fn directml_candidates(app: &AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        paths.push(dir.join("resources").join(DIRECTML_DIRECTORY).join(DIRECTML_FILE));
        paths.push(dir.join(DIRECTML_DIRECTORY).join(DIRECTML_FILE));
    }
    #[cfg(debug_assertions)]
    paths.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join(DIRECTML_DIRECTORY)
            .join(DIRECTML_FILE),
    );
    paths
}

#[cfg(windows)]
fn preload_directml(app: &AppHandle) -> bool {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(
            name: *const u16,
            file: *mut std::ffi::c_void,
            flags: u32,
        ) -> *mut std::ffi::c_void;
    }
    const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;

    for path in directml_candidates(app) {
        if !path.is_file() {
            continue;
        }
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>();
        // 进程里已加载同名模块后，延迟导入按名字解析到这一份，不会再去 System32 找旧版。
        let module =
            unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
        if !module.is_null() {
            log::info!("Bundled DirectML loaded; local models will try the GPU");
            return true;
        }
        log::warn!("Bundled DirectML.dll could not be loaded; local models use CPU");
        return false;
    }
    log::info!("Bundled DirectML.dll not found; local models use CPU");
    false
}

#[cfg(not(windows))]
fn preload_directml(_app: &AppHandle) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_batches_hands_one_batch_per_call_in_order() {
        let lock = Mutex::new(());
        let mut calls = Vec::new();
        let out = sequential_batches(&lock, &[1, 2, 3, 4, 5], 2, |chunk| {
            calls.push(chunk.len());
            Ok(chunk.iter().map(|value| value * 10).collect())
        })
        .unwrap();
        assert_eq!(out, vec![10, 20, 30, 40, 50]);
        assert_eq!(calls, vec![2, 2, 1]);
    }
}
