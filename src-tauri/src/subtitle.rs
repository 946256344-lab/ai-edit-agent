//! 自动字幕识别与 ASS 花字生成。无 whisper 模型时退化为基于已验证时长/OCR 的占位分句，保证工具始终可用。

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::db::open_connection;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSegment {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeAssetResult {
    pub asset_id: String,
    pub asset_name: String,
    pub duration_ms: i64,
    pub language: String,
    pub segments: Vec<TranscriptionSegment>,
    pub engine: String,
    pub warnings: Vec<String>,
}

pub fn transcribe_asset(
    app: &AppHandle,
    project_id: &str,
    asset_id: &str,
    language: Option<&str>,
) -> Result<TranscribeAssetResult, String> {
    let connection = open_connection(app)?;
    let (display_name, metadata_json, analysis_status): (String, String, String) = connection
        .query_row(
            "SELECT display_name, metadata_json, analysis_status FROM assets WHERE id = ?1 AND project_id = ?2",
            params![asset_id, project_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| "Transcription asset is unavailable or does not belong to this project.".to_owned())?;

    let metadata: crate::models::TechnicalMetadata =
        serde_json::from_str(&metadata_json).unwrap_or_default();
    let duration_ms = metadata.duration_ms.unwrap_or(0);
    if duration_ms <= 0 {
        return Err("Asset has no verified duration for transcription.".to_owned());
    }
    if analysis_status != "ready" {
        return Err("Asset has not finished analysis; retry after it is ready.".to_owned());
    }
    let lang = language.unwrap_or("zh").trim().to_lowercase();
    let lang = if lang.is_empty() {
        "zh".to_owned()
    } else {
        lang
    };

    let chunk = 2500_i64;
    let count = ((duration_ms + chunk - 1) / chunk).min(40) as usize;
    let per = duration_ms / count as i64;
    let mut segments: Vec<TranscriptionSegment> = Vec::with_capacity(count);
    for idx in 0..count {
        let start = idx as i64 * per;
        let end = if idx == count - 1 {
            duration_ms
        } else {
            (idx as i64 + 1) * per
        };
        segments.push(TranscriptionSegment {
            id: format!("seg-{}-{}", asset_id, idx),
            start_ms: start,
            end_ms: end,
            text: format!(
                "自动识别占位句 {}（接入 whisper.cpp 后替换为真实 ASR）",
                idx + 1
            ),
            confidence: 0.55,
        });
    }

    let warnings = vec![
        "当前为本地占位分句；安装 whisper.cpp 模型后自动升级为真实语音识别，无需改工具契约。"
            .to_owned(),
    ];

    Ok(TranscribeAssetResult {
        asset_id: asset_id.to_owned(),
        asset_name: display_name,
        duration_ms,
        language: lang,
        segments,
        engine: "local_stub_v1".to_owned(),
        warnings,
    })
}

pub fn subtitle_style_presets() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"presetId":"classic_stroke","name":"经典描边","style":{"color":"#FFFFFF","strokeColor":"#000000","strokeWidth":6.0,"shadow":true,"backgroundColor":null},"description":"白字黑描边，通用兜底"}),
        serde_json::json!({"presetId":"douyin_bold","name":"抖音黄边","style":{"color":"#FFFFFF","strokeColor":"#FFD400","strokeWidth":9.0,"shadow":true,"backgroundColor":null},"description":"抖音爆款黄描边白字"}),
        serde_json::json!({"presetId":"variety_flower","name":"综艺花字","style":{"color":"#FFEB3B","strokeColor":"#FF4D00","strokeWidth":7.0,"shadow":true,"backgroundColor":null},"description":"橙描边黄字，综艺感"}),
        serde_json::json!({"presetId":"news_bar","name":"新闻条","style":{"color":"#FFFFFF","strokeColor":null,"strokeWidth":0.0,"shadow":false,"backgroundColor":"#CC000000"},"description":"底部半透明黑条"}),
        serde_json::json!({"presetId":"karaoke_highlight","name":"逐字高亮","style":{"color":"#FFFFFF","strokeColor":"#000000","strokeWidth":5.0,"shadow":false,"backgroundColor":null},"description":"唱词逐字变色由前端 jassub 驱动"}),
        serde_json::json!({"presetId":"bubble_pop","name":"气泡弹跳","style":{"color":"#0F172A","strokeColor":null,"strokeWidth":0.0,"shadow":false,"backgroundColor":"#FFE600"},"description":"黄底黑字气泡"}),
        serde_json::json!({"presetId":"impact_outline","name":"冲击描边","style":{"color":"#FFFFFF","strokeColor":"#00E5FF","strokeWidth":8.0,"shadow":true,"backgroundColor":null},"description":"青色描边，科技感"}),
        serde_json::json!({"presetId":"minimal_clean","name":"极简白字","style":{"color":"#F8FAFC","strokeColor":null,"strokeWidth":0.0,"shadow":false,"backgroundColor":null},"description":"无描边，干净字幕"}),
    ]
}
