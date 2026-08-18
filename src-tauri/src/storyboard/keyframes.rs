//! 关键帧网格图提取（预留接口）。
//!
//! 从场景段均匀采样 4-8 帧关键帧，拼成网格图供多模态模型直接理解视频内容。
//! 当前阶段只定义类型和接口，具体 FFmpeg 截图与拼图逻辑留作 TODO。

use crate::models::SceneSegment;
use std::path::PathBuf;

/// 从素材提取关键帧并拼成网格图。
///
/// TODO: 实现关键帧提取逻辑：
/// 1. 根据场景段数量决定采样帧数（4-8 帧）
/// 2. 从每个场景段的中点或均匀采样时间点截取关键帧
/// 3. 使用 FFmpeg 截取帧：`ffmpeg -ss <time> -i <input> -frames:v 1 <output>`
/// 4. 使用 image crate 或 FFmpeg 的 tile filter 拼成 2x2 或 2x4 网格图
/// 5. 保存到 output_path，返回最终路径
///
/// 当前返回空路径错误作为占位。
///
/// # 参数
///
/// - `asset_path`: 素材文件路径
/// - `scene_segments`: 场景段列表，用于确定采样时间点
/// - `output_path`: 输出网格图路径（如 `.cache/<asset_id>_grid.jpg`）
///
/// # 返回
///
/// 成功返回网格图路径，失败返回错误信息。
pub fn extract_keyframe_grid(
    _asset_path: &str,
    _scene_segments: &[SceneSegment],
    _output_path: &str,
) -> Result<PathBuf, String> {
    // TODO: 实现 FFmpeg 截图与拼图逻辑
    // let frame_count = scene_segments.len().clamp(4, 8);
    // let sample_times = distribute_sample_times(scene_segments, frame_count);
    //
    // let mut frames = Vec::new();
    // for time_ms in sample_times {
    //     let frame_path = extract_frame_at(asset_path, time_ms)?;
    //     frames.push(frame_path);
    // }
    //
    // let grid_path = tile_frames_to_grid(&frames, output_path)?;
    // Ok(PathBuf::from(grid_path))

    Err("Keyframe grid extraction not yet implemented.".to_owned())
}

/// 根据场景段和目标帧数分配采样时间点。
///
/// TODO: 实现采样策略：
/// - 优先从每个场景段的中点采样
/// - 场景段数量 > 目标帧数时，选择最长的场景段
/// - 场景段数量 < 目标帧数时，在长场景段内多次采样
fn _distribute_sample_times(_scene_segments: &[SceneSegment], _target_count: usize) -> Vec<i64> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SceneSegment;

    #[test]
    fn extract_keyframe_grid_returns_error_placeholder() {
        let segments = vec![
            SceneSegment {
                start_ms: 0,
                end_ms: 5_000,
                scene_duration_ms: Some(5_000),
                visual_quality_score: None,
            },
            SceneSegment {
                start_ms: 5_000,
                end_ms: 10_000,
                scene_duration_ms: Some(5_000),
                visual_quality_score: None,
            },
        ];

        let result = extract_keyframe_grid("test.mp4", &segments, "output.jpg");

        // 当前阶段返回未实现错误
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not yet implemented"));
    }
}
