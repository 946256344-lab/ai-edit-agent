//! 实验性 OAuth 的 loopback PKCE 流程、令牌刷新与系统凭据存取边界。
//! 这是可替换 Provider 的实验入口，不应表述为官方第三方 OpenAI OAuth。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use keyring::Entry;
use rand::{distr::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::Emitter;
use url::Url;
use uuid::Uuid;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
const CALLBACK_PORT: u16 = 1455;
const CREDENTIAL_SERVICE: &str = "AssemblyVideoAgent";
const CREDENTIAL_ACCOUNT: &str = "experimental-openai-oauth";

#[derive(Clone)]
enum LoginState {
    Idle,
    Pending,
    Connected,
    Failed(String),
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStatus {
    pub state: String,
    pub message: Option<String>,
    pub experimental: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStart {
    pub authorization_url: String,
    pub experimental: bool,
}

#[derive(Deserialize, PartialEq, Serialize)]
struct OAuthCredentials {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    account_id: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    id_token: Option<String>,
}

pub struct AuthorizedOAuth {
    pub access_token: String,
    pub account_id: Option<String>,
}

fn login_state() -> &'static Mutex<LoginState> {
    static STATE: OnceLock<Mutex<LoginState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(LoginState::Idle))
}

fn set_login_state(state: LoginState) {
    if let Ok(mut current) = login_state().lock() {
        *current = state;
    }
}

fn current_status() -> OAuthStatus {
    let state = login_state()
        .lock()
        .map(|state| state.clone())
        .unwrap_or(LoginState::Failed("Login state is unavailable.".to_owned()));
    match state {
        LoginState::Pending => OAuthStatus {
            state: "pending".to_owned(),
            message: Some("Complete sign-in in your browser.".to_owned()),
            experimental: true,
        },
        LoginState::Connected => OAuthStatus {
            state: "connected".to_owned(),
            message: Some("Experimental OpenCode-compatible OAuth.".to_owned()),
            experimental: true,
        },
        LoginState::Failed(message) => OAuthStatus {
            state: "failed".to_owned(),
            message: Some(message),
            experimental: true,
        },
        LoginState::Idle => OAuthStatus {
            state: "disconnected".to_owned(),
            message: None,
            experimental: true,
        },
    }
}

fn credential_entry() -> Result<Entry, String> {
    Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
        .map_err(|_| "Windows Credential Manager is unavailable.".to_owned())
}

fn persisted_credential_status() -> Result<bool, String> {
    match credential_entry()?.get_secret() {
        Ok(raw) => {
            serde_json::from_slice::<OAuthCredentials>(&raw)
                .map_err(|_| "Stored experimental OAuth credentials are invalid.".to_owned())?;
            Ok(true)
        }
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(_) => Err(
            "Windows Credential Manager could not read experimental OAuth credentials.".to_owned(),
        ),
    }
}

fn load_credentials() -> Result<OAuthCredentials, String> {
    let raw = credential_entry()?
        .get_secret()
        .map_err(|_| "Experimental OAuth is not connected.".to_owned())?;
    serde_json::from_slice(&raw)
        .map_err(|_| "Experimental OAuth credentials are invalid.".to_owned())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn pkce_verifier() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn account_id(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    claims
        .get("chatgpt_account_id")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth")
                .and_then(|value| value.get("chatgpt_account_id"))
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
}

fn authorization_url(state: &str, verifier: &str) -> String {
    let mut url = Url::parse(&format!("{ISSUER}/oauth/authorize")).expect("valid issuer URL");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair(
            "redirect_uri",
            &format!("http://localhost:{CALLBACK_PORT}/auth/callback"),
        )
        .append_pair("scope", "openid profile email offline_access")
        .append_pair("code_challenge", &pkce_challenge(verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", "opencode")
        .append_pair("state", state);
    url.to_string()
}

fn exchange_code(code: &str, verifier: &str) -> Result<OAuthCredentials, String> {
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", code)
        .append_pair(
            "redirect_uri",
            &format!("http://localhost:{CALLBACK_PORT}/auth/callback"),
        )
        .append_pair("client_id", CLIENT_ID)
        .append_pair("code_verifier", verifier)
        .finish();
    let response = ureq::post(&format!("{ISSUER}/oauth/token"))
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&form)
        .map_err(|_| "Token exchange failed.".to_owned())?;
    let body = response
        .into_string()
        .map_err(|_| "Token exchange returned no data.".to_owned())?;
    let tokens: TokenResponse = serde_json::from_str(&body)
        .map_err(|_| "Token exchange returned invalid data.".to_owned())?;
    Ok(OAuthCredentials {
        account_id: tokens
            .id_token
            .as_deref()
            .and_then(account_id)
            .or_else(|| account_id(&tokens.access_token)),
        access_token: tokens.access_token,
        refresh_token: tokens
            .refresh_token
            .ok_or_else(|| "Token exchange did not return a refresh token.".to_owned())?,
        expires_at: now_millis() + tokens.expires_in.unwrap_or(3600) * 1000,
    })
}

fn refresh_credentials(credentials: OAuthCredentials) -> Result<OAuthCredentials, String> {
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("refresh_token", &credentials.refresh_token)
        .append_pair("client_id", CLIENT_ID)
        .finish();
    let response = ureq::post(&format!("{ISSUER}/oauth/token"))
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&form)
        .map_err(|_| "OAuth token refresh failed. Sign in again.".to_owned())?;
    let body = response
        .into_string()
        .map_err(|_| "OAuth token refresh returned no data.".to_owned())?;
    let tokens: TokenResponse = serde_json::from_str(&body)
        .map_err(|_| "OAuth token refresh returned invalid data.".to_owned())?;
    Ok(OAuthCredentials {
        account_id: tokens
            .id_token
            .as_deref()
            .and_then(account_id)
            .or(credentials.account_id),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.unwrap_or(credentials.refresh_token),
        expires_at: now_millis() + tokens.expires_in.unwrap_or(3600) * 1000,
    })
}

pub fn experimental_access() -> Result<AuthorizedOAuth, String> {
    let mut credentials = load_credentials()?;
    if credentials.expires_at <= now_millis() + 60_000 {
        credentials = refresh_credentials(credentials)?;
        store_credentials(&credentials)?;
    }
    Ok(AuthorizedOAuth {
        access_token: credentials.access_token,
        account_id: credentials.account_id,
    })
}

fn store_credentials(credentials: &OAuthCredentials) -> Result<(), String> {
    let raw = serde_json::to_vec(credentials)
        .map_err(|_| "Could not secure OAuth credentials.".to_owned())?;
    let entry = credential_entry()?;
    entry.set_secret(&raw).map_err(|error| {
        format!("Could not save OAuth credentials to Windows Credential Manager: {error}")
    })?;

    // Confirm the credential survives a separate read before reporting a successful login.
    let stored = entry.get_secret().map_err(|error| {
        format!(
            "Windows Credential Manager could not verify experimental OAuth credentials: {error}"
        )
    })?;
    let stored: OAuthCredentials = serde_json::from_slice(&stored).map_err(|_| {
        "Windows Credential Manager returned invalid experimental OAuth credentials.".to_owned()
    })?;
    if stored != *credentials {
        return Err(
            "Windows Credential Manager did not retain experimental OAuth credentials.".to_owned(),
        );
    }
    Ok(())
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes());
}

fn emit_status(app: &tauri::AppHandle) {
    let _ = app.emit("experimental-openai-oauth-status", current_status());
}

fn wait_for_callback(
    app: tauri::AppHandle,
    listener: TcpListener,
    state: String,
    verifier: String,
) {
    let _ = listener.set_nonblocking(true);
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut request = [0_u8; 8192];
                let read = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..read]);
                let target = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1));
                let result = target
                    .and_then(|target| {
                        Url::parse(&format!("http://localhost:{CALLBACK_PORT}{target}")).ok()
                    })
                    .ok_or_else(|| "OAuth callback is invalid.".to_owned())
                    .and_then(|url| {
                        if url.path() != "/auth/callback"
                            || url
                                .query_pairs()
                                .find(|(key, _)| key == "state")
                                .map(|(_, value)| value)
                                != Some(state.clone().into())
                        {
                            return Err("OAuth callback state did not match.".to_owned());
                        }
                        let code = url
                            .query_pairs()
                            .find(|(key, _)| key == "code")
                            .map(|(_, value)| value.into_owned())
                            .ok_or_else(|| "OAuth callback did not include a code.".to_owned())?;
                        exchange_code(&code, &verifier)
                            .and_then(|credentials| store_credentials(&credentials))
                    });
                match result {
                    Ok(()) => {
                        set_login_state(LoginState::Connected);
                        emit_status(&app);
                        respond(
                            &mut stream,
                            "200 OK",
                            "<p>Assembly Video Agent connected and saved the credential. You can close this tab.</p>",
                        );
                    }
                    Err(error) => {
                        set_login_state(LoginState::Failed(error.clone()));
                        emit_status(&app);
                        respond(
                            &mut stream,
                            "400 Bad Request",
                            "<p>Connection failed. Return to Assembly Video Agent.</p>",
                        );
                    }
                }
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(_) => {
                set_login_state(LoginState::Failed(
                    "OAuth callback listener failed.".to_owned(),
                ));
                emit_status(&app);
                return;
            }
        }
    }
    set_login_state(LoginState::Failed("OAuth sign-in timed out.".to_owned()));
    emit_status(&app);
}

