//! 多模态选镜：关键帧网格生成与视觉输入构建。
//!
//! 负责从已提取的关键帧拼接成网格图，并为模型准备多模态输入。
//! 与评分模块（scoring.rs）协作：评分模块初筛 top-5 候选，本模块为这些候选
//! 生成视觉证据，让模型直接从画面判断语义匹配度。

use crate::models::StoryboardSource;
use image::{ImageBuffer, Rgb, RgbImage};
use std::path::{Path, PathBuf};

/// 关键帧网格配置：每个视频提取多少帧、拼成几行几列。
#[derive(Debug, Clone)]
pub struct KeyframeGridConfig {
    /// 提取的关键帧数量（固定 4 帧）
    pub frame_count: usize,
    /// 网格列数（2 列适合 2x2 布局）
    pub grid_columns: usize,
    /// 单帧缩略图宽度（像素）
    pub thumbnail_width: u32,
    /// 单帧缩略图高度（像素）
    pub thumbnail_height: u32,
}

impl Default for KeyframeGridConfig {
    fn default() -> Self {
        Self {
            frame_count: 4,       // 固定 4 帧
            grid_columns: 2,      // 2x2 网格
            thumbnail_width: 320, // 单帧 320x180
            thumbnail_height: 180,
        }
    }
}

/// 为单个素材生成关键帧网格图。
///
/// 从已提取的 4 个关键帧 JPG 文件（keyframe_001.jpg ~ keyframe_004.jpg）
/// 拼接成一张 2x2 网格图，保存到 <derived_dir>/<asset_id>_grid.jpg。
///
/// 参数：
/// - asset_id: 素材 ID
/// - keyframe_paths: 已提取的关键帧路径列表（按时间顺序）
/// - derived_dir: 派生数据目录
/// - config: 网格配置
///
/// 返回网格图路径，如果关键帧不足或拼接失败则返回 None。
pub fn generate_keyframe_grid(
    asset_id: &str,
    keyframe_paths: &[String],
    derived_dir: &Path,
    config: &KeyframeGridConfig,
) -> Result<Option<PathBuf>, String> {
    if keyframe_paths.is_empty() {
        return Ok(None);
    }

    // 限制最多使用 config.frame_count 个帧
    let paths_to_use = &keyframe_paths[..keyframe_paths.len().min(config.frame_count)];
    let rows = (paths_to_use.len() + config.grid_columns - 1) / config.grid_columns;

    let grid_width = config.grid_columns as u32 * config.thumbnail_width;
    let grid_height = rows as u32 * config.thumbnail_height;

    // 创建空白画布（黑色背景）
    let mut grid: RgbImage = ImageBuffer::from_pixel(grid_width, grid_height, Rgb([0, 0, 0]));

    // 逐帧加载并粘贴到网格位置
    for (index, path) in paths_to_use.iter().enumerate() {
        let img = image::open(path)
            .map_err(|e| format!("Failed to open keyframe {}: {}", path, e))?
            .to_rgb8();

        // 调整大小到目标尺寸
        let resized = image::imageops::resize(
            &img,
            config.thumbnail_width,
            config.thumbnail_height,
            image::imageops::FilterType::Lanczos3,
        );

        // 计算粘贴位置
        let col = index % config.grid_columns;
        let row = index / config.grid_columns;
        let x = col as u32 * config.thumbnail_width;
        let y = row as u32 * config.thumbnail_height;

        // 粘贴到网格
        image::imageops::replace(&mut grid, &resized, x.into(), y.into());
    }

    // 保存网格图
    let grid_path = derived_dir.join(format!("{}_grid.jpg", asset_id));
    grid.save(&grid_path)
        .map_err(|e| format!("Failed to save keyframe grid: {}", e))?;

    Ok(Some(grid_path))
}

/// 为 top-N 候选构建多模态输入内容块。
///
/// 返回 Vec<serde_json::Value> 供 Provider 多模态请求使用。
/// 每个候选的内容块顺序：
/// 1. 如果有 keyframe_grid_path，读取图像并 base64 编码为 image block
/// 2. 文本 block 包含 assetId、duration、sceneSegments 元数据
pub fn build_multimodal_content(
    candidates: &[StoryboardSource],
) -> Result<Vec<serde_json::Value>, String> {
    use base64::{engine::general_purpose, Engine as _};

    let mut blocks = Vec::new();

    for candidate in candidates {
        // 如果有网格图路径，添加图像块
        if let Some(grid_path) = &candidate.keyframe_grid_path {
            let image_data = std::fs::read(grid_path)
                .map_err(|e| format!("Failed to read keyframe grid {}: {}", grid_path, e))?;
            let base64_data = general_purpose::STANDARD.encode(&image_data);

            blocks.push(serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/jpeg",
                    "data": base64_data
                }
            }));
        }

        // 添加元数据文本块
        let scene_info = candidate
            .scene_segments
            .iter()
            .map(|seg| format!("{}ms-{}ms", seg.start_ms, seg.end_ms))
            .collect::<Vec<_>>()
            .join(", ");

        let metadata_text = format!(
            "Asset ID: {}\nDuration: {}ms\nScene segments: {}",
            candidate.asset_id,
            candidate.duration_ms.unwrap_or(0),
            if scene_info.is_empty() {
                "none"
            } else {
                &scene_info
            }
        );

        blocks.push(serde_json::json!({
            "type": "text",
            "text": metadata_text
        }));
    }

    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_2x2_grid() {
        let config = KeyframeGridConfig::default();
        assert_eq!(config.frame_count, 4);
        assert_eq!(config.grid_columns, 2);
        assert_eq!(config.thumbnail_width, 320);
        assert_eq!(config.thumbnail_height, 180);
    }

    #[test]
    fn build_multimodal_content_returns_empty_for_no_candidates() {
        let candidates = vec![];
        let result = build_multimodal_content(&candidates);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
