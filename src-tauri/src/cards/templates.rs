//! 品牌卡模板注册表：HTML/CSS 模板与 manifest 编译期嵌入二进制，运行时不读外部模板文件。
//! 模型只能选这里列出的 id、只能填 manifest 声明的文案槽位；长度按 manifest 截断。
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const BASE_CSS: &str = include_str!("../../templates/cards/_base.css");
const RUNTIME_JS: &str = include_str!("../../templates/cards/_runtime.js");

const SOURCES: [(&str, &str); 4] = [
    (
        include_str!("../../templates/cards/opening_title/manifest.json"),
        include_str!("../../templates/cards/opening_title/template.html"),
    ),
    (
        include_str!("../../templates/cards/end_card/manifest.json"),
        include_str!("../../templates/cards/end_card/template.html"),
    ),
    (
        include_str!("../../templates/cards/corner_logo/manifest.json"),
        include_str!("../../templates/cards/corner_logo/template.html"),
    ),
    (
        include_str!("../../templates/cards/info_card/manifest.json"),
        include_str!("../../templates/cards/info_card/template.html"),
    ),
];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SlotLimit {
    pub required: bool,
    pub max_words: usize,
    pub max_cjk_chars: usize,
    pub max_chars: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CardManifest {
    pub id: String,
    pub version: u32,
    /// opening | closing | whole | at_shot
    pub anchor: String,
    pub default_duration_ms: i64,
    pub requires_logo: bool,
    pub requires_brand: bool,
    pub purpose: String,
    pub slots: BTreeMap<String, SlotLimit>,
}

pub(crate) struct CardTemplate {
    pub manifest: CardManifest,
    html: &'static str,
}

pub(crate) fn card_templates() -> &'static [CardTemplate] {
    static TEMPLATES: OnceLock<Vec<CardTemplate>> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(manifest, html)| CardTemplate {
                manifest: serde_json::from_str(manifest).expect("bundled card manifest is valid"),
                html,
            })
            .collect()
    })
}

pub(crate) fn card_template(id: &str) -> Option<&'static CardTemplate> {
    card_templates()
        .iter()
        .find(|template| template.manifest.id == id)
}

impl CardTemplate {
    /// 模板内容指纹：模板或共享运行时改动后，旧缓存 PNG 自然失效。
    pub(crate) fn fingerprint(&self) -> String {
        crate::preview_cache::key(&(
            self.manifest.version,
            self.html,
            BASE_CSS,
            RUNTIME_JS,
        ))
    }

    /// 拼出完整页面：CSP 只允许 brand 虚拟主机的图片与字体，数据以 JSON 注入，文案由运行时写 textContent。
    pub(crate) fn compose(&self, card_json: &serde_json::Value, brand_origin: &str) -> String {
        let data = serde_json::to_string(card_json)
            .unwrap_or_else(|_| "{}".to_owned())
            .replace('<', "\\u003c")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        let head = format!(
            "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src {brand_origin}; font-src {brand_origin}; style-src 'unsafe-inline'; script-src 'unsafe-inline'\" />\n<style>{BASE_CSS}</style>\n<script>window.CARD = {data};</script>"
        );
        self.html
            .replace("<!--CARD_HEAD-->", &head)
            .replace("<!--CARD_RUNTIME-->", &format!("<script>{RUNTIME_JS}</script>"))
    }
}

fn is_cjk(character: char) -> bool {
    matches!(character as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

fn is_emoji_or_control(character: char) -> bool {
    character.is_control()
        || matches!(character as u32,
            0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0xFE00..=0xFE0F | 0x200B..=0x200D | 0xE0000..=0xE007F)
}

/// 清洗并截断一段模型文案：去控制字符与 emoji、合并空白；中文按字数、拉丁文按词数，再按字符上限。
/// 返回 (文案, 是否被改动)。空结果返回 None，由调用方决定必填槽位是否报错。
pub(crate) fn clamp_copy(raw: &str, limit: SlotLimit) -> (Option<String>, bool) {
    let cleaned = raw
        .chars()
        .map(|character| if character == '\n' || character == '\r' || character == '\t' { ' ' } else { character })
        .filter(|character| !is_emoji_or_control(*character))
        .collect::<String>();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cjk_count = collapsed.chars().filter(|character| is_cjk(*character)).count();
    let mut clamped = if cjk_count * 2 >= collapsed.chars().filter(|c| !c.is_whitespace()).count() && cjk_count > 0 {
        let mut kept = 0;
        collapsed
            .chars()
            .take_while(|character| {
                if !character.is_whitespace() {
                    kept += 1;
                }
                kept <= limit.max_cjk_chars
            })
            .collect::<String>()
    } else {
        collapsed
            .split(' ')
            .take(limit.max_words)
            .collect::<Vec<_>>()
            .join(" ")
    };
    if clamped.chars().count() > limit.max_chars {
        clamped = clamped.chars().take(limit.max_chars).collect();
        if let Some(space) = clamped.rfind(' ') {
            if space > limit.max_chars / 2 {
                clamped.truncate(space);
            }
        }
    }
    let clamped = clamped
        .trim()
        .trim_end_matches([',', ';', ':', '，', '；', '：', '、', '-', '—'])
        .trim()
        .to_owned();
    let changed = clamped != raw.trim();
    if clamped.is_empty() {
        (None, !raw.trim().is_empty())
    } else {
        (Some(clamped), changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADLINE: SlotLimit = SlotLimit { required: true, max_words: 6, max_cjk_chars: 14, max_chars: 48 };

    #[test]
    fn bundled_templates_parse_and_declare_their_anchor() {
        let ids = card_templates().iter().map(|t| t.manifest.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, ["opening_title", "end_card", "corner_logo", "info_card"]);
        for template in card_templates() {
            assert!(matches!(template.manifest.anchor.as_str(), "opening" | "closing" | "whole" | "at_shot"));
            let page = template.compose(&serde_json::json!({"slots": {"headline": "</script><b>x"}}), "https://brand.voycut.local");
            assert!(!page.contains("<!--CARD_"));
            assert!(!page.contains("</script><b>x"), "slot text must not break out of the data script");
        }
    }

    #[test]
    fn model_copy_is_clamped_by_words_or_cjk_characters() {
        assert_eq!(
            clamp_copy("Weekend road trip along the wild Pacific coast 🚗", HEADLINE),
            (Some("Weekend road trip along the wild".to_owned()), true)
        );
        assert_eq!(
            clamp_copy("周末沿着海岸线一路向北的公路旅行全记录", HEADLINE),
            (Some("周末沿着海岸线一路向北的公路".to_owned()), true)
        );
        assert_eq!(clamp_copy("  Built to last\n", HEADLINE), (Some("Built to last".to_owned()), false));
        assert_eq!(clamp_copy("🔥🔥", HEADLINE), (None, true));
    }
}
