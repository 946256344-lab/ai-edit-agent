//! 发行安装包瘦身后的本地模型补齐：把 BGE/CLIP 的 ONNX 大文件下到 app_data，校验哈希后供选镜加载。
//!
//! 下载在后台进行，不阻塞工作台；缺失时选镜降级。小配置/tokenizer 仍随安装包分发。
//! 默认先走 HuggingFace 官方，失败则换国内镜像并断点续传、自动重试；完整安装包可捆绑 ONNX。

use crate::outbound_http;
use crate::storyboard::{clip, semantic};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

const PROGRESS_EVENT: &str = "runtime-model-progress";
const RUNTIME_MODELS_DIR: &str = "runtime-models";
const PROGRESS_EMIT_EVERY_BYTES: u64 = 2 * 1024 * 1024;
/// 单次 HTTP 尝试上限（含读体）。大 ONNX + 慢网下 60s 会稳定失败。
const DOWNLOAD_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// 每个产物最多尝试次数（含官方与镜像轮换）。
const MAX_DOWNLOAD_ATTEMPTS: u32 = 10;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(2);
const HF_HOST: &str = "https://huggingface.co/";
/// 国内可达的 HuggingFace 内容镜像；同一路径、同一 SHA，不是换模型源。
const HF_MIRROR_HOST: &str = "https://hf-mirror.com/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactId {
    BgeOnnx,
    ClipVisionOnnx,
    ClipTextOnnx,
}

impl ArtifactId {
    fn as_str(self) -> &'static str {
        match self {
            Self::BgeOnnx => "bge_onnx",
            Self::ClipVisionOnnx => "clip_vision_onnx",
            Self::ClipTextOnnx => "clip_text_onnx",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::BgeOnnx => "中文语义模型",
            Self::ClipVisionOnnx => "CLIP 视觉模型",
            Self::ClipTextOnnx => "CLIP 文本模型",
        }
    }

    fn relative_path(self) -> &'static str {
        match self {
            Self::BgeOnnx => "bge-small-zh-v1.5/onnx/model.onnx",
            Self::ClipVisionOnnx => "clip-ViT-B-32-vision/model.onnx",
            Self::ClipTextOnnx => "clip-ViT-B-32-text/model.onnx",
        }
    }

    fn canonical_url(self) -> &'static str {
        match self {
            Self::BgeOnnx => {
                "https://huggingface.co/Xenova/bge-small-zh-v1.5/resolve/main/onnx/model.onnx"
            }
            Self::ClipVisionOnnx => {
                "https://huggingface.co/Qdrant/clip-ViT-B-32-vision/resolve/main/model.onnx"
            }
            Self::ClipTextOnnx => {
                "https://huggingface.co/Qdrant/clip-ViT-B-32-text/resolve/main/model.onnx"
            }
        }
    }

    fn download_urls(self) -> [&'static str; 2] {
        let canonical = self.canonical_url();
        let mirror = match self {
            Self::BgeOnnx => {
                "https://hf-mirror.com/Xenova/bge-small-zh-v1.5/resolve/main/onnx/model.onnx"
            }
            Self::ClipVisionOnnx => {
                "https://hf-mirror.com/Qdrant/clip-ViT-B-32-vision/resolve/main/model.onnx"
            }
            Self::ClipTextOnnx => {
                "https://hf-mirror.com/Qdrant/clip-ViT-B-32-text/resolve/main/model.onnx"
            }
        };
        debug_assert!(canonical.starts_with(HF_HOST));
        debug_assert!(mirror.starts_with(HF_MIRROR_HOST));
        [canonical, mirror]
    }

    fn sha256(self) -> &'static str {
        match self {
            Self::BgeOnnx => semantic::MODEL_SHA256,
            Self::ClipVisionOnnx => clip::VISION_MODEL_SHA256,
            Self::ClipTextOnnx => clip::TEXT_MODEL_SHA256,
        }
    }

    fn model_leaf(self) -> &'static str {
        match self {
            Self::BgeOnnx => "bge-small-zh-v1.5",
            Self::ClipVisionOnnx => "clip-ViT-B-32-vision",
            Self::ClipTextOnnx => "clip-ViT-B-32-text",
        }
    }
}

