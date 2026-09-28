//! Jamendo 音乐、ElevenLabs / Fish Audio 配音的凭据、有界 HTTP 适配器，以及经 Voycut 网关的配音。
//! 下载后的音频仍通过素材模块登记并进入本地分析与审计流程；TTS 领域算法在 voice_provider。

use crate::assets::store_downloaded_audio;
use crate::models::Asset;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::{fs, io::Read};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const CREDENTIAL_SERVICE: &str = "AssemblyVideoAgent";
const CREDENTIAL_ACCOUNT: &str = "jamendo-music-provider";
const API_BASE: &str = "https://api.jamendo.com/v3.0";
const MAX_DOWNLOAD_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JamendoStatus {
    pub state: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JamendoTrack {
    pub id: String,
    pub name: String,
    pub artist_name: String,
    pub duration: i64,
    pub license_ccurl: String,
    pub audiodownload_allowed: bool,
}

#[derive(Deserialize)]
struct JamendoResponse {
    headers: JamendoHeaders,
    results: Vec<JamendoTrack>,
}

#[derive(Deserialize)]
struct JamendoHeaders {
    status: String,
    code: i64,
}

fn checked_tracks(response: JamendoResponse) -> Result<Vec<JamendoTrack>, String> {
    if response.headers.status != "success" {
        return Err(format!(
            "Jamendo rejected the music request (code {}). Check the Client ID and application status.",
            response.headers.code
        ));
    }
    Ok(response.results)
}

fn entry() -> Result<Entry, String> {
    Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
        .map_err(|_| "Windows Credential Manager is unavailable.".to_owned())
}

fn client_id() -> Result<String, String> {
    entry()?
        .get_password()
        .map_err(|_| "Jamendo music Provider is not configured.".to_owned())
}

/// 快照只投影连接布尔值；读取异常与明确未配置必须分开，避免把凭据故障伪装成空配置。
pub(crate) fn jamendo_configured_for_snapshot() -> Result<bool, String> {
    match entry()?.get_password() {
        Ok(value) => Ok(!value.trim().is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(_) => Err("Windows Credential Manager could not read Jamendo credentials.".to_owned()),
    }
}

fn allowed(track: &JamendoTrack) -> bool {
    let license = track
        .license_ccurl
        .trim()
        .trim_end_matches('/')
        .to_ascii_lowercase();
    track.audiodownload_allowed
        && (license == "https://creativecommons.org/publicdomain/zero/1.0"
            || license == "http://creativecommons.org/publicdomain/zero/1.0"
            || license == "https://creativecommons.org/licenses/by/3.0"
            || license == "http://creativecommons.org/licenses/by/3.0"
            || license == "https://creativecommons.org/licenses/by/4.0"
            || license == "http://creativecommons.org/licenses/by/4.0")
}

pub(crate) fn attribution_for(track: &JamendoTrack) -> String {
    format!(
        "{} — {} (Jamendo, {})",
        track.artist_name, track.name, track.license_ccurl
    )
}

pub(crate) fn eligible_track(track_id: &str) -> Result<JamendoTrack, String> {
    let id = client_id()?;
    let lookup_url = api_url(
        "tracks",
        &[
            ("client_id", id),
            ("format", "json".to_owned()),
            ("id", track_id.to_owned()),
        ],
    )?;
    let lookup = ureq::get(&lookup_url)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|_| "Jamendo music search is unavailable.".to_owned())?;
    let catalog: JamendoResponse = serde_json::from_reader(lookup.into_reader())
        .map_err(|_| "Jamendo returned an invalid music catalog response.".to_owned())?;
    checked_tracks(catalog)?
        .into_iter()
        .find(|track| track.id == track_id && allowed(track))
        .ok_or_else(|| {
            "The selected Jamendo track is unavailable or not eligible for automatic use."
                .to_owned()
        })
}

fn api_url(path: &str, query: &[(&str, String)]) -> Result<String, String> {
    let mut url = url::Url::parse(&format!("{API_BASE}/{path}/"))
        .map_err(|_| "Could not prepare Jamendo request.".to_owned())?;
    {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query {
            pairs.append_pair(key, value);
        }
    }
    Ok(url.into())
}

pub(crate) fn search_tracks(query: &str) -> Result<Vec<JamendoTrack>, String> {
    let id = client_id()?;
    let url = api_url(
        "tracks",
        &[
            ("client_id", id),
            ("format", "json".to_owned()),
            ("limit", "20".to_owned()),
            ("namesearch", query.trim().to_owned()),
            ("audioformat", "mp32".to_owned()),
        ],
    )?;
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|_| "Jamendo music search is unavailable.".to_owned())?;
    let response: JamendoResponse = serde_json::from_reader(response.into_reader())
        .map_err(|_| "Jamendo returned an invalid music catalog response.".to_owned())?;
    Ok(checked_tracks(response)?.into_iter().filter(allowed).collect())
}

