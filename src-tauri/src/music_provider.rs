//! Jamendo 音乐搜索、授权资格、下载与凭据状态适配器。
//! 下载后的音频仍通过素材模块登记并进入本地分析与审计流程。

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

#[cfg(test)]
mod tests {
    use super::{allowed, attribution_for, JamendoTrack};

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