const ARTIFACTS: [ArtifactId; 3] = [
    ArtifactId::BgeOnnx,
    ArtifactId::ClipVisionOnnx,
    ArtifactId::ClipTextOnnx,
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelArtifactStatus {
    pub id: String,
    pub title: String,
    pub state: String,
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelStatus {
    pub overall: String,
    pub current_id: Option<String>,
    pub message: String,
    /// 稳定文案键：前端按界面语言翻译；中文 message 保留作回落与日志。
    pub message_key: String,
    pub message_params: BTreeMap<String, String>,
    pub artifacts: Vec<RuntimeModelArtifactStatus>,
}

#[derive(Debug, Clone)]
struct ArtifactProgress {
    state: String,
    bytes_downloaded: u64,
    bytes_total: Option<u64>,
    error: Option<String>,
}

impl Default for ArtifactProgress {
    fn default() -> Self {
        Self {
            state: "pending".to_owned(),
            bytes_downloaded: 0,
            bytes_total: None,
            error: None,
        }
    }
}

#[derive(Debug)]
struct DownloadState {
    overall: String,
    current: Option<ArtifactId>,
    message: String,
    message_key: String,
    message_params: BTreeMap<String, String>,
    artifacts: [ArtifactProgress; 3],
    worker_running: bool,
}

impl DownloadState {
    /// 同时写中文回落文案与界面文案键；参数里的模型用 id，由前端翻成当前语言的名称。
    fn say(&mut self, key: &str, params: &[(&str, String)], message: String) {
        self.message = message;
        self.message_key = key.to_owned();
        self.message_params = params
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect();
    }
}

impl Default for DownloadState {
    fn default() -> Self {
        Self {
            overall: "idle".to_owned(),
            current: None,
            message: String::new(),
            message_key: String::new(),
            message_params: BTreeMap::new(),
            artifacts: [
                ArtifactProgress::default(),
                ArtifactProgress::default(),
                ArtifactProgress::default(),
            ],
            worker_running: false,
        }
    }
}

fn state_lock() -> &'static Mutex<DownloadState> {
    static STATE: OnceLock<Mutex<DownloadState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(DownloadState::default()))
}

fn artifact_index(id: ArtifactId) -> usize {
    match id {
        ArtifactId::BgeOnnx => 0,
        ArtifactId::ClipVisionOnnx => 1,
        ArtifactId::ClipTextOnnx => 2,
    }
}

pub(crate) fn runtime_models_root(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(RUNTIME_MODELS_DIR);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

/// 解析模型目录：优先 app_data 已下好的权重，其次安装包/开发目录。
pub(crate) fn resolve_model_directory(
    app: &AppHandle,
    resource_relative: &str,
    required_relative: &str,
) -> Result<PathBuf, String> {
    let leaf = resource_relative
        .rsplit('/')
        .next()
        .unwrap_or(resource_relative);
    if let Ok(root) = runtime_models_root(app) {
        let runtime = root.join(leaf);
        if runtime.join(required_relative).is_file() {
            let _ = ensure_sidecars(app, resource_relative, &runtime);
            return Ok(runtime);
        }
    }

    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|_| "model_resource_unavailable".to_owned())?;
    let packaged = resource_dir.join(resource_relative);
    if packaged.join(required_relative).is_file() {
        return Ok(packaged);
    }
    let flattened = resource_dir.join("models").join(leaf);
    if flattened.join(required_relative).is_file() {
        return Ok(flattened);
    }

    #[cfg(debug_assertions)]
    {
        let development = Path::new(env!("CARGO_MANIFEST_DIR")).join(resource_relative);
        if development.join(required_relative).is_file() {
            return Ok(development);
        }
    }

    Err("model_resource_unavailable".to_owned())
}