/// 自动配乐按情绪标签找器乐曲（`namesearch` 只匹配曲名，搜 “upbeat” 这类词几乎无结果）。
pub(crate) fn search_instrumental_by_tags(tags: &str) -> Result<Vec<JamendoTrack>, String> {
    let id = client_id()?;
    let url = api_url(
        "tracks",
        &[
            ("client_id", id),
            ("format", "json".to_owned()),
            ("limit", "20".to_owned()),
            ("fuzzytags", tags.trim().to_owned()),
            ("vocalinstrumental", "instrumental".to_owned()),
            ("order", "popularity_total".to_owned()),
            ("audioformat", "mp32".to_owned()),
        ],
    )?;
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|_| "Jamendo music search is unavailable.".to_owned())?;
    let response: JamendoResponse = serde_json::from_reader(response.into_reader())
        .map_err(|_| "Jamendo returned an invalid music catalog response.".to_owned())?;
    Ok(checked_tracks(response)?.into_iter().filter(allowed).collect())
}

pub(crate) fn download_track(
    app: &AppHandle,
    project_id: &str,
    track_id: &str,
) -> Result<Asset, String> {
    let id = client_id()?;
    let track = eligible_track(track_id)?;
    let url = api_url(
        "tracks/file",
        &[
            ("client_id", id),
            ("id", track.id.clone()),
            ("audioformat", "mp32".to_owned()),
            ("action", "download".to_owned()),
        ],
    )?;
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(60))
        .call()
        .map_err(|_| "Jamendo could not download the selected track.".to_owned())?;
    if response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|size| size > MAX_DOWNLOAD_BYTES)
    {
        return Err("The selected music file is too large for automatic download.".to_owned());
    }
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("licensed-music")
        .join(project_id);
    fs::create_dir_all(&directory)
        .map_err(|_| "Could not prepare local music storage.".to_owned())?;
    let destination = directory.join(format!("jamendo-{}-{}.mp3", track.id, Uuid::new_v4()));
    let mut reader = response.into_reader().take(MAX_DOWNLOAD_BYTES + 1);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|_| "Jamendo music download was interrupted.".to_owned())?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err("The selected music file is too large for automatic download.".to_owned());
    }
    fs::write(&destination, bytes)
        .map_err(|_| "Could not save the selected music locally.".to_owned())?;
    store_downloaded_audio(
        app,
        project_id,
        destination,
        &format!("Jamendo: {} — {}", track.artist_name, track.name),
    )
}

#[tauri::command(async)]
pub fn get_jamendo_status() -> JamendoStatus {
    JamendoStatus {
        state: if client_id().is_ok() {
            "connected"
        } else {
            "disconnected"
        }
        .to_owned(),
    }
}

#[tauri::command(async)]
pub fn save_jamendo_client_id(client_id: String) -> JamendoStatus {
    let value = client_id.trim();
    if value.is_empty()
        || entry()
            .and_then(|entry| {
                entry
                    .set_password(value)
                    .map_err(|_| "Could not save Jamendo credentials.".to_owned())
            })
            .is_err()
    {
        return JamendoStatus {
            state: "failed".to_owned(),
        };
    }
    JamendoStatus {
        state: "connected".to_owned(),
    }
}

const ELEVENLABS_ACCOUNT: &str = "elevenlabs-voice-provider";
const ELEVENLABS_API_ROOT: &str = "https://api.elevenlabs.io/v1";
const ELEVENLABS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevenLabsStatus {
    pub key_stored: bool,
    pub voices_readable: bool,
    pub tts_authorized: Option<bool>,
    pub last_error_code: Option<String>,
    pub importable: bool,
}

fn elevenlabs_entry() -> Result<Entry, String> {
    Entry::new(CREDENTIAL_SERVICE, ELEVENLABS_ACCOUNT)
        .map_err(|_| "ElevenLabs Credential Manager is unavailable.".to_owned())
}