#[tauri::command]
pub fn get_experimental_openai_oauth_status() -> OAuthStatus {
    if matches!(
        login_state().lock().ok().as_deref(),
        Some(LoginState::Pending)
    ) {
        return current_status();
    }
    match persisted_credential_status() {
        Ok(true) => set_login_state(LoginState::Connected),
        Ok(false) => set_login_state(LoginState::Idle),
        Err(error) => set_login_state(LoginState::Failed(error)),
    }
    current_status()
}

#[tauri::command]
pub fn start_experimental_openai_oauth(app: tauri::AppHandle) -> Result<OAuthStart, String> {
    if matches!(
        login_state().lock().ok().as_deref(),
        Some(LoginState::Pending)
    ) {
        return Err("OAuth sign-in is already pending.".to_owned());
    }
    let listener = TcpListener::bind(("127.0.0.1", CALLBACK_PORT)).map_err(|_| "The OAuth callback port is unavailable. Close another OpenCode-compatible login and try again.".to_owned())?;
    let state = Uuid::new_v4().to_string();
    let verifier = pkce_verifier();
    let url = authorization_url(&state, &verifier);
    set_login_state(LoginState::Pending);
    thread::spawn(move || wait_for_callback(app, listener, state, verifier));
    Ok(OAuthStart {
        authorization_url: url,
        experimental: true,
    })
}

#[tauri::command]
pub fn clear_experimental_openai_oauth() -> OAuthStatus {
    match credential_entry() {
        Ok(entry) => match entry.delete_credential() {
            Ok(()) => set_login_state(LoginState::Idle),
            Err(keyring::Error::NoEntry) => set_login_state(LoginState::Idle),
            Err(error) => set_login_state(LoginState::Failed(format!(
                "Could not clear experimental OAuth credentials: {error}"
            ))),
        },
        Err(error) => set_login_state(LoginState::Failed(error)),
    }
    current_status()
}