fn package_model_directory(app: &AppHandle, resource_relative: &str) -> Option<PathBuf> {
    let leaf = resource_relative
        .rsplit('/')
        .next()
        .unwrap_or(resource_relative);
    if let Ok(resource_dir) = app.path().resource_dir() {
        let packaged = resource_dir.join(resource_relative);
        if packaged.is_dir() {
            return Some(packaged);
        }
        let flattened = resource_dir.join("models").join(leaf);
        if flattened.is_dir() {
            return Some(flattened);
        }
    }
    #[cfg(debug_assertions)]
    {
        let development = Path::new(env!("CARGO_MANIFEST_DIR")).join(resource_relative);
        if development.is_dir() {
            return Some(development);
        }
    }
    None
}

fn ensure_sidecars(app: &AppHandle, resource_relative: &str, target: &Path) -> Result<(), String> {
    let Some(source) = package_model_directory(app, resource_relative) else {
        return Ok(());
    };
    let sidecars: &[&str] = match resource_relative {
        "resources/models/bge-small-zh-v1.5" => &[
            "config.json",
            "special_tokens_map.json",
            "tokenizer_config.json",
            "tokenizer.json",
            "LICENSE",
            "NOTICE.md",
        ],
        "resources/models/clip-ViT-B-32-vision" => {
            &["config.json", "preprocessor_config.json", "NOTICE.md"]
        }
        "resources/models/clip-ViT-B-32-text" => &[
            "config.json",
            "tokenizer.json",
            "tokenizer_config.json",
            "special_tokens_map.json",
            "NOTICE.md",
        ],
        _ => &[],
    };
    for relative in sidecars {
        let dest = target.join(relative);
        if dest.is_file() {
            continue;
        }
        let src = source.join(relative);
        if !src.is_file() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::copy(&src, &dest).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn artifact_ready_on_disk(app: &AppHandle, id: ArtifactId) -> bool {
    let required = match id {
        ArtifactId::BgeOnnx => "onnx/model.onnx",
        ArtifactId::ClipVisionOnnx | ArtifactId::ClipTextOnnx => "model.onnx",
    };
    let resource = format!("resources/models/{}", id.model_leaf());
    let Ok(directory) = resolve_model_directory(app, &resource, required) else {
        return false;
    };
    let path = directory.join(required);
    let Ok(metadata) = fs::metadata(&path) else {
        return false;
    };
    // 权重合计约 700 MB：同一进程内按路径+大小+修改时间记住已校验通过的文件，
    // 避免启动与每次状态快照重复整文件哈希；文件被替换后元数据变化即重新校验。
    let fingerprint = (metadata.len(), metadata.modified().ok());
    // 串行化整文件哈希：启动后台检查与状态查询并发时，后到者等首个结果进缓存，不重复读 700 MB。
    // 持有期间不得再取 state_lock，否则与持 state_lock 调用本函数的路径互锁。
    let _hashing = hashing_lock().lock();
    let verified = verified_artifacts();
    if verified
        .lock()
        .map(|cache| cache.get(&path) == Some(&fingerprint))
        .unwrap_or(false)
    {
        return true;
    }
    let ready = hash_file(&path).is_ok_and(|actual| actual == id.sha256());
    if ready {
        if let Ok(mut cache) = verified.lock() {
            cache.insert(path, fingerprint);
        }
    }
    ready
}

type ArtifactFingerprint = (u64, Option<SystemTime>);

fn hashing_lock() -> &'static Mutex<()> {
    static HASHING: Mutex<()> = Mutex::new(());
    &HASHING
}

fn verified_artifacts() -> &'static Mutex<HashMap<PathBuf, ArtifactFingerprint>> {
    static VERIFIED: OnceLock<Mutex<HashMap<PathBuf, ArtifactFingerprint>>> = OnceLock::new();
    VERIFIED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn snapshot_status(app: &AppHandle, state: &DownloadState) -> RuntimeModelStatus {
    let artifacts = ARTIFACTS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let progress = &state.artifacts[index];
            let ready = artifact_ready_on_disk(app, *id);
            let item_state = if ready {
                "ready".to_owned()
            } else {
                progress.state.clone()
            };
            RuntimeModelArtifactStatus {
                id: id.as_str().to_owned(),
                title: id.title().to_owned(),
                state: item_state,
                bytes_downloaded: progress.bytes_downloaded,
                bytes_total: progress.bytes_total,
                error: progress.error.clone(),
            }
        })
        .collect::<Vec<_>>();

    let all_ready = artifacts.iter().all(|item| item.state == "ready");
    let any_failed = artifacts.iter().any(|item| item.state == "failed");
    let any_active = artifacts.iter().any(|item| {
        matches!(
            item.state.as_str(),
            "downloading" | "verifying" | "checking"
        )
    });

    let overall = if all_ready {
        "ready".to_owned()
    } else if state.worker_running || any_active {
        "downloading".to_owned()
    } else if any_failed {
        "failed".to_owned()
    } else if artifacts.iter().any(|item| item.state == "pending") {
        "pending".to_owned()
    } else {
        state.overall.clone()
    };

    let (message, message_key, message_params) = if all_ready {
        (
            "本地选镜模型已就绪。".to_owned(),
            "allReady".to_owned(),
            BTreeMap::new(),
        )
    } else if !state.message.is_empty() {
        (
            state.message.clone(),
            state.message_key.clone(),
            state.message_params.clone(),
        )
    } else if any_failed {
        (
            "本地模型下载失败，可重试。已尝试官方与国内镜像续传；选镜仍可用，但语义/画面加权会降级。"
                .to_owned(),
            "failedRetryable".to_owned(),
            BTreeMap::new(),
        )
    } else if matches!(overall.as_str(), "downloading" | "pending") {
        (
            "正在后台下载本地选镜模型…".to_owned(),
            "downloadingAll".to_owned(),
            BTreeMap::new(),
        )
    } else {
        (
            "本地选镜模型待下载。".to_owned(),
            "notDownloaded".to_owned(),
            BTreeMap::new(),
        )
    };

    RuntimeModelStatus {
        overall,
        current_id: state.current.map(|id| id.as_str().to_owned()),
        message,
        message_key,
        message_params,
        artifacts,
    }
}

