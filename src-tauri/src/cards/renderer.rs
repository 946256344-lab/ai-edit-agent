//! 本地品牌卡渲染器：独立线程持有一个隐藏窗口里的 WebView2，把模板页面截成透明 PNG。
//! 不占用 Tauri 主线程，不联网（模板 CSP + 虚拟主机只映射本机目录）；空闲一分钟自动释放。
//! 截图走 DevTools 协议：固定视口与缩放系数、透明背景，输出尺寸与请求不符即判失败。
#![cfg(windows)]

use base64::Engine;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, Microsoft::Web::WebView2::Win32::*,
    NavigationCompletedEventHandler,
};
use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::{Com::*, LibraryLoader},
        UI::WindowsAndMessaging::{self, MSG, WNDCLASSW},
    },
};

use super::BRAND_HOST;

const CARD_HOST: &str = "card.voycut.local";
const STEP_TIMEOUT: Duration = Duration::from_secs(20);
const CALLER_TIMEOUT: Duration = Duration::from_secs(45);
const IDLE_SHUTDOWN: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct RenderJob {
    /// card 虚拟主机映射的目录与其中的页面文件名。
    pub card_dir: PathBuf,
    pub page_name: String,
    /// brand 虚拟主机映射的目录（项目品牌套件文件）。
    pub brand_dir: PathBuf,
    pub width: u32,
    pub height: u32,
}

type Reply = mpsc::Sender<Result<Vec<u8>, String>>;

struct Worker {
    id: u64,
    tx: mpsc::Sender<(RenderJob, Reply)>,
}

static NEXT_WORKER_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

static WORKER: Mutex<Option<Worker>> = Mutex::new(None);

enum Attempt {
    Done(Result<Vec<u8>, String>),
    /// 工作线程恰好因空闲退出，任务没被处理，可以换新线程重发。
    WorkerGone,
}

/// 渲染一张卡片，返回 PNG 字节。工作线程失联时换新线程重发一次，超时如实报错。
pub(crate) fn render_png(user_data_dir: &Path, job: RenderJob) -> Result<Vec<u8>, String> {
    #[cfg(feature = "footage-eval")]
    if crate::footage_eval::trace_directory().is_some() {
        return Err("eval_card_webview_disabled".to_owned());
    }
    match attempt(user_data_dir, job.clone()) {
        Attempt::Done(result) => result,
        Attempt::WorkerGone => match attempt(user_data_dir, job) {
            Attempt::Done(result) => result,
            Attempt::WorkerGone => Err("The local card renderer could not start.".to_owned()),
        },
    }
}

/// 只摘掉指定线程的句柄：卡住后被替换的旧线程稍后空闲退出时，不能误删新线程。
fn forget_worker(id: Option<u64>) {
    if let Ok(mut worker) = WORKER.lock() {
        if id.is_none() || worker.as_ref().map(|current| current.id) == id {
            *worker = None;
        }
    }
}

fn attempt(user_data_dir: &Path, job: RenderJob) -> Attempt {
    let (reply_tx, reply_rx) = mpsc::channel();
    let worker_id;
    {
        let Ok(mut worker) = WORKER.lock() else {
            return Attempt::Done(Err("Card renderer is unavailable.".to_owned()));
        };
        let current = worker.get_or_insert_with(|| spawn_worker(user_data_dir.to_path_buf()));
        worker_id = current.id;
        if current.tx.send((job, reply_tx)).is_err() {
            *worker = None;
            return Attempt::WorkerGone;
        }
    }
    match reply_rx.recv_timeout(CALLER_TIMEOUT) {
        Ok(Err(message)) if message == WORKER_STOPPED => Attempt::WorkerGone,
        Ok(result) => Attempt::Done(result),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            forget_worker(Some(worker_id));
            Attempt::Done(Err("The local card renderer timed out.".to_owned()))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            forget_worker(Some(worker_id));
            Attempt::WorkerGone
        }
    }
}

const WORKER_STOPPED: &str = "The card renderer stopped.";

fn spawn_worker(user_data_dir: PathBuf) -> Worker {
    let (tx, rx) = mpsc::channel::<(RenderJob, Reply)>();
    let id = NEXT_WORKER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::thread::Builder::new()
        .name("card-renderer".to_owned())
        .spawn(move || worker_main(id, user_data_dir, rx))
        .expect("spawn card renderer thread");
    Worker { id, tx }
}

