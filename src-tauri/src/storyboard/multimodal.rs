//! 多模态选镜：关键帧网格生成与视觉输入构建。
//!
//! 负责从已提取的关键帧拼接成网格图，并为模型准备多模态输入。
//! Phase 3 附带候选 2×2 网格；Phase 4 用导入关键帧建粗窗，再在窗内加密抽帧。

use crate::models::StoryboardSource;
use crate::process::{hidden_command, run_hidden_command_with_timeout};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{ImageBuffer, Rgb, RgbImage};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const PHASE4_FRAME_FFMPEG_TIMEOUT: Duration = Duration::from_secs(20);
/// 单素材最多保留多少个内容候选窗（每窗 1 张代表帧给选段）。
const PHASE4_MAX_WINDOWS_PER_ASSET: usize = 12;
/// Pass A 单次请求最多附带多少张窗中点帧，避免与 Pass B 同类的网关断连。
pub(crate) const PHASE4_PASS_A_MAX_IMAGES: usize = 40;
/// 段内精修默认抽帧数 / 不确定时加密码。
pub(crate) const PHASE4_REFINE_FRAMES: usize = 6;
pub(crate) const PHASE4_UNCERTAIN_FRAMES: usize = 10;
/// Pass B/C 每批最多精修多少镜；每镜仍抽满上列帧数，再拼成一张网格，避免单次 100+ 图断连。
pub(crate) const PHASE4_REFINE_SHOTS_PER_BATCH: usize = 10;
/// Pass B 窗内帧间距超过该值时，Pass C 围绕精修子区间再加密收窄。
pub(crate) const PHASE4_MAX_FRAME_SPACING_MS: i64 = 1_500;
/// Phase 3 单次请求最多附带多少张候选网格，避免体量失控。
pub(crate) const PHASE3_MAX_GRID_IMAGES: usize = 60;

/// Phase 4 内容候选窗：导入关键帧/三分段划出的一段可用素材。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Phase4ContentWindow {
    pub window_id: String,
    pub asset_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

impl Phase4ContentWindow {
    pub(crate) fn mid_ms(&self) -> i64 {
        self.start_ms + (self.end_ms - self.start_ms).max(0) / 2
    }

    pub(crate) fn span_ms(&self) -> i64 {
        (self.end_ms - self.start_ms).max(0)
    }
}

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

/// 读取 JPEG 为 Responses API `input_image` 块；失败返回 None（调用方跳过该图）。
pub(crate) fn read_input_image(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(json!({
        "type": "input_image",
        "image_url": format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))
    }))
}

/// 把同一镜的多张定时帧拼成一张网格，时间采样点数不变，只减少请求里的 image 块数量。
pub(crate) fn compose_timed_frame_grid(
    frame_paths: &[PathBuf],
    output_path: &Path,
    columns: u32,
) -> Option<PathBuf> {
    if frame_paths.is_empty() || columns == 0 {
        return None;
    }
    const CELL_W: u32 = 320;
    const CELL_H: u32 = 180;
    let rows = ((frame_paths.len() as u32) + columns - 1) / columns;
    let mut grid: RgbImage =
        ImageBuffer::from_pixel(columns * CELL_W, rows * CELL_H, Rgb([0, 0, 0]));
    for (index, path) in frame_paths.iter().enumerate() {
        let img = image::open(path).ok()?.to_rgb8();
        let resized =
            image::imageops::resize(&img, CELL_W, CELL_H, image::imageops::FilterType::Triangle);
        let col = (index as u32) % columns;
        let row = (index as u32) / columns;
        image::imageops::replace(
            &mut grid,
            &resized,
            (col * CELL_W).into(),
            (row * CELL_H).into(),
        );
    }
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    grid.save(output_path).ok()?;
    output_path.is_file().then(|| output_path.to_path_buf())
}