fn elevenlabs_stored_key() -> Result<String, String> {
    elevenlabs_entry()?
        .get_password()
        .map_err(|_| "ElevenLabs voice Provider is not configured.".to_owned())
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Err("ElevenLabs voice Provider is not configured.".to_owned())
            } else {
                Ok(trimmed.to_owned())
            }
        })
}

/// 快照不探活、不发网络请求，只在凭据所有者边界内确认本机密钥是否存在。
pub(crate) fn elevenlabs_configured_for_snapshot() -> Result<bool, String> {
    match elevenlabs_entry()?.get_password() {
        Ok(value) => Ok(!value.trim().is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(_) => {
            Err("Windows Credential Manager could not read ElevenLabs credentials.".to_owned())
        }
    }
}

fn elevenlabs_environment_key() -> Option<String> {
    std::env::var("ELEVENLABS_API_KEY")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(crate) fn elevenlabs_json_request(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let api_key = elevenlabs_stored_key()?;
    let url = format!("{ELEVENLABS_API_ROOT}{path}");
    let agent = crate::outbound_http::voice_agent();
    let mut request = if method == "POST" {
        agent.post(&url)
    } else {
        agent.get(&url)
    };
    request = request
        .set("xi-api-key", &api_key)
        .set("Accept", "application/json")
        .timeout(crate::execution_deadline::timeout(ELEVENLABS_TIMEOUT)?);
    let result = if let Some(payload) = body {
        request
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string())
    } else {
        request.call()
    };
    match result {
        Ok(response) => response
            .into_string()
            .map_err(|_| "ElevenLabs returned an invalid JSON response.".to_owned())
            .and_then(|body| {
                serde_json::from_str(&body)
                    .map_err(|_| "ElevenLabs returned an invalid JSON response.".to_owned())
            }),
        Err(ureq::Error::Status(code, response)) => {
            let detail = response.into_string().unwrap_or_default();
            Err(classify_elevenlabs_http_error(code, &detail))
        }
        Err(ureq::Error::Transport(transport)) => Err(
            crate::outbound_http::classify_voice_transport("ElevenLabs", &transport),
        ),
    }
}

fn classify_elevenlabs_http_error(code: u16, detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    if code == 402 && lower.contains("paid_plan_required") {
        return "This ElevenLabs voice requires a paid plan. Choose Charlie or another premade voice.".to_owned();
    }
    if code == 401 {
        return "ElevenLabs API key was rejected.".to_owned();
    }
    if lower.contains("voices_read") {
        return "voices_read_missing".to_owned();
    }
    if code == 400 {
        if lower.contains("voice") {
            return "ElevenLabs rejected the selected voice.".to_owned();
        }
        if lower.contains("model") {
            return "ElevenLabs rejected the selected model.".to_owned();
        }
        return "ElevenLabs rejected the speech request.".to_owned();
    }
    format!("ElevenLabs API error {code}.")
}

fn elevenlabs_error_code(error: &str) -> String {
    if error.contains("timed out") {
        "timeout".to_owned()
    } else if error.contains("rejected") {
        "unauthorized".to_owned()
    } else if error.contains("paid plan") {
        "paid_plan_required".to_owned()
    } else if error == "voices_read_missing" {
        error.to_owned()
    } else {
        "elevenlabs_error".to_owned()
    }
}

#[tauri::command(async)]
pub fn get_elevenlabs_status() -> ElevenLabsStatus {
    let key_stored = elevenlabs_stored_key().is_ok();
    let importable = !key_stored && elevenlabs_environment_key().is_some();
    if !key_stored {
        return ElevenLabsStatus {
            key_stored: false,
            voices_readable: false,
            tts_authorized: None,
            last_error_code: None,
            importable,
        };
    }
    match elevenlabs_json_request("GET", "/voices", None) {
        Ok(_) => ElevenLabsStatus {
            key_stored: true,
            voices_readable: true,
            tts_authorized: None,
            last_error_code: None,
            importable: false,
        },
        Err(error) if error == "voices_read_missing" => ElevenLabsStatus {
            key_stored: true,
            voices_readable: false,
            tts_authorized: None,
            last_error_code: Some(error),
            importable: false,
        },
        Err(error) => ElevenLabsStatus {
            key_stored: true,
            voices_readable: false,
            tts_authorized: None,
            last_error_code: Some(elevenlabs_error_code(&error)),
            importable: false,
        },
    }
}

#[tauri::command(async)]
pub fn save_elevenlabs_api_key(api_key: String) -> ElevenLabsStatus {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return ElevenLabsStatus {
            key_stored: false,
            voices_readable: false,
            tts_authorized: None,
            last_error_code: Some("empty_key".to_owned()),
            importable: elevenlabs_environment_key().is_some(),
        };
    }
    if elevenlabs_entry()
        .and_then(|entry| {
            entry
                .set_password(trimmed)
                .map_err(|_| "Could not save ElevenLabs credentials.".to_owned())
        })
        .is_err()
    {
        return ElevenLabsStatus {
            key_stored: false,
            voices_readable: false,
            tts_authorized: None,
            last_error_code: Some("credential_store_failed".to_owned()),
            importable: elevenlabs_environment_key().is_some(),
        };
    }
    get_elevenlabs_status()
}

