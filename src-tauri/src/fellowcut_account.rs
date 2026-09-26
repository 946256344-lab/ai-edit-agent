//! FellowCut 桌面账号与登录令牌；模型调用的资格由服务端网关校验。

use keyring::Entry;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const API_KEY: &str = "AIzaSyBNaiSDQE_PgIKzyzxQoh2kl5KgUjzINiU";
const PROJECT_ID: &str = "voycut-d8101";
const CREDENTIAL_SERVICE: &str = "AssemblyVideoAgent";
const CREDENTIAL_ACCOUNT: &str = "fellowcut-firebase-refresh-token";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FellowCutAccountStatus {
    state: &'static str,
    email: Option<String>,
    entitlement: Option<String>,
    trial_started_at: Option<String>,
    /// 网站账号页（注册、验证邮箱、找回密码），由内置网关地址推出；未配置网关时为空。
    account_page_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignInResponse {
    id_token: String,
    refresh_token: String,
}

#[derive(Deserialize)]
struct RefreshResponse {
    id_token: String,
    refresh_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountInfo {
    users: Vec<AccountUser>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountUser {
    local_id: String,
    email: String,
    email_verified: bool,
}

/// 账号错误带稳定码前缀（`account_*: 中文原文`），前端按码显示当前界面语言；
/// 中文原文作为未知码的回落与日志。
fn account_error(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}

fn credential_entry() -> Result<Entry, String> {
    Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
        .map_err(|_| account_error("account_credential_store", "Windows 凭据库不可用。"))
}

fn saved_refresh_token() -> Result<Option<String>, String> {
    match credential_entry()?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(account_error("account_credential_store", "无法读取 Voycut 登录凭据。")),
    }
}

fn save_refresh_token(token: &str) -> Result<(), String> {
    credential_entry()?
        .set_password(token)
        .map_err(|_| account_error("account_credential_store", "无法保存 Voycut 登录凭据。"))
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build()
}

fn auth_url(method: &str) -> String {
    format!("https://identitytoolkit.googleapis.com/v1/accounts:{method}?key={API_KEY}")
}

fn response_json<T: DeserializeOwned>(response: ureq::Response) -> Result<T, String> {
    let body = response
        .into_string()
        .map_err(|_| account_error("account_service_invalid", "账号服务返回了无效数据。"))?;
    serde_json::from_str(&body).map_err(|_| account_error("account_service_invalid", "账号服务返回了无效数据。"))
}

fn account_info(agent: &ureq::Agent, id_token: &str) -> Result<AccountUser, String> {
    let response = agent
        .post(&auth_url("lookup"))
        .set("Content-Type", "application/json")
        .send_string(&json!({ "idToken": id_token }).to_string())
        .map_err(|_| account_error("account_verify_failed", "无法核验 Voycut 账号，请检查网络或重新登录。"))?;
    let response: AccountInfo = response_json(response)?;
    response
        .users
        .into_iter()
        .next()
        .ok_or_else(|| account_error("account_not_found", "账号不存在，请重新登录。"))
}

fn entitlement(
    agent: &ureq::Agent,
    id_token: &str,
    uid: &str,
) -> Result<Option<(String, Option<String>)>, String> {
    let url = format!(
        "https://firestore.googleapis.com/v1/projects/{PROJECT_ID}/databases/(default)/documents/entitlements/{uid}"
    );
    let response = match agent
        .get(&url)
        .set("Authorization", &format!("Bearer {id_token}"))
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(_) => return Err(account_error("account_entitlement_unavailable", "无法读取试用资格，请稍后重试。")),
    };
    let document: Value = response_json(response)?;
    let status = document["fields"]["status"]["stringValue"]
        .as_str()
        .ok_or_else(|| account_error("account_service_invalid", "试用资格数据无效。"))?;
    let started_at = document["fields"]["startedAt"]["timestampValue"]
        .as_str()
        .map(str::to_owned);
    Ok(Some((status.to_owned(), started_at)))
}

fn status_from_token(
    agent: &ureq::Agent,
    id_token: &str,
) -> Result<FellowCutAccountStatus, String> {
    let user = account_info(agent, id_token)?;
    if !user.email_verified {
        return Ok(FellowCutAccountStatus {
            state: "unverified",
            email: Some(user.email),
            entitlement: None,
            trial_started_at: None,
            account_page_url: account_page_url(),
        });
    }
    let access = entitlement(agent, id_token, &user.local_id)?;
    Ok(FellowCutAccountStatus {
        state: "verified",
        email: Some(user.email),
        entitlement: access.as_ref().map(|value| value.0.clone()),
        trial_started_at: access.and_then(|value| value.1),
        account_page_url: account_page_url(),
    })
}

fn sign_in(email: String, password: String) -> Result<FellowCutAccountStatus, String> {
    if email.trim().is_empty() || password.is_empty() {
        return Err(account_error("account_missing_credentials", "请输入邮箱和密码。"));
    }
    let agent = http_agent();
    let response = agent
        .post(&auth_url("signInWithPassword"))
        .set("Content-Type", "application/json")
        .send_string(
            &json!({ "email": email.trim(), "password": password, "returnSecureToken": true })
                .to_string(),
        )
        .map_err(|error| match error {
            ureq::Error::Status(400, _) => account_error("account_invalid_credentials", "邮箱或密码不正确。"),
            _ => account_error("account_sign_in_unavailable", "登录服务暂时不可用，请检查网络。"),
        })?;
    let response: SignInResponse = response_json(response)?;
    let status = status_from_token(&agent, &response.id_token)?;
    save_refresh_token(&response.refresh_token)?;
    Ok(status)
}

fn get_status() -> Result<FellowCutAccountStatus, String> {
    if saved_refresh_token()?.is_none() {
        return Ok(FellowCutAccountStatus {
            state: "signedOut",
            email: None,
            entitlement: None,
            trial_started_at: None,
            account_page_url: account_page_url(),
        });
    };
    let agent = http_agent();
    let id_token = fresh_id_token()?;
    status_from_token(&agent, &id_token)
}

pub(crate) fn fresh_id_token() -> Result<String, String> {
    let refresh_token = saved_refresh_token()?
        .ok_or_else(|| account_error("account_signed_out", "请先登录 Voycut 账号。"))?;
    let agent = http_agent();
    let response = agent
        .post(&format!(
            "https://securetoken.googleapis.com/v1/token?key={API_KEY}"
        ))
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(
            &url::form_urlencoded::Serializer::new(String::new())
                .append_pair("grant_type", "refresh_token")
                .append_pair("refresh_token", &refresh_token)
                .finish(),
        )
        .map_err(|_| account_error("account_session_expired", "登录已失效或网络不可用，请重新登录。"))?;
    let response: RefreshResponse = response_json(response)?;
    if response.refresh_token != refresh_token {
        save_refresh_token(&response.refresh_token)?;
    }
    Ok(response.id_token)
}

/// 网关与网站同域（`https://<站点>/api/model`），账号页固定在站点根下的 `account.html`。
fn account_page_url() -> Option<String> {
    let base_url = gateway_base_url().ok().flatten()?;
    Some(format!("{}/account.html", base_url.strip_suffix("/api/model")?))
}

/// 正式构建必须内置公开网关地址；开发构建可用环境变量连接本地网关。
pub(crate) fn gateway_base_url() -> Result<Option<String>, String> {
    let configured = if cfg!(debug_assertions) {
        std::env::var("FELLOWCUT_GATEWAY_BASE_URL")
            .ok()
            .or_else(|| option_env!("FELLOWCUT_GATEWAY_BASE_URL").map(str::to_owned))
    } else {
        option_env!("FELLOWCUT_GATEWAY_BASE_URL").map(str::to_owned)
    };
    let Some(base_url) = configured else {
        return if cfg!(debug_assertions) {
            Ok(None)
        } else {
            Err("provider_gateway_not_configured: This build has no Voycut model service configured.".to_owned())
        };
    };
    let url = url::Url::parse(&base_url)
        .map_err(|_| "provider_gateway_not_configured: The Voycut model service address in this build is invalid.".to_owned())?;
    let local_dev = cfg!(debug_assertions)
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost"));
    if (url.scheme() != "https" && !local_dev)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().trim_end_matches('/') != "/api/model"
    {
        return Err("provider_gateway_not_configured: The Voycut model service address in this build is invalid.".to_owned());
    }
    Ok(Some(base_url.trim_end_matches('/').to_owned()))
}

#[tauri::command]
pub async fn sign_in_fellowcut(
    email: String,
    password: String,
) -> Result<FellowCutAccountStatus, String> {
    tauri::async_runtime::spawn_blocking(move || sign_in(email, password))
        .await
        .map_err(|_| account_error("account_task_interrupted", "登录任务未完成。"))?
}

#[tauri::command]
pub async fn get_fellowcut_account_status() -> Result<FellowCutAccountStatus, String> {
    tauri::async_runtime::spawn_blocking(get_status)
        .await
        .map_err(|_| account_error("account_task_interrupted", "账号状态读取未完成。"))?
}

#[tauri::command]
pub fn sign_out_fellowcut() -> Result<FellowCutAccountStatus, String> {
    match credential_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(FellowCutAccountStatus {
            state: "signedOut",
            email: None,
            entitlement: None,
            trial_started_at: None,
            account_page_url: account_page_url(),
        }),
        Err(_) => Err(account_error("account_credential_store", "无法清除 Voycut 登录凭据。")),
    }
}
