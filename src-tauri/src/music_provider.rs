//! Jamendo 音乐与 ElevenLabs 配音的凭据、有界 HTTP 适配器。
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
    results: Vec<JamendoTrack>,
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
    catalog
        .results
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
    Ok(response.results.into_iter().filter(allowed).collect())
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
pub fn clear_elevenlabs_api_key() -> ElevenLabsStatus {
    if let Ok(entry) = elevenlabs_entry() {
        let _ = entry.delete_credential();
    }
    get_elevenlabs_status()
}

#[tauri::command]
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

    pub(crate) fn synthesize(text: &str, reference_id: Option<&str>) -> Result<Value, String> {
        let mut body =
            json!({ "text": text, "format": "mp3", "sample_rate": 44100, "mp3_bitrate": 128 });
        if let Some(reference_id) = reference_id.filter(|value| !value.trim().is_empty()) {
            body["reference_id"] = json!(reference_id);
        }
        let response = request("POST", "/v1/tts/stream/with-timestamp", Some(body))?;
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
        let segments = alignments
            .into_values()
            .flat_map(|alignment| {
                let offset = alignment
                    .get("offset")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                alignment
                    .get("segments")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |mut segment| {
                        if let Some(start) = segment.get("start").and_then(Value::as_f64) {
                            segment["start"] = json!(start + offset);
                        }
                        if let Some(end) = segment.get("end").and_then(Value::as_f64) {
                            segment["end"] = json!(end + offset);
                        }
                        segment
                    })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "audio_base64": base64::engine::general_purpose::STANDARD.encode(audio),
            "alignment": { "segments": segments }
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

    #[tauri::command]
    pub fn get_fish_audio_status() -> FishAudioStatus {
        status()
    }

    #[tauri::command]
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

    #[tauri::command]
    pub fn clear_fish_audio_api_key() -> FishAudioStatus {
        if let Ok(entry) = entry() {
            let _ = entry.delete_credential();
        }
        status()
    }

    #[tauri::command]
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

#[cfg(test)]
mod tests {
    use super::{allowed, attribution_for, classify_elevenlabs_http_error, JamendoTrack};

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