#[tauri::command(async)]
pub fn clear_elevenlabs_api_key() -> ElevenLabsStatus {
    if let Ok(entry) = elevenlabs_entry() {
        let _ = entry.delete_credential();
    }
    get_elevenlabs_status()
}

#[tauri::command(async)]
pub fn import_elevenlabs_api_key_from_environment() -> ElevenLabsStatus {
    if elevenlabs_stored_key().is_ok() {
        return get_elevenlabs_status();
    }
    let Some(api_key) = elevenlabs_environment_key() else {
        return ElevenLabsStatus {
            key_stored: false,
            voices_readable: false,
            tts_authorized: None,
            last_error_code: Some("environment_key_missing".to_owned()),
            importable: false,
        };
    };
    save_elevenlabs_api_key(api_key)
}

pub(crate) mod fish_audio {
    // Fish Audio 配音凭据与有界 HTTP 适配器。
    // API Key 仅存 Windows Credential Manager；对外只返回连接状态和标准化音频/时间戳。

    use base64::Engine;
    use keyring::Entry;
    use serde::Serialize;
    use serde_json::{json, Value};
    use std::{collections::BTreeMap, io::Read, time::Duration};

    const CREDENTIAL_SERVICE: &str = "AssemblyVideoAgent";
    const CREDENTIAL_ACCOUNT: &str = "fish-audio-voice-provider";
    const API_ROOT: &str = "https://api.fish.audio";
    const MODEL: &str = "s2.1-pro-free";
    const TIMEOUT: Duration = Duration::from_secs(90);
    const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FishAudioStatus {
        pub key_stored: bool,
        pub voices_readable: bool,
        pub last_error_code: Option<String>,
        pub importable: bool,
    }

    fn entry() -> Result<Entry, String> {
        Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
            .map_err(|_| "Fish Audio Credential Manager is unavailable.".to_owned())
    }

    fn stored_key() -> Result<String, String> {
        entry()?
            .get_password()
            .map_err(|_| "Fish Audio voice Provider is not configured.".to_owned())
            .and_then(|value| {
                let value = value.trim();
                (!value.is_empty())
                    .then(|| value.to_owned())
                    .ok_or_else(|| "Fish Audio voice Provider is not configured.".to_owned())
            })
    }

