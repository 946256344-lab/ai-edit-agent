//! 自定义 OpenAI 兼容 Provider 的配置与 Windows Credential Manager 凭据边界。
//! API Key 只进入系统凭据库，不进入 SQLite、日志或前端持久化。

use keyring::Entry;
use serde::{Deserialize, Serialize};

const CREDENTIAL_SERVICE: &str = "AssemblyVideoAgent";
const CREDENTIAL_ACCOUNT: &str = "custom-model-api";
const CHAT_COMPLETIONS_PATH: &str = "/chat/completions";

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub(crate) struct CustomApiConfig {
    pub(crate) base_url: String,
    pub(crate) model: String,
    #[serde(default)]
    pub(crate) coarse_visual_model: String,
    pub(crate) api_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomApiStatus {
    pub state: String,
    pub message: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub coarse_visual_model: Option<String>,
}

fn trim_endpoint(mut value: &str) -> &str {
    while value.ends_with('/') {
        value = &value[..value.len() - 1];
    }
    value
}

pub(crate) fn chat_endpoint(base_url: &str) -> String {
    format!("{}{CHAT_COMPLETIONS_PATH}", trim_endpoint(base_url))
}

fn credential_entry() -> Result<Entry, String> {
    Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
        .map_err(|_| "Windows Credential Manager is unavailable.".to_owned())
}

fn status_message(error: &str) -> String {
    format!("无法读取自定义 API 凭据：{error}")
}

pub(crate) fn custom_config() -> Result<Option<CustomApiConfig>, String> {
    let raw = match credential_entry()?.get_secret() {
        Ok(raw) => raw,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(error) => return Err(status_message(&error.to_string())),
    };
    serde_json::from_slice::<CustomApiConfig>(&raw)
        .map(Some)
        .map_err(|_| "已保存的自定义 API 凭据无效。".to_owned())
}

fn status_for(config: Option<&CustomApiConfig>) -> CustomApiStatus {
    match config {
        Some(config) => CustomApiStatus {
            state: "connected".to_owned(),
            message: Some("自定义 API 凭据已保存于 Windows 凭据库。".to_owned()),
            base_url: Some(config.base_url.clone()),
            model: Some(config.model.clone()),
            coarse_visual_model: (!config.coarse_visual_model.is_empty())
                .then(|| config.coarse_visual_model.clone()),
        },
        None => CustomApiStatus {
            state: "disconnected".to_owned(),
            message: None,
            base_url: None,
            model: None,
            coarse_visual_model: None,
        },
    }
}

fn with_stored_status() -> CustomApiStatus {
    match custom_config() {
        Ok(Some(config)) => status_for(Some(&config)),
        Ok(None) => status_for(None),
        Err(error) => CustomApiStatus {
            state: "failed".to_owned(),
            message: Some(error),
            base_url: None,
            model: None,
            coarse_visual_model: None,
        },
    }
}

fn store_config(config: &CustomApiConfig) -> Result<(), String> {
    let raw = serde_json::to_vec(config).map_err(|error| error.to_string())?;
    let entry = credential_entry()?;
    entry
        .set_secret(&raw)
        .map_err(|error| format!("无法将自定义 API 凭据保存到 Windows 凭据库：{error}"))?;
    let stored = entry
        .get_secret()
        .map_err(|error| format!("Windows 凭据库无法验证自定义 API 凭据：{error}"))?;
    let stored: CustomApiConfig = serde_json::from_slice(&stored)
        .map_err(|_| "Windows 凭据库返回了无效的自定义 API 凭据。".to_owned())?;
    if stored.base_url != config.base_url
        || stored.model != config.model
        || stored.coarse_visual_model != config.coarse_visual_model
        || stored.api_key != config.api_key
    {
        return Err("Windows 凭据库未保留自定义 API 凭据。".to_owned());
    }
    Ok(())
}

fn validate_input(base_url: &str, model: &str, api_key: &str) -> Result<(), String> {
    if base_url.trim().is_empty() {
        return Err("Base URL 不能为空。".to_owned());
    }
    if model.trim().is_empty() {
        return Err("Model 名称不能为空。".to_owned());
    }
    if api_key.trim().is_empty() {
        return Err("API Key 不能为空。".to_owned());
    }
    Ok(())
}

#[tauri::command]
pub fn get_custom_api_status() -> CustomApiStatus {
    with_stored_status()
}

#[tauri::command]
pub fn save_custom_api(
    base_url: String,
    model: String,
    coarse_visual_model: Option<String>,
    api_key: String,
) -> CustomApiStatus {
    let trimmed_url = trim_endpoint(base_url.trim());
    let trimmed_model = model.trim();
    let trimmed_coarse_visual_model = coarse_visual_model.as_deref().unwrap_or_default().trim();
    let trimmed_key = api_key.trim();
    if let Err(error) = validate_input(trimmed_url, trimmed_model, trimmed_key) {
        return CustomApiStatus {
            state: "failed".to_owned(),
            message: Some(error),
            base_url: None,
            model: None,
            coarse_visual_model: None,
        };
    }
    let config = CustomApiConfig {
        base_url: trimmed_url.to_owned(),
        model: trimmed_model.to_owned(),
        coarse_visual_model: trimmed_coarse_visual_model.to_owned(),
        api_key: trimmed_key.to_owned(),
    };
    match store_config(&config) {
        Ok(()) => status_for(Some(&config)),
        Err(error) => CustomApiStatus {
            state: "failed".to_owned(),
            message: Some(error),
            base_url: None,
            model: None,
            coarse_visual_model: None,
        },
    }
}

#[tauri::command]
pub fn clear_custom_api() -> CustomApiStatus {
    match credential_entry() {
        Ok(entry) => match entry.delete_credential() {
            Ok(()) => status_for(None),
            Err(keyring::Error::NoEntry) => status_for(None),
            Err(error) => CustomApiStatus {
                state: "failed".to_owned(),
                message: Some(format!("无法清除自定义 API 凭据：{error}")),
                base_url: None,
                model: None,
                coarse_visual_model: None,
            },
        },
        Err(error) => CustomApiStatus {
            state: "failed".to_owned(),
            message: Some(format!("无法清除自定义 API 凭据：{error}")),
            base_url: None,
            model: None,
            coarse_visual_model: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_defaults_coarse_visual_model_to_blank() {
        let config: CustomApiConfig = serde_json::from_value(serde_json::json!({
            "base_url": "https://api.example.com/v1",
            "model": "main-model",
            "api_key": "secret"
        }))
        .expect("deserialize legacy custom API config");

        assert!(config.coarse_visual_model.is_empty());
    }

    #[test]
    fn status_exposes_models_without_api_key() {
        let config = CustomApiConfig {
            base_url: "https://api.example.com/v1".to_owned(),
            model: "main-model".to_owned(),
            coarse_visual_model: "coarse-model".to_owned(),
            api_key: "secret".to_owned(),
        };

        let value = serde_json::to_value(status_for(Some(&config))).expect("serialize status");
        assert_eq!(value["model"], "main-model");
        assert_eq!(value["coarseVisualModel"], "coarse-model");
        assert!(value.get("apiKey").is_none());
    }
}
