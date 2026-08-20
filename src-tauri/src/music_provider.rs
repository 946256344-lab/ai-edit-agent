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
    let mut request = if method == "POST" {
        ureq::post(&url)
    } else {
        ureq::get(&url)
    };
    request = request
        .set("xi-api-key", &api_key)
        .set("Accept", "application/json")
        .timeout(ELEVENLABS_TIMEOUT);
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
        Err(ureq::Error::Transport(transport)) => {
            let message = transport.to_string().to_ascii_lowercase();
            if message.contains("timed out") || message.contains("timeout") {
                Err("ElevenLabs request timed out.".to_owned())
            } else {
                Err("ElevenLabs is unavailable.".to_owned())
            }
        }
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