fn emit_status(app: &AppHandle, state: &DownloadState) {
    let status = snapshot_status(app, state);
    let _ = app.emit(PROGRESS_EVENT, &status);
}

fn refresh_artifact_baselines(app: &AppHandle, state: &mut DownloadState) {
    for id in ARTIFACTS {
        let index = artifact_index(id);
        if artifact_ready_on_disk(app, id) {
            state.artifacts[index].state = "ready".to_owned();
            state.artifacts[index].error = None;
        } else if state.artifacts[index].state == "ready" {
            state.artifacts[index].state = "pending".to_owned();
        }
    }
}

#[tauri::command(async)]
pub fn get_runtime_model_status(app: AppHandle) -> Result<RuntimeModelStatus, String> {
    let mut state = state_lock()
        .lock()
        .map_err(|_| "runtime_model_state_unavailable".to_owned())?;
    refresh_artifact_baselines(&app, &mut state);
    Ok(snapshot_status(&app, &state))
}

#[tauri::command(async)]
pub fn start_runtime_model_download(app: AppHandle) -> Result<RuntimeModelStatus, String> {
    let status = {
        let mut state = state_lock()
            .lock()
            .map_err(|_| "runtime_model_state_unavailable".to_owned())?;
        refresh_artifact_baselines(&app, &mut state);
        if state.worker_running {
            return Ok(snapshot_status(&app, &state));
        }
        if ARTIFACTS.iter().all(|id| artifact_ready_on_disk(&app, *id)) {
            state.overall = "ready".to_owned();
            state.say("allReady", &[], "本地选镜模型已就绪。".to_owned());
            return Ok(snapshot_status(&app, &state));
        }
        state.worker_running = true;
        state.overall = "downloading".to_owned();
        state.say(
            "downloadingAll",
            &[],
            "正在后台下载本地选镜模型…".to_owned(),
        );
        for id in ARTIFACTS {
            let index = artifact_index(id);
            if state.artifacts[index].state != "ready" {
                state.artifacts[index].state = "pending".to_owned();
                state.artifacts[index].error = None;
            }
        }
        emit_status(&app, &state);
        snapshot_status(&app, &state)
    };

    let worker_app = app.clone();
    thread::spawn(move || {
        run_download_worker(worker_app);
    });

    Ok(status)
}