fn worker_main(id: u64, user_data_dir: PathBuf, rx: mpsc::Receiver<(RenderJob, Reply)>) {
    if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
        forget_worker(Some(id));
        drain(&rx, "The card renderer could not initialize COM.");
        return;
    }
    let host = match Host::create(&user_data_dir) {
        Ok(host) => host,
        Err(error) => {
            log::warn!("Card renderer could not start WebView2: {error}");
            forget_worker(Some(id));
            drain(&rx, &format!("WebView2 is unavailable for card rendering: {error}"));
            unsafe { CoUninitialize() };
            return;
        }
    };
    let mut last_used = Instant::now();
    loop {
        match rx.try_recv() {
            Ok((job, reply)) => {
                let started = Instant::now();
                let result = host.render(&job);
                log::info!(
                    "Card render {}x{} finished in {} ms ({}).",
                    job.width,
                    job.height,
                    started.elapsed().as_millis(),
                    if result.is_ok() { "ok" } else { "failed" }
                );
                let _ = reply.send(result);
                last_used = Instant::now();
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {
                pump_pending_messages();
                if last_used.elapsed() > IDLE_SHUTDOWN {
                    // 先摘掉全局句柄，再退出；之后到达的调用会新建线程。
                    forget_worker(Some(id));
                    drain(&rx, WORKER_STOPPED);
                    break;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    }
    drop(host);
    unsafe { CoUninitialize() };
}

fn drain(rx: &mpsc::Receiver<(RenderJob, Reply)>, message: &str) {
    while let Ok((_, reply)) = rx.try_recv() {
        let _ = reply.send(Err(message.to_owned()));
    }
}

fn pump_pending_messages() {
    let mut message = MSG::default();
    unsafe {
        while WindowsAndMessaging::PeekMessageW(&mut message, None, 0, 0, WindowsAndMessaging::PM_REMOVE).as_bool() {
            let _ = WindowsAndMessaging::TranslateMessage(&message);
            WindowsAndMessaging::DispatchMessageW(&message);
        }
    }
}

/// WebView2 回调都在本线程分发；等待期间持续抽消息，超过期限如实失败，不无限阻塞。
fn pump_until<T>(rx: &mpsc::Receiver<T>, step: &str) -> Result<T, String> {
    let deadline = Instant::now() + STEP_TIMEOUT;
    loop {
        if let Ok(value) = rx.try_recv() {
            return Ok(value);
        }
        if Instant::now() > deadline {
            return Err(format!("{step} timed out"));
        }
        pump_pending_messages();
        std::thread::sleep(Duration::from_millis(4));
    }
}

extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, message, wparam, lparam) }
}

