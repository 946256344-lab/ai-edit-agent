//! 出站 HTTP 公共边界：配音等境外 API 读取环境代理，并保留可诊断的传输失败分类。
//!
//! `ureq` 默认直连，不读 `HTTPS_PROXY`。本机若只通过本地代理访问 `api.fish.audio`
//! 等主机，直连会变成 Windows 连接超时，被旧逻辑笼统写成 unavailable。

use std::sync::OnceLock;
use ureq::{Agent, AgentBuilder, Proxy};

/// 配音 Provider 共用的出站 Agent；进程内只构建一次。
pub(crate) fn voice_agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let mut builder = AgentBuilder::new();
        if let Some(proxy_url) = env_http_proxy() {
            match Proxy::new(&proxy_url) {
                Ok(proxy) => {
                    log::info!("Voice HTTP client will use the process proxy environment.");
                    builder = builder.proxy(proxy);
                }
                Err(error) => {
                    log::warn!("Ignoring invalid voice HTTP proxy environment: {error}");
                }
            }
        }
        builder.build()
    })
}

fn env_http_proxy() -> Option<String> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

/// 将 ureq 传输错误收成稳定对外文案，并写入真实传输细节便于排障。
pub(crate) fn classify_voice_transport(provider: &str, transport: &ureq::Transport) -> String {
    let detail = transport.to_string();
    log::warn!("{provider} transport failure: {detail}");
    classify_voice_transport_message(provider, &detail)
}

pub(crate) fn classify_voice_transport_message(provider: &str, detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    // 中文 Windows 的连接超时文案不含英文 timeout，但带 os error 10060。
    if lower.contains("timed out")
        || lower.contains("timeout")
        || detail.contains("10060")
        || detail.contains("没有正确答复")
        || detail.contains("连接尝试失败")
    {
        return format!("{provider} request timed out.");
    }
    format!("{provider} is unavailable.")
}

#[cfg(test)]
mod tests {
    use super::classify_voice_transport_message;

    #[test]
    fn chinese_windows_connect_timeout_is_classified_as_timeout() {
        let detail = "https://api.fish.audio/model: Connection Failed: Connect error: 由于连接方在一段时间后没有正确答复或连接的主机没有反应，连接尝试失败。 (os error 10060)";
        assert_eq!(
            classify_voice_transport_message("Fish Audio", detail),
            "Fish Audio request timed out."
        );
    }

    #[test]
    fn english_timeout_stays_timeout() {
        assert_eq!(
            classify_voice_transport_message("ElevenLabs", "request timed out"),
            "ElevenLabs request timed out."
        );
    }

    #[test]
    fn other_transport_stays_unavailable() {
        assert_eq!(
            classify_voice_transport_message("Fish Audio", "Connection reset by peer"),
            "Fish Audio is unavailable."
        );
    }
}
