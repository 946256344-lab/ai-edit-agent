# Storyboard 多模态选镜架构

日期：2026-08-18

## 变更

### 多模态选镜架构层

- 新建 `storyboard/multimodal.rs` 子模块（137 行）。
- `KeyframeGridConfig` 定义关键帧网格配置：提取 4-8 帧，拼成 2x2 或 2x4 网格，单帧缩略图 320x180 像素。
- `generate_keyframe_grid` 定义关键帧提取与拼图接口：使用 FFmpeg 提取 I 帧，均匀采样，使用 image crate 拼接成网格图，保存为 JPEG（压缩质量 85）。
- `build_multimodal_content` 定义多模态内容块构建接口：遍历候选，读取关键帧网格图，base64 编码为 image block，构建元数据 text block。
- 当前阶段只定义接口和类型，实现体保留 TODO 标记供后续集成 FFmpeg 和 image crate。

### 选镜流程集成

- `storyboard.rs::request_storyboard` 检测候选是否包含 `keyframe_grid_path`。
- 多模态模式下：模型看到关键帧网格图 + 元数据（assetId、duration、sceneSegments），直接从画面判断语义匹配度。
- 纯文本模式下：回退到原有的 JSON 证据描述。
- 调用 `multimodal::build_multimodal_content` 构建多模态内容块，扩展到 `content_parts`。

### 提示词优化

- 多模态模式提示词明确告知模型："每个候选包含关键帧网格图，直接从画面判断语义匹配，不要仅依赖文本描述"。
- 强调候选已预排序（质量、时长、相关性），优先选择 top 候选。

## 边界

- **向后兼容**：`keyframe_grid_path` 字段已在上一阶段添加（`Option` 类型），旧记录安全读取为 `None`。
- **公开契约不变**：Tauri 命令、SQLite schema、Agent 工具白名单均未改变。
- **架构解耦**：多模态模块独立于评分、语义、验证模块，可单独演进和测试。
- **实现分阶**：当前阶段只定义接口和类型，FFmpeg 调用和 base64 编码实现在独立任务中完成。

## 验证

- `cargo test --lib`：114 个测试通过
- `cargo fmt --check`：格式检查通过
- `npm run harness:check`：架构与文档同步检查通过

同步文档：`docs/architecture.md`、`docs/api.md`。