/// 启动时若缺权重则自动开下，不阻塞调用方：冷启动整文件哈希约 10 秒，放到后台线程。
pub(crate) fn maybe_start_runtime_model_download(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        if ARTIFACTS.iter().all(|id| artifact_ready_on_disk(&app, *id)) {
            return;
        }
        let _ = start_runtime_model_download(app);
    });
}

fn run_download_worker(app: AppHandle) {
    for id in ARTIFACTS {
        if artifact_ready_on_disk(&app, id) {
            if let Ok(mut state) = state_lock().lock() {
                let index = artifact_index(id);
                state.artifacts[index].state = "ready".to_owned();
                state.artifacts[index].error = None;
                emit_status(&app, &state);
            }
            continue;
        }

        {
            let mut state = match state_lock().lock() {
                Ok(guard) => guard,
                Err(_) => break,
            };
            let index = artifact_index(id);
            state.current = Some(id);
            state.artifacts[index].state = "downloading".to_owned();
            state.artifacts[index].error = None;
            state.say(
                "downloading",
                &[("model", id.as_str().to_owned())],
                format!("正在下载{}…", id.title()),
            );
            emit_status(&app, &state);
        }

        match download_artifact(&app, id) {
            Ok(()) => {
                semantic::invalidate_failed_model_cache();
                clip::invalidate_failed_model_caches();
                if let Ok(mut state) = state_lock().lock() {
                    let index = artifact_index(id);
                    state.artifacts[index].state = "ready".to_owned();
                    state.artifacts[index].error = None;
                    state.say(
                        "modelReady",
                        &[("model", id.as_str().to_owned())],
                        format!("{}已就绪。", id.title()),
                    );
                    emit_status(&app, &state);
                }
            }
            Err(error) => {
                if let Ok(mut state) = state_lock().lock() {
                    let index = artifact_index(id);
                    state.artifacts[index].state = "failed".to_owned();
                    state.artifacts[index].error = Some(error.clone());
                    state.overall = "failed".to_owned();
                    state.say(
                        "modelFailed",
                        &[("model", id.as_str().to_owned()), ("error", error.clone())],
                        format!("{}下载失败：{error}", id.title()),
                    );
                    state.current = None;
                    state.worker_running = false;
                    emit_status(&app, &state);
                }
                return;
            }
        }
    }

    if let Ok(mut state) = state_lock().lock() {
        state.current = None;
        state.worker_running = false;
        refresh_artifact_baselines(&app, &mut state);
        if ARTIFACTS.iter().all(|id| artifact_ready_on_disk(&app, *id)) {
            state.overall = "ready".to_owned();
            state.say("allReady", &[], "本地选镜模型已就绪。".to_owned());
        }
        emit_status(&app, &state);
    }
}