struct Host {
    hwnd: HWND,
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            let _ = self.controller.Close();
            let _ = WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Host {
    fn create(user_data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(user_data_dir).map_err(|error| error.to_string())?;
        let class_name = wide("VoycutCardRenderer");
        let hwnd = unsafe {
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            WindowsAndMessaging::RegisterClassW(&class);
            WindowsAndMessaging::CreateWindowExW(
                WindowsAndMessaging::WS_EX_TOOLWINDOW | WindowsAndMessaging::WS_EX_NOACTIVATE,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WindowsAndMessaging::WS_POPUP,
                -32000,
                -32000,
                64,
                64,
                None,
                None,
                LibraryLoader::GetModuleHandleW(None).ok().map(|handle| HINSTANCE(handle.0)),
                None,
            )
            .map_err(|error| error.to_string())?
        };
        let user_data = wide(&user_data_dir.to_string_lossy());
        let (env_tx, env_rx) = mpsc::channel();
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |code, environment| {
            let _ = env_tx.send(code.and_then(|_| environment.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))));
            Ok(())
        }));
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), PCWSTR(user_data.as_ptr()), None, &handler)
                .map_err(|error| error.to_string())?;
        }
        let environment = pump_until(&env_rx, "WebView2 environment")?.map_err(|error| error.to_string())?;
        let (controller_tx, controller_rx) = mpsc::channel();
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |code, controller| {
            let _ = controller_tx.send(code.and_then(|_| controller.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))));
            Ok(())
        }));
        unsafe {
            environment
                .CreateCoreWebView2Controller(hwnd, &handler)
                .map_err(|error| error.to_string())?;
        }
        let controller = pump_until(&controller_rx, "WebView2 controller")?.map_err(|error| error.to_string())?;
        let webview = unsafe {
            controller.SetIsVisible(true).map_err(|error| error.to_string())?;
            if let Ok(controller2) = controller.cast::<ICoreWebView2Controller2>() {
                controller2
                    .SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 0, R: 0, G: 0, B: 0 })
                    .map_err(|error| error.to_string())?;
            }
            if let Ok(controller3) = controller.cast::<ICoreWebView2Controller3>() {
                let _ = controller3.SetShouldDetectMonitorScaleChanges(false);
                let _ = controller3.SetRasterizationScale(1.0);
            }
            let webview = controller.CoreWebView2().map_err(|error| error.to_string())?;
            if let Ok(settings) = webview.Settings() {
                let _ = settings.SetAreDefaultContextMenusEnabled(false);
                let _ = settings.SetIsStatusBarEnabled(false);
                let _ = settings.SetAreHostObjectsAllowed(false);
                let _ = settings.SetIsWebMessageEnabled(false);
            }
            webview
        };
        Ok(Self { hwnd, controller, webview })
    }

    fn map_host(&self, host: &str, folder: &Path) -> Result<(), String> {
        let webview3: ICoreWebView2_3 = self.webview.cast().map_err(|error| error.to_string())?;
        let host = wide(host);
        let folder = wide(&folder.to_string_lossy());
        unsafe {
            let _ = webview3.ClearVirtualHostNameToFolderMapping(PCWSTR(host.as_ptr()));
            webview3
                .SetVirtualHostNameToFolderMapping(
                    PCWSTR(host.as_ptr()),
                    PCWSTR(folder.as_ptr()),
                    COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS,
                )
                .map_err(|error| error.to_string())
        }
    }

    fn cdp(&self, method: &str, params: &str) -> Result<serde_json::Value, String> {
        let (tx, rx) = mpsc::channel();
        let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |code, result| {
            let _ = tx.send(code.map(|_| result));
            Ok(())
        }));
        let method_w = wide(method);
        let params_w = wide(params);
        unsafe {
            self.webview
                .CallDevToolsProtocolMethod(PCWSTR(method_w.as_ptr()), PCWSTR(params_w.as_ptr()), &handler)
                .map_err(|error| error.to_string())?;
        }
        let raw = pump_until(&rx, method)?.map_err(|error| format!("{method}: {error}"))?;
        serde_json::from_str(&raw).map_err(|error| format!("{method}: {error}"))
    }

    fn render(&self, job: &RenderJob) -> Result<Vec<u8>, String> {
        self.map_host(CARD_HOST, &job.card_dir)?;
        self.map_host(BRAND_HOST, &job.brand_dir)?;
        unsafe {
            self.controller
                .SetBounds(RECT { left: 0, top: 0, right: job.width as i32, bottom: job.height as i32 })
                .map_err(|error| error.to_string())?;
        }
        let (tx, rx) = mpsc::channel();
        let handler = NavigationCompletedEventHandler::create(Box::new(move |_sender, args| {
            let succeeded = args
                .map(|args| {
                    let mut ok = windows::core::BOOL::default();
                    unsafe { args.IsSuccess(&mut ok) }.map(|_| ok.as_bool()).unwrap_or(false)
                })
                .unwrap_or(false);
            let _ = tx.send(succeeded);
            Ok(())
        }));
        let mut token = 0_i64;
        let url = wide(&format!("https://{CARD_HOST}/{}", job.page_name));
        unsafe {
            self.webview
                .add_NavigationCompleted(&handler, &mut token)
                .map_err(|error| error.to_string())?;
        }
        let navigated = unsafe { self.webview.Navigate(PCWSTR(url.as_ptr())) }
            .map_err(|error| error.to_string())
            .and_then(|_| pump_until(&rx, "Card page load"));
        unsafe {
            let _ = self.webview.remove_NavigationCompleted(token);
        }
        if !navigated? {
            return Err("The card page did not load.".to_owned());
        }
        self.cdp(
            "Emulation.setDeviceMetricsOverride",
            &format!(r#"{{"width":{},"height":{},"deviceScaleFactor":1,"mobile":false}}"#, job.width, job.height),
        )?;
        self.cdp(
            "Emulation.setDefaultBackgroundColorOverride",
            r#"{"color":{"r":0,"g":0,"b":0,"a":0}}"#,
        )?;
        let ready = self.cdp(
            "Runtime.evaluate",
            r#"{"expression":"Promise.resolve(window.__cardReady).then(r => JSON.stringify(r || {ok:false, failures:['runtime']}))","awaitPromise":true,"returnByValue":true}"#,
        )?;
        if ready.get("exceptionDetails").is_some() {
            return Err("The card template raised a script error.".to_owned());
        }
        let status: serde_json::Value = ready["result"]["value"]
            .as_str()
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default();
        if status["ok"] != true {
            let failures = status["failures"]
                .as_array()
                .map(|items| items.iter().filter_map(|item| item.as_str()).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            return Err(format!("The card could not load its brand assets ({failures})."));
        }
        let shot = self.cdp(
            "Page.captureScreenshot",
            &format!(
                r#"{{"format":"png","fromSurface":true,"clip":{{"x":0,"y":0,"width":{},"height":{},"scale":1}}}}"#,
                job.width, job.height
            ),
        )?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(shot["data"].as_str().unwrap_or_default())
            .map_err(|_| "The card screenshot was not a valid image.".to_owned())?;
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .map_err(|_| "The card screenshot was not a valid PNG.".to_owned())?;
        if decoded.width() != job.width || decoded.height() != job.height {
            return Err(format!(
                "The card rendered at {}x{} instead of {}x{}.",
                decoded.width(),
                decoded.height(),
                job.width,
                job.height
            ));
        }
        Ok(bytes)
    }
}
