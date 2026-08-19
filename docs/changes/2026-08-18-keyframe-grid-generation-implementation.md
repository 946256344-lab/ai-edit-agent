# 2026-08-18: 实现关键帧网格拼接与固定 4 帧采样策略

## 变更类型

feature（选镜优化系统 - 第一阶段基础设施）

## 触发规则

- desktop-contract（修改 `src-tauri/src/assets/analysis.rs`、`src-tauri/src/storyboard/multimodal.rs`）
- desktop-runtime（修改 `src-tauri/Cargo.toml`）

## 变更范围

### 依赖变更

**src-tauri/Cargo.toml**
- 新增 `image = { version = "0.25", default-features = false, features = ["jpeg"] }` 依赖

### 关键帧提取策略（src-tauri/src/assets/analysis.rs）

**常量定义**
- 修改 `KEYFRAME_COUNT: usize = 4`（从 6 改为 4）
- 删除 `SCENE_SCAN_CAP_SECONDS`、`SCENE_SCAN_FPS`、`MAX_INITIAL_SCENE_KEYFRAMES` 常量
- 删除 `SCENE_SCAN_FFMPEG_TIMEOUT` 常量

**`generate_video_keyframes` 函数重写**
- **旧策略**：场景检测（前 30 秒，最多 6 帧）
- **新策略**：固定时间采样（整个视频，精确 4 帧）
  - 采样点：第 1 秒、1/3 处、2/3 处、最后 1 秒（≥1.5s）
  - 短视频回退（≤2 秒）：开头和中间各一帧
- FFmpeg 使用 `-ss` 直接定位，`-frames:v 1` 提取单帧，`-vf scale=320:-2` 保持宽高比
- 输出：`keyframe_001.jpg` ~ `keyframe_004.jpg`，保存到 `<derived_dir>/<asset_id>/`
- 返回值：`Vec<KeyframeMetadata>` + 空 `Vec<SceneSegment>`（场景检测已移除）

**删除函数**
- `scene_scan_filter()`（场景检测滤镜构建，不再使用）

**`analyze_asset_with_permission` 集成**
- 在关键帧提取完成后调用 `generate_keyframe_grid`：
  ```rust
  if !metadata.keyframes.is_empty() {
      use crate::storyboard::multimodal::{generate_keyframe_grid, KeyframeGridConfig};
      let keyframe_paths: Vec<String> = metadata.keyframes
          .iter()
          .map(|kf| kf.image_path.clone())
          .collect();
      
      match generate_keyframe_grid(
          asset_id,
          &keyframe_paths,
          &derived_directory(app, asset_id)?,
          &KeyframeGridConfig::default(),
      ) {
          Ok(Some(grid_path)) => {
              metadata.keyframe_grid_path = Some(grid_path.to_string_lossy().into_owned());
          }
          Ok(None) => {
              log::warn!("Keyframe grid generation returned None for asset {asset_id}");
          }
          Err(error) => {
              log::warn!("Failed to generate keyframe grid for asset {asset_id}: {error}");
          }
      }
  }
  ```
- 网格生成失败时只记录警告，不阻塞素材导入流程

### 关键帧网格拼接（src-tauri/src/storyboard/multimodal.rs）

**`generate_keyframe_grid` 实现**
- 输入：4 个关键帧路径（`keyframe_001.jpg` ~ `keyframe_004.jpg`）
- 输出：2×2 网格图 `<asset_id>_grid.jpg`（640×360，单帧 320×180）
- 算法：
  1. 创建 640×360 黑色画布
  2. 逐帧加载、Lanczos3 缩放到 320×180
  3. 按行列索引粘贴：`(col * 320, row * 180)`
  4. JPEG 编码保存到 `<derived_dir>/<asset_id>_grid.jpg`
- 错误处理：图像打开失败或保存失败返回 `Err(String)`，关键帧不足返回 `Ok(None)`

**`build_multimodal_content` 实现**
- 为 top-N 候选构建多模态内容块（`Vec<serde_json::Value>`）
- 每个候选生成两个块：
  1. **image block**（如果有 `keyframe_grid_path`）：
     ```json
     {
       "type": "image",
       "source": {
         "type": "base64",
         "media_type": "image/jpeg",
         "data": "<base64_encoded_grid_image>"
       }
     }
     ```
  2. **text block**（元数据）：
     ```
     Asset ID: <asset_id>
     Duration: <duration_ms>ms
     Scene segments: <start_ms>ms-<end_ms>ms, ...
     ```
- Base64 编码使用 `base64::engine::general_purpose::STANDARD`
- 文件读取失败返回 `Err`，空候选列表返回空 `Vec`

### 向后兼容性

- `TechnicalMetadata.keyframe_grid_path: Option<String>`（已存在，2026-08-17 引入）
- 旧素材记录：`keyframe_grid_path` 读取为 `None`，不影响现有功能
- 新素材导入：自动生成网格图并记录路径
- 网格生成失败不阻塞素材导入，只记录警告日志

## 同步文档

- **docs/architecture.md**：更新素材分析流程，说明关键帧采样从场景检测改为固定时间采样
- **docs/api.md**：确认 `TechnicalMetadata.keyframe_grid_path` 字段已文档化（2026-08-17 已更新）
- **README.md**：无需更新（依赖变更不影响用户可见功能）
- **docs/decisions.md**：记录采样策略决策（固定 4 帧 vs 场景检测）及网格拼接方案
- **TASKS.md**：标记任务 #6 第二步完成（网格拼接实现）

## 公开契约

### Tauri 命令

无变更。

### SQLite schema

无变更（`TechnicalMetadata.keyframe_grid_path` 已在 2026-08-17 添加）。

### Agent 工具

无变更（`keyframe_grid_path` 字段通过现有 `list_assets_for_agent` 暴露）。

## 验证证据

- ✅ Rust 库测试：113 passed
- ✅ Rust fmt/check：通过
- ✅ 前端 lint/build：通过
- ✅ harness:test：通过
- ⏳ harness:check：需要同步 README.md 和 docs/decisions.md 后通过

## 测试覆盖

**单元测试（src-tauri/src/storyboard/multimodal.rs）**
- `default_config_uses_2x2_grid`：验证默认配置（4 帧 2×2 网格）
- `generate_keyframe_grid_returns_none_placeholder`：占位测试（TODO：补充真实网格拼接测试）
- `build_multimodal_content_returns_empty_placeholder`：占位测试（TODO：补充 base64 编码测试）

**TODO 测试（需补充）**
- 真实 4 帧图像拼接集成测试（创建临时 JPG → 调用 `generate_keyframe_grid` → 验证输出尺寸和内容）
- Base64 编码正确性测试（验证 image block 结构和 data 字段）

## 后续任务

1. **补充集成测试**：真实图像拼接和 base64 编码验证
2. **暴露质量分数**（任务 #6 第一步剩余部分）：在 `StoryboardSource` 和 `SceneSegment` 新增 `visual_quality_score` 字段
3. **调用多模态选镜**：在 `storyboard.rs` 的 `request_storyboard` 中调用 `build_multimodal_content`，将网格图和元数据传给模型

## 风险与限制

- **性能**：每个素材导入时额外生成一次网格图（4 帧加载 + 拼接 + JPEG 编码），预计增加 50-100ms
- **磁盘占用**：每个素材额外存储一张 640×360 JPEG（约 30-50KB）
- **失败静默**：网格生成失败只记录警告，不阻塞导入（用户不可见错误）
- **测试覆盖不足**：当前只有占位测试，未验证真实图像拼接逻辑