fn download_artifact(app: &AppHandle, id: ArtifactId) -> Result<(), String> {
    let root = runtime_models_root(app)?;
    let relative = PathBuf::from(id.relative_path());
    let final_path = root.join(&relative);
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let resource = format!("resources/models/{}", id.model_leaf());
    ensure_sidecars(app, &resource, &root.join(id.model_leaf()))?;

    let partial_path = final_path.with_extension("onnx.partial");
    let urls = id.download_urls();
    let mut last_error = "下载失败。".to_owned();

    for attempt in 0..MAX_DOWNLOAD_ATTEMPTS {
        let url = urls[(attempt as usize) % urls.len()];
        let source_label = if url.starts_with(HF_MIRROR_HOST) {
            "国内镜像"
        } else {
            "官方源"
        };
        let existing = fs::metadata(&partial_path)
            .map(|meta| meta.len())
            .unwrap_or(0);

        {
            let mut state = state_lock()
                .lock()
                .map_err(|_| "runtime_model_state_unavailable".to_owned())?;
            let index = artifact_index(id);
            state.artifacts[index].bytes_downloaded = existing;
            state.artifacts[index].state = "downloading".to_owned();
            state.artifacts[index].error = None;
            let source = if url.starts_with(HF_MIRROR_HOST) {
                "mirror"
            } else {
                "official"
            };
            if attempt == 0 {
                state.say(
                    "downloadingFrom",
                    &[
                        ("model", id.as_str().to_owned()),
                        ("source", source.to_owned()),
                    ],
                    format!("正在从{source_label}下载{}…", id.title()),
                );
            } else {
                state.say(
                    "resumingFrom",
                    &[
                        ("model", id.as_str().to_owned()),
                        ("source", source.to_owned()),
                        ("attempt", (attempt + 1).to_string()),
                        ("max", MAX_DOWNLOAD_ATTEMPTS.to_string()),
                    ],
                    format!(
                        "正在从{source_label}续传{}（第 {}/{} 次）…",
                        id.title(),
                        attempt + 1,
                        MAX_DOWNLOAD_ATTEMPTS
                    ),
                );
            }
            emit_status(app, &state);
        }

        match download_artifact_attempt(app, id, url, &partial_path, &final_path) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = error.clone();
                let transient = is_transient_download_error(&error);
                if !transient {
                    let _ = fs::remove_file(&partial_path);
                    return Err(error);
                }
                if attempt + 1 >= MAX_DOWNLOAD_ATTEMPTS {
                    break;
                }
                {
                    if let Ok(mut state) = state_lock().lock() {
                        let index = artifact_index(id);
                        state.say(
                            "interrupted",
                            &[("model", id.as_str().to_owned())],
                            format!("{}下载中断，即将自动换源/续传重试…", id.title()),
                        );
                        state.artifacts[index].error = Some(error);
                        emit_status(app, &state);
                    }
                }
                let delay = RETRY_BASE_DELAY
                    .saturating_mul(attempt + 1)
                    .min(Duration::from_secs(20));
                thread::sleep(delay);
            }
        }
    }

    Err(format!(
        "多次重试仍失败（已尝试官方与国内镜像）：{last_error}"
    ))
}