/// 按边界时刻生成内容候选窗（合并过短段，数量封顶）；无边界时退回整段。
pub(crate) fn phase4_content_windows(
    asset_id: &str,
    duration_ms: i64,
    cut_times_ms: &[i64],
) -> Vec<Phase4ContentWindow> {
    if duration_ms <= 0 {
        return Vec::new();
    }
    let mut boundaries = vec![0_i64];
    for &cut in cut_times_ms {
        if cut > 0 && cut < duration_ms {
            boundaries.push(cut);
        }
    }
    boundaries.push(duration_ms);
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut ranges = boundaries
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .filter(|(start, end)| end - start >= 400)
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        ranges.push((0, duration_ms));
    }

    // 合并过短邻段，避免碎窗刷屏。
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if end - start < 1_200 {
                last.1 = end;
                continue;
            }
        }
        merged.push((start, end));
    }
    while merged.len() > PHASE4_MAX_WINDOWS_PER_ASSET {
        // 合并当前最短邻对，保持时间顺序覆盖全片。
        let mut best_i = 0usize;
        let mut best_span = i64::MAX;
        for i in 0..merged.len().saturating_sub(1) {
            let span = merged[i + 1].1 - merged[i].0;
            if span < best_span {
                best_span = span;
                best_i = i;
            }
        }
        let right = merged.remove(best_i + 1);
        merged[best_i].1 = right.1;
    }

    // 导入关键帧常在 1s 处切出 [0,1s) 头窗；过短首窗并入后窗，避免整镜被钳进 1s。
    if merged.len() >= 2 && merged[0].1 - merged[0].0 < 1_200 {
        merged[1].0 = merged[0].0;
        merged.remove(0);
    }

    merged
        .into_iter()
        .enumerate()
        .map(|(index, (start_ms, end_ms))| Phase4ContentWindow {
            window_id: format!("{asset_id}:w{index}"),
            asset_id: asset_id.to_owned(),
            start_ms,
            end_ms,
        })
        .collect()
}

/// 在闭区间内均匀取 `count` 个检查时刻（略避开端点）。
pub(crate) fn densify_times_in_range(start_ms: i64, end_ms: i64, count: usize) -> Vec<i64> {
    if count == 0 || end_ms <= start_ms {
        return Vec::new();
    }
    if count == 1 {
        return vec![start_ms + (end_ms - start_ms) / 2];
    }
    let span = (end_ms - start_ms) as f64;
    let mut times = Vec::with_capacity(count);
    for index in 0..count {
        let ratio = (index as f64 + 0.5) / count as f64;
        let time = start_ms + (span * ratio).round() as i64;
        times.push(time.clamp(start_ms, end_ms.saturating_sub(1).max(start_ms)));
    }
    times.sort_unstable();
    times.dedup();
    times
}

/// 用导入期关键帧时间做粗候选窗；无关键帧时退回前/中/后三段。
/// **不做**每条素材的全片场景切点扫描（太慢且多数素材切点≈0）。
pub(crate) fn build_phase4_windows_from_keyframes(
    asset_id: &str,
    duration_ms: i64,
    keyframe_times_ms: &[i64],
) -> Vec<Phase4ContentWindow> {
    let duration_ms = duration_ms.max(1);
    let mut cuts: Vec<i64> = keyframe_times_ms
        .iter()
        .copied()
        .filter(|time| *time > 0 && *time < duration_ms)
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    if cuts.is_empty() && duration_ms > 3_000 {
        cuts = vec![duration_ms / 3, (duration_ms * 2) / 3];
    }
    phase4_content_windows(asset_id, duration_ms, &cuts)
}

/// 在指定时刻列表抽 JPEG；`label` 区分 passA/passB 缓存目录。
pub(crate) fn extract_frames_at_times(
    app: &AppHandle,
    asset_id: &str,
    source_path: &Path,
    times_ms: &[i64],
    label: &str,
) -> Vec<(i64, PathBuf)> {
    if times_ms.is_empty() || !source_path.is_file() {
        return Vec::new();
    }
    let Ok(app_data) = app.path().app_data_dir() else {
        return Vec::new();
    };
    let directory = app_data
        .join("derived")
        .join(asset_id)
        .join("phase4_inspect")
        .join(label);
    if std::fs::create_dir_all(&directory).is_err() {
        return Vec::new();
    }
    let mut frames = Vec::new();
    for (index, &time_ms) in times_ms.iter().enumerate() {
        let destination = directory.join(format!("frame_{:03}_{time_ms}.jpg", index + 1));
        if extract_jpeg_at_time(source_path, time_ms, &destination) {
            frames.push((time_ms, destination));
        } else {
            log::warn!(
                "Phase 4 frame extract failed: asset={} label={} t={}ms",
                asset_id,
                label,
                time_ms
            );
        }
    }
    frames
}