    fn environment_key() -> Option<String> {
        std::env::var("FISH_API_KEY")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn configured_for_snapshot() -> Result<bool, String> {
        match entry()?.get_password() {
            Ok(value) => Ok(!value.trim().is_empty()),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(_) => {
                Err("Windows Credential Manager could not read Fish Audio credentials.".to_owned())
            }
        }
    }

    fn request(method: &str, path: &str, body: Option<Value>) -> Result<ureq::Response, String> {
        let key = stored_key()?;
        let url = format!("{API_ROOT}{path}");
        let agent = crate::outbound_http::voice_agent();
        let mut request = if method == "POST" {
            agent.post(&url)
        } else {
            agent.get(&url)
        };
        request = request
            .set("Authorization", &format!("Bearer {key}"))
            .set("Accept", "application/json")
            .timeout(crate::execution_deadline::timeout(TIMEOUT)?);
        let result = if let Some(body) = body {
            request
                .set("Content-Type", "application/json")
                .set("model", MODEL)
                .send_string(&body.to_string())
        } else {
            request.call()
        };
        result.map_err(|error| match error {
            ureq::Error::Status(401, _) => "Fish Audio API key was rejected.".to_owned(),
            ureq::Error::Status(402, _) => {
                "Fish Audio account cannot use the selected TTS model.".to_owned()
            }
            ureq::Error::Status(code, _) => format!("Fish Audio API error {code}."),
            ureq::Error::Transport(transport) => {
                crate::outbound_http::classify_voice_transport("Fish Audio", &transport)
            }
        })
    }

    pub(crate) fn list_voices() -> Result<Value, String> {
        let response = request("GET", "/model?page_size=20&page_number=1&self=true", None)?;
        let payload: Value = serde_json::from_reader(response.into_reader())
            .map_err(|_| "Fish Audio returned an invalid voice list.".to_owned())?;
        let voices = payload
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let id = item.get("_id")?.as_str()?;
                let name = item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Unnamed");
                Some(json!({ "voice_id": id, "name": name, "category": "fish_audio" }))
            })
            .collect::<Vec<_>>();
        Ok(json!({ "voices": voices }))
    }

    fn shift_timed_units(alignment: &Value, key: &str, offset: f64) -> Vec<Value> {
        alignment
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .cloned()
            .map(|mut unit| {
                if let Some(start) = unit.get("start").and_then(Value::as_f64) {
                    unit["start"] = json!(start + offset);
                }
                if let Some(end) = unit.get("end").and_then(Value::as_f64) {
                    unit["end"] = json!(end + offset);
                }
                unit
            })
            .collect()
    }

    pub(crate) fn synthesize(text: &str, reference_id: Option<&str>) -> Result<Value, String> {
        let mut body =
            json!({ "text": text, "format": "mp3", "sample_rate": 44100, "mp3_bitrate": 128 });
        if let Some(reference_id) = reference_id.filter(|value| !value.trim().is_empty()) {
            body["reference_id"] = json!(reference_id);
        }
        let response = request("POST", "/v1/tts/stream/with-timestamp", Some(body))?;
        read_timestamp_stream(response)
    }

    /// 读取 Fish 带时间戳的 SSE 流，合成音频并按 chunk 偏移拼接 segments/words。
    /// 直连与经 Voycut 网关转发的是同一格式，共用本解析。
    pub(crate) fn read_timestamp_stream(response: ureq::Response) -> Result<Value, String> {
        let mut raw = String::new();
        response
            .into_reader()
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_string(&mut raw)
            .map_err(|_| "Fish Audio response was interrupted.".to_owned())?;
        if raw.len() as u64 > MAX_RESPONSE_BYTES {
            return Err("Fish Audio returned an unusable audio payload.".to_owned());
        }
        let mut audio = Vec::new();
        let mut alignments = BTreeMap::<i64, Value>::new();
        for line in raw.lines().filter_map(|line| line.strip_prefix("data: ")) {
            let event: Value = serde_json::from_str(line)
                .map_err(|_| "Fish Audio returned an invalid timestamp stream.".to_owned())?;
            let encoded = event
                .get("audio_base64")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !encoded.is_empty() {
                audio.extend(
                    base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .map_err(|_| "Fish Audio returned invalid audio encoding.".to_owned())?,
                );
            }
            if let (Some(sequence), Some(alignment)) = (
                event.get("chunk_seq").and_then(Value::as_i64),
                event.get("alignment").filter(|value| value.is_object()),
            ) {
                let offset = event
                    .get("chunk_audio_offset_sec")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let mut value = alignment.clone();
                value["offset"] = json!(offset);
                alignments.insert(sequence, value);
            }
        }
        if audio.is_empty() {
            return Err("Fish Audio did not return audio.".to_owned());
        }
        let mut segments = Vec::new();
        let mut words = Vec::new();
        for alignment in alignments.into_values() {
            let offset = alignment
                .get("offset")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            segments.extend(shift_timed_units(&alignment, "segments", offset));
            words.extend(shift_timed_units(&alignment, "words", offset));
        }
        let mut alignment = json!({ "segments": segments });
        if !words.is_empty() {
            alignment["words"] = json!(words);
        }
        Ok(json!({
            "audio_base64": base64::engine::general_purpose::STANDARD.encode(audio),
            "alignment": alignment
        }))
    }

    fn status() -> FishAudioStatus {
        let key_stored = stored_key().is_ok();
        let importable = !key_stored && environment_key().is_some();
        if !key_stored {
            return FishAudioStatus {
                key_stored: false,
                voices_readable: false,
                last_error_code: None,
                importable,
            };
        }
        match list_voices() {
            Ok(_) => FishAudioStatus {
                key_stored: true,
                voices_readable: true,
                last_error_code: None,
                importable: false,
            },
            Err(error) => FishAudioStatus {
                key_stored: true,
                voices_readable: false,
                last_error_code: Some(
                    if error.contains("rejected") {
                        "unauthorized"
                    } else {
                        "fish_audio_error"
                    }
                    .to_owned(),
                ),
                importable: false,
            },
        }
    }

    #[tauri::command(async)]
    pub fn get_fish_audio_status() -> FishAudioStatus {
        status()
    }

    #[tauri::command(async)]
    pub fn save_fish_audio_api_key(api_key: String) -> FishAudioStatus {
        if entry()
            .and_then(|entry| {
                entry
                    .set_password(api_key.trim())
                    .map_err(|_| "Could not save Fish Audio credentials.".to_owned())
            })
            .is_err()
        {
            return FishAudioStatus {
                key_stored: false,
                voices_readable: false,
                last_error_code: Some("credential_store_failed".to_owned()),
                importable: environment_key().is_some(),
            };
        }
        status()
    }

    #[tauri::command(async)]
    pub fn clear_fish_audio_api_key() -> FishAudioStatus {
        if let Ok(entry) = entry() {
            let _ = entry.delete_credential();
        }
        status()
    }

    #[tauri::command(async)]
    pub fn import_fish_audio_api_key_from_environment() -> FishAudioStatus {
        match environment_key() {
            Some(key) => save_fish_audio_api_key(key),
            None => FishAudioStatus {
                key_stored: false,
                voices_readable: false,
                last_error_code: Some("environment_key_missing".to_owned()),
                importable: false,
            },
        }
    }
}