fn download_artifact_attempt(
    app: &AppHandle,
    id: ArtifactId,
    url: &str,
    partial_path: &Path,
    final_path: &Path,
) -> Result<(), String> {
    let existing = fs::metadata(partial_path)
        .map(|meta| meta.len())
        .unwrap_or(0);

    let agent = outbound_http::voice_agent();
    let mut request = agent.get(url);
    if existing > 0 {
        request = request.set("Range", &format!("bytes={existing}-"));
    }

    let response = request
        .timeout(DOWNLOAD_ATTEMPT_TIMEOUT)
        .call()
        .map_err(|error| format!("下载失败：{error}"))?;

    let status = response.status();
    if status != 200 && status != 206 {
        return Err(format!("下载失败：HTTP {status}"));
    }

    let total = parse_total_bytes(
        response.header("Content-Range"),
        response.header("Content-Length"),
        existing,
        status,
    );
    {
        let mut state = state_lock()
            .lock()
            .map_err(|_| "runtime_model_state_unavailable".to_owned())?;
        let index = artifact_index(id);
        state.artifacts[index].bytes_total = total;
        emit_status(app, &state);
    }

    let append = status == 206 && existing > 0;
    if !append && partial_path.exists() {
        let _ = fs::remove_file(partial_path);
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(partial_path)
        .map_err(|error| error.to_string())?;

    let mut reader = response.into_reader();
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded = if append { existing } else { 0 };
    let mut since_emit = 0_u64;

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("下载中断：{error}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| format!("写入失败：{error}"))?;
        downloaded += read as u64;
        since_emit += read as u64;
        if since_emit >= PROGRESS_EMIT_EVERY_BYTES {
            since_emit = 0;
            if let Ok(mut state) = state_lock().lock() {
                let index = artifact_index(id);
                state.artifacts[index].bytes_downloaded = downloaded;
                state.artifacts[index].bytes_total = total.or(Some(downloaded));
                emit_status(app, &state);
            }
        }
    }
    file.flush().map_err(|error| error.to_string())?;
    drop(file);

    if let Some(expected) = total {
        if downloaded < expected {
            return Err(format!("下载未完成（已下 {downloaded}/{expected} 字节）"));
        }
    }

    {
        let mut state = state_lock()
            .lock()
            .map_err(|_| "runtime_model_state_unavailable".to_owned())?;
        let index = artifact_index(id);
        state.artifacts[index].state = "verifying".to_owned();
        state.artifacts[index].bytes_downloaded = downloaded;
        state.say(
            "verifying",
            &[("model", id.as_str().to_owned())],
            format!("正在校验{}…", id.title()),
        );
        emit_status(app, &state);
    }

    let actual = hash_file(partial_path)?;
    if actual != id.sha256() {
        let _ = fs::remove_file(partial_path);
        return Err("文件校验失败，请重试。".to_owned());
    }

    if final_path.exists() {
        fs::remove_file(final_path).map_err(|error| error.to_string())?;
    }
    fs::rename(partial_path, final_path).map_err(|error| error.to_string())?;
    Ok(())
}

fn is_transient_download_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("下载中断")
        || lower.contains("下载未完成")
        || lower.contains("connection")
        || lower.contains("reset")
        || lower.contains("broken pipe")
        || lower.contains("eof")
        || lower.contains("temporarily")
        || lower.contains("unavailable")
        || lower.contains("http 408")
        || lower.contains("http 425")
        || lower.contains("http 429")
        || lower.contains("http 500")
        || lower.contains("http 502")
        || lower.contains("http 503")
        || lower.contains("http 504")
        || lower.contains("os error 10060")
        || lower.contains("os error 10054")
}

fn parse_total_bytes(
    content_range: Option<&str>,
    content_length: Option<&str>,
    existing: u64,
    status: u16,
) -> Option<u64> {
    if let Some(range) = content_range {
        // bytes start-end/total
        if let Some(total) = range.split('/').nth(1) {
            if total != "*" {
                if let Ok(value) = total.parse::<u64>() {
                    return Some(value);
                }
            }
        }
    }
    let length = content_length.and_then(|value| value.parse::<u64>().ok())?;
    if status == 206 {
        Some(existing + length)
    } else {
        Some(length)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_transient_download_error, ArtifactId, HF_HOST, HF_MIRROR_HOST};

    #[test]
    fn download_urls_cover_official_and_mirror() {
        for id in [
            ArtifactId::BgeOnnx,
            ArtifactId::ClipVisionOnnx,
            ArtifactId::ClipTextOnnx,
        ] {
            let urls = id.download_urls();
            assert!(urls[0].starts_with(HF_HOST));
            assert!(urls[1].starts_with(HF_MIRROR_HOST));
            assert_eq!(
                urls[0].trim_start_matches(HF_HOST),
                urls[1].trim_start_matches(HF_MIRROR_HOST)
            );
        }
    }

    #[test]
    fn transient_classifier_keeps_partial_on_timeout() {
        assert!(is_transient_download_error(
            "下载中断：timed out reading response"
        ));
        assert!(is_transient_download_error(
            "下载未完成（已下 100/200 字节）"
        ));
        assert!(!is_transient_download_error("文件校验失败，请重试。"));
        assert!(!is_transient_download_error("下载失败：HTTP 404"));
    }
}