fn extract_jpeg_at_time(source_path: &Path, time_ms: i64, destination: &Path) -> bool {
    let time_seconds = (time_ms as f64 / 1000.0).max(0.0);
    let mut command = hidden_command("ffmpeg");
    command
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{time_seconds:.3}"),
            "-i",
        ])
        .arg(source_path)
        .args(["-frames:v", "1", "-vf", "scale=320:-2"])
        .arg(destination);
    matches!(
        run_hidden_command_with_timeout(&mut command, PHASE4_FRAME_FFMPEG_TIMEOUT),
        Ok(_) if destination.is_file()
    )
}

/// 为 top-N 候选构建多模态输入内容块。
#[allow(dead_code)] // 预留：统一多模态候选内容块；Phase 3/4 现走专用拼装
pub fn build_multimodal_content(
    candidates: &[StoryboardSource],
) -> Result<Vec<serde_json::Value>, String> {
    let mut blocks = Vec::new();

    for candidate in candidates {
        if let Some(grid_path) = &candidate.keyframe_grid_path {
            if let Some(image) = read_input_image(Path::new(grid_path)) {
                blocks.push(image);
            }
        }

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

        blocks.push(json!({
            "type": "input_text",
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

    #[test]
    fn content_windows_follow_cuts_in_order() {
        let windows = phase4_content_windows("a1", 20_000, &[5_000, 12_000]);
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].window_id, "a1:w0");
        assert_eq!(windows[0].start_ms, 0);
        assert_eq!(windows[0].end_ms, 5_000);
        assert_eq!(windows[2].start_ms, 12_000);
        assert_eq!(windows[2].end_ms, 20_000);
    }

    #[test]
    fn content_windows_merge_short_head_into_following() {
        // 导入关键帧常在 1s 切出 [0,1s)；应并入后窗，避免陷阱头窗。
        let windows = phase4_content_windows("a1", 30_000, &[1_000, 10_000]);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].start_ms, 0);
        assert_eq!(windows[0].end_ms, 10_000);
        assert_eq!(windows[1].start_ms, 10_000);
        assert_eq!(windows[1].end_ms, 30_000);
    }

    #[test]
    fn keyframe_windows_use_import_times_else_thirds() {
        let from_kf = build_phase4_windows_from_keyframes("a1", 30_000, &[8_000, 18_000]);
        assert_eq!(from_kf.len(), 3);
        assert_eq!(from_kf[1].start_ms, 8_000);
        assert_eq!(from_kf[1].end_ms, 18_000);

        let thirds = build_phase4_windows_from_keyframes("a2", 9_000, &[]);
        assert_eq!(thirds.len(), 3);
        assert_eq!(thirds[0].end_ms, 3_000);
        assert_eq!(thirds[2].start_ms, 6_000);
    }

    #[test]
    fn densify_times_stay_inside_range() {
        let times = densify_times_in_range(1_000, 5_000, 4);
        assert_eq!(times.len(), 4);
        assert!(times.iter().all(|time| (1_000..5_000).contains(time)));
    }

    #[test]
    fn compose_timed_frame_grid_keeps_all_cells() {
        use image::{Rgb, RgbImage};
        let directory = std::env::temp_dir().join(format!(
            "phase4-grid-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let mut paths = Vec::new();
        for index in 0..6 {
            let path = directory.join(format!("cell_{index}.jpg"));
            let mut img = RgbImage::new(40, 40);
            for pixel in img.pixels_mut() {
                *pixel = Rgb([index as u8 * 40, 20, 20]);
            }
            img.save(&path).unwrap();
            paths.push(path);
        }
        let out = directory.join("grid.jpg");
        let grid = compose_timed_frame_grid(&paths, &out, 3).expect("grid");
        let composed = image::open(&grid).unwrap();
        assert_eq!(composed.width(), 960);
        assert_eq!(composed.height(), 360);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
