//! 对抗验证框架（预留接口）。
//!
//! 定义独立验证模型审查 storyboard 选镜的接口，让模型在生成后自我纠错。
//! 当前阶段只定义类型和接口，具体验证循环集成留作 TODO。

use crate::models::StoryboardContent;
use crate::provider::ModelAccess;

/// 验证结果：通过或需要修订。
#[derive(Debug, Clone)]
pub enum ValidationResult {
    /// 所有镜头通过验证，可以继续。
    Approved,
    /// 部分镜头被拒绝，需要重新选择。
    NeedsRevision(Vec<RejectedBeat>),
}

/// 被拒绝的 beat，包含拒绝原因。
#[derive(Debug, Clone)]
pub struct RejectedBeat {
    pub beat_id: String,
    /// 为什么这个镜头不符合用户要求的具体原因。
    pub reason: String,
}

/// 对抗验证：独立模型逐个审查 storyboard 已选镜头。
///
/// TODO: 实现验证循环：
/// 1. 构建验证提示词，包含用户 brief 和当前选择的镜头
/// 2. 为每个镜头提供关键帧网格图（如果可用），让验证模型直接从画面判断是否匹配 brief
/// 3. 请求独立模型（或同一模型的审查角色）判断每个镜头是否真的符合要求
/// 4. 解析返回的拒绝列表
/// 5. 返回 ValidationResult
///
/// 当前返回 Approved 作为占位。
pub fn verify_storyboard_selections(
    _access: &ModelAccess,
    _candidate: &StoryboardContent,
    _original_brief: &str,
    _asset_keyframe_grids: &std::collections::HashMap<String, String>,
) -> Result<ValidationResult, String> {
    // TODO: 实现对抗验证
    // let verification_prompt = format!(
    //     "你是视频质量审查员。用户要求：{}\n\
    //      当前选择的镜头：\n{}\n\n\
    //      请逐个检查每个镜头是否真的符合用户要求。\
    //      只返回不合格的 beat_id 和具体原因。",
    //     original_brief,
    //     format_beats_for_review(candidate)
    // );
    //
    // // 为每个镜头添加关键帧网格图（如果可用）
    // for shot in &candidate.shots {
    //     if let Some(grid_path) = asset_keyframe_grids.get(&shot.asset_id) {
    //         // 添加图像到验证请求
    //     }
    // }
    //
    // let response = post_model_payload(access, &request, timeout)?;
    // parse_rejected_beats(&response)

    Ok(ValidationResult::Approved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{StoryboardBeat, StoryboardShot};

    #[test]
    fn verify_storyboard_returns_approved_placeholder() {
        let content = StoryboardContent {
            brief: "Create a test video".to_owned(),
            title: "Test".to_owned(),
            summary: "Test summary".to_owned(),
            target_duration_ms: 10_000,
            script_mode: "full_script".to_owned(),
            beats: vec![StoryboardBeat {
                id: "beat-1".to_owned(),
                purpose: "test".to_owned(),
                required_visual: "test visual".to_owned(),
            }],
            uncovered_beat_ids: vec![],
            shots: vec![StoryboardShot {
                order_index: 1,
                duration_ms: 5_000,
                purpose: "test shot".to_owned(),
                on_screen_text: String::new(),
                asset_id: "asset-1".to_owned(),
                source_start_ms: 0,
                source_end_ms: 5_000,
                reason: "test reason".to_owned(),
                beat_id: "beat-1".to_owned(),
                match_level: "direct".to_owned(),
            }],
        };

        // 模拟 ModelAccess（当前阶段不实际调用）
        let access = match crate::provider::ModelAccess::resolve() {
            Ok(access) => access,
            Err(_) => return, // 无可用 Provider 时跳过测试
        };
        let grids = std::collections::HashMap::new();
        let result = verify_storyboard_selections(&access, &content, "Create a test video", &grids);

        // 当前阶段返回 Approved
        assert!(matches!(result, Ok(ValidationResult::Approved)));
    }
}