pub(crate) mod gateway_voice {
    // Voycut 网关配音：登录 Voycut 账号后经网站网关合成，配音密钥只在服务端，桌面不持有。
    // 网关原样转发 Fish Audio 带时间戳的流，解析复用 `music_provider::fish_audio`；
    // 失败带 `voice_gateway_*` 稳定码前缀，由 Agent 失败上下文按码说明，不静默换 Provider。

    use serde::Serialize;
    use serde_json::{json, Value};
    use std::{io::Read, time::Duration};

    const TIMEOUT: Duration = Duration::from_secs(90);
    const MAX_LIST_BYTES: u64 = 1024 * 1024;

    pub(crate) const AUTH: &str = "voice_gateway_auth";
    pub(crate) const ENTITLEMENT: &str = "voice_gateway_entitlement";
    pub(crate) const UPGRADE_REQUIRED: &str = "voice_gateway_upgrade_required";
    pub(crate) const DAILY_QUOTA: &str = "voice_gateway_daily_quota";
    pub(crate) const NOT_CONFIGURED: &str = "voice_gateway_not_configured";
    pub(crate) const REJECTED: &str = "voice_gateway_rejected";
    pub(crate) const UNAVAILABLE: &str = "voice_gateway_unavailable";

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct VoiceAvailability {
        /// 配音开关是否显示；只有网关明确没有配音能力时为 false。
        available: bool,
        via_gateway: bool,
        /// 探测失败的稳定码；登录失效、网络等暂时性原因仍视为可用，由真实请求给出具体原因。
        reason: Option<String>,
    }

    /// 错误里带的网关配音码（可能被上层加了前缀，按子串识别）。
    pub(crate) fn error_code(error: &str) -> Option<&'static str> {
        [AUTH, ENTITLEMENT, UPGRADE_REQUIRED, DAILY_QUOTA, NOT_CONFIGURED, REJECTED, UNAVAILABLE]
            .into_iter()
            .find(|code| error.contains(code))
    }

    /// Agent 失败上下文里给模型的事实与恢复说明；只有 unavailable 可重试。
    pub(crate) fn failure_guidance(code: &str) -> (&'static str, &'static str) {
        match code {
            AUTH => ("The user's Voycut sign-in is missing or expired.", "Ask the user to sign in to Voycut again from the account menu."),
            ENTITLEMENT => ("Voycut access is not active for this account.", "Tell the user their Voycut access is not active and point them to the account page."),
            UPGRADE_REQUIRED => ("This Voycut version is no longer supported by the voice service.", "Ask the user to download the latest Voycut from the website."),
            DAILY_QUOTA => ("Today's Voycut voiceover quota for this account is used up; it resets at 00:00 UTC.", "Do not retry voiceover today. Offer to continue without narration."),
            NOT_CONFIGURED => ("The Voycut service does not offer voiceover right now.", "Do not retry voiceover. Offer to continue without narration."),
            REJECTED => ("The Voycut voice service rejected this narration.", "Do not retry the same narration. Ask the user whether to shorten or rewrite it."),
            _ => ("The Voycut voice service is temporarily unavailable.", "Voiceover may be retried once later if the user asks."),
        }
    }

    /// 与模型网关同一站点：`https://<站点>/api/model` → `https://<站点>/api/voice/<path>`。
    fn voice_url(path: &str) -> Result<Option<String>, String> {
        let Some(base_url) = crate::fellowcut_account::gateway_base_url()? else {
            return Ok(None);
        };
        let site = base_url.strip_suffix("/api/model").ok_or_else(|| {
            "provider_gateway_not_configured: The Voycut model service address in this build is invalid."
                .to_owned()
        })?;
        Ok(Some(format!("{site}/api/voice/{path}")))
    }

    /// 构建内置了网关时，配音只走网关（与模型访问同一顺序），不再读本机配音密钥。
    pub(crate) fn enabled() -> Result<bool, String> {
        Ok(crate::fellowcut_account::gateway_base_url()?.is_some())
    }

    fn failure(status: u16, body: &str) -> String {
        let code = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|value| value.get("error").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default();
        match (status, code.as_str()) {
            (401, _) => format!("{AUTH}: Voycut sign-in is missing or expired."),
            (403, _) => format!("{ENTITLEMENT}: Voycut access is not active for this account."),
            (426, _) => format!("{UPGRADE_REQUIRED}: This Voycut version is no longer supported by the voice service."),
            (429, "daily_quota_exceeded") => {
                format!("{DAILY_QUOTA}: Today's Voycut voiceover quota is used up; it resets at 00:00 UTC.")
            }
            // 旧网关没有配音接口（404），或服务端没配配音密钥。
            (404, _) | (503, "voice_not_configured") => {
                format!("{NOT_CONFIGURED}: The Voycut service does not offer voiceover right now.")
            }
            (400 | 413, _) => format!("{REJECTED}: The Voycut voice service rejected this narration (HTTP {status})."),
            _ => format!("{UNAVAILABLE}: The Voycut voice service is unavailable (HTTP {status})."),
        }
    }

    fn request(method: &str, path: &str, body: Option<Value>) -> Result<ureq::Response, String> {
        let url = voice_url(path)?.ok_or_else(|| {
            format!("{NOT_CONFIGURED}: This build has no Voycut service configured.")
        })?;
        let id_token = crate::fellowcut_account::fresh_id_token()
            .map_err(|_| format!("{AUTH}: Voycut sign-in is missing or expired."))?;
        let agent = crate::outbound_http::voice_agent();
        let request = if method == "POST" { agent.post(&url) } else { agent.get(&url) }
            .set("Authorization", &format!("Bearer {id_token}"))
            .set("X-Voycut-Version", env!("CARGO_PKG_VERSION"))
            .set("Accept", "application/json")
            .timeout(crate::execution_deadline::timeout(TIMEOUT)?);
        let result = match body {
            Some(body) => request
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
            None => request.call(),
        };
        result.map_err(|error| match error {
            ureq::Error::Status(status, response) => {
                let mut body = String::new();
                let _ = response.into_reader().take(4096).read_to_string(&mut body);
                failure(status, &body)
            }
            ureq::Error::Transport(transport) => format!(
                "{UNAVAILABLE}: {}",
                crate::outbound_http::classify_voice_transport("Voycut voice service", &transport)
            ),
        })
    }

    /// 网关已把 Fish 音色整理成 `{ voices: [{ voice_id, name, category }] }`。
    pub(crate) fn list_voices() -> Result<Value, String> {
        let response = request("GET", "voices", None)?;
        let payload: Value = serde_json::from_reader(response.into_reader().take(MAX_LIST_BYTES))
            .map_err(|_| format!("{UNAVAILABLE}: The Voycut voice service returned an invalid voice list."))?;
        if !payload.get("voices").is_some_and(Value::is_array) {
            return Err(format!("{UNAVAILABLE}: The Voycut voice service returned an invalid voice list."));
        }
        Ok(payload)
    }

    pub(crate) fn synthesize(text: &str, reference_id: Option<&str>) -> Result<Value, String> {
        let mut body = json!({ "text": text });
        if let Some(reference_id) = reference_id.filter(|value| !value.trim().is_empty()) {
            body["reference_id"] = json!(reference_id);
        }
        crate::music_provider::fish_audio::read_timestamp_stream(request("POST", "tts", Some(body))?)
    }

    fn availability() -> VoiceAvailability {
        match enabled() {
            Ok(true) => {}
            // 未内置网关（开发构建）：沿用本机配音密钥，开关照常显示。
            Ok(false) | Err(_) => {
                return VoiceAvailability { available: true, via_gateway: false, reason: None };
            }
        }
        match list_voices() {
            Ok(_) => VoiceAvailability { available: true, via_gateway: true, reason: None },
            Err(error) => {
                let code = error.split_once(": ").map(|(code, _)| code.to_owned());
                VoiceAvailability {
                    available: code.as_deref() != Some(NOT_CONFIGURED),
                    via_gateway: true,
                    reason: code,
                }
            }
        }
    }

    #[tauri::command]
    pub async fn get_voice_availability() -> VoiceAvailability {
        tauri::async_runtime::spawn_blocking(availability)
            .await
            .unwrap_or(VoiceAvailability {
                available: true,
                via_gateway: false,
                reason: Some(UNAVAILABLE.to_owned()),
            })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn gateway_statuses_map_to_stable_voice_codes() {
            let cases = [
                (429, r#"{"error":"daily_quota_exceeded"}"#, DAILY_QUOTA),
                (429, r#"{"error":"voice_rate_limited"}"#, UNAVAILABLE),
                (503, r#"{"error":"voice_not_configured"}"#, NOT_CONFIGURED),
                (404, "Not Found", NOT_CONFIGURED),
                (503, r#"{"error":"gateway_disabled"}"#, UNAVAILABLE),
                (401, r#"{"error":"login_expired"}"#, AUTH),
                (403, r#"{"error":"trial_inactive"}"#, ENTITLEMENT),
                (426, r#"{"error":"upgrade_required"}"#, UPGRADE_REQUIRED),
                (400, r#"{"error":"voice_rejected_request"}"#, REJECTED),
            ];
            for (status, body, code) in cases {
                assert!(failure(status, body).starts_with(&format!("{code}: ")), "{status} {body}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{allowed, attribution_for, checked_tracks, classify_elevenlabs_http_error, JamendoResponse, JamendoTrack};

    #[test]
    fn suspended_jamendo_application_is_not_an_empty_search_result() {
        let response: JamendoResponse = serde_json::from_str(
            r#"{"headers":{"status":"failed","code":11,"error_message":"Application suspended"},"results":[]}"#,
        ).unwrap();
        let error = checked_tracks(response).err().unwrap();
        assert!(error.contains("code 11"));
    }

    fn track(license_ccurl: &str, audiodownload_allowed: bool) -> JamendoTrack {
        JamendoTrack {
            id: "track-1".to_owned(),
            name: "Safe music".to_owned(),
            artist_name: "Artist".to_owned(),
            duration: 60,
            license_ccurl: license_ccurl.to_owned(),
            audiodownload_allowed,
        }
    }

    #[test]
    fn elevenlabs_http_400_is_classified_without_echoing_the_body() {
        let error = classify_elevenlabs_http_error(
            400,
            r#"{"detail":{"status":"voice_not_found","message":"secret"}}"#,
        );
        assert!(error.contains("voice"));
        assert!(!error.contains("secret"));
        let generic = classify_elevenlabs_http_error(400, r#"{"detail":"bad request"}"#);
        assert_eq!(generic, "ElevenLabs rejected the speech request.");
    }

    #[test]
    fn allows_downloadable_cc0_and_attribution_licenses_only() {
        assert!(allowed(&track(
            "https://creativecommons.org/publicdomain/zero/1.0/",
            true
        )));
        assert!(allowed(&track(
            "http://creativecommons.org/licenses/by/4.0/",
            true
        )));
        assert!(!allowed(&track(
            "https://creativecommons.org/licenses/by-nc/4.0/",
            true
        )));
        assert!(!allowed(&track(
            "https://creativecommons.org/licenses/by-nd/4.0/",
            true
        )));
        assert!(!allowed(&track(
            "https://creativecommons.org/licenses/by/4.0/",
            false
        )));
    }

    #[test]
    fn attribution_keeps_artist_title_and_license_url() {
        let value = attribution_for(&track("https://creativecommons.org/licenses/by/4.0/", true));
        assert!(value.contains("Artist"));
        assert!(value.contains("Safe music"));
        assert!(value.contains("creativecommons.org/licenses/by/4.0"));
    }
}
