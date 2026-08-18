# 任务清单

## 当前任务窗口

<!-- ACTIVE_TASKS_START -->
- [ ] 进行中（2026-08-18，feature）：多模态视觉选镜系统——关键帧网格图。
  1. **第一阶段（视觉分析存储层）**：`TechnicalMetadata` 新增 `keyframe_grid_path: Option<String>` 字段存储关键帧网格图路径；`models.rs` 的 `StoryboardSource` 同步新增该字段；向后兼容（旧记录读为 `None`）。
  2. **第二阶段（关键帧提取子模块）**：新建 `storyboard/keyframes.rs` 独立子模块。定义 `extract_keyframe_grid(asset_path, scene_segments, output_path) -> Result<PathBuf, String>` 接口，从场景段均匀采样 4-8 帧，拼成 2x2 或 2x4 网格图，保存到 `.cache/<asset_id>_grid.jpg`。当前阶段定义接口和结构，实现体保留 TODO 标记（后续集成 FFmpeg 截图逻辑）。
  3. **第三阶段（多模态 prompt 构建）**：`storyboard.rs::request_storyboard` 改造为多模态输入。为 top-5 候选的每个素材，如果 `keyframe_grid_path` 存在，读取图像并 base64 编码，构建 `{"type": "image"}` 内容块；prompt 指示模型直接从关键帧画面判断语义匹配度，而非依赖文本化的 `visual_evidence`。
  4. **第四阶段（验证层集成）**：`storyboard/validation.rs::verify_storyboard_selections` 同步使用关键帧网格图，让独立验证模型也能看到画面，对抗审查选镜合理性。
- [ ] 本项完成门：110+ 个 Rust 库测试、前端 lint/build、Rust fmt/check、harness:test/check 通过。新增字段向后兼容。公开 Tauri 命令、SQLite schema、Agent 工具白名单不变。第二、三、四阶段架构先定义接口，实现体分独立任务完成。变更记录见 `docs/changes/2026-08-18-multimodal-keyframe-grid-selection.md`。
<!-- ACTIVE_TASKS_END -->

- [x] 完成（2026-08-18，feature）：深度解耦地实现完整选镜优化系统（四阶段）。暴露质量分数、多样性硬门；提取独立评分模块（语义50分、质量25分、时长15分）；定义语义匹配层架构（embedding接口）；定义对抗验证框架架构。新增字段向后兼容，公开契约不变。变更记录见 `docs/changes/2026-08-18-storyboard-selection-scoring-system.md`。

- [x] 完成（2026-08-17，refactor）：将 `agentloop.rs`（3684 行）拆分为四个子模块。`agentloop/schema.rs`（纯类型与常量）、`agentloop/prompt.rs`（提示构建与历史加载）、`agentloop/skills.rs`（技能执行器与状态辅助）、`agentloop/runtime.rs`（路由决策与主循环）；父文件收缩为薄 re-export 层加测试。`check-agent-contracts.mjs` 同步扩展扫描 `runtime.rs`。公开命令名称、SQLite schema、工具白名单、Provider 接口均不变。
- [x] 本项完成门：103 个 Rust 库测试、前端 lint/build、Rust fmt/check、harness:test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-17-extract-agentloop-submodules.md`。
- [x] 完成（2026-08-17，refactor）：从 `assets.rs` 提取 `assets/library.rs` 子模块。将素材库查询、目录投影、collection/tag/metadata 管理、旧版路径兼容等 545+ 行从 `assets.rs`（4114 → 3569 行）提取为独立 `assets/library.rs` 子模块（813 行）。Tauri 命令注册路径从 `assets::*` 更新为 `assets::library::*`；公开命令名称、参数、返回值、SQLite schema、工具白名单均不变。修复提取过程中引入的四处函数实现差异（`legacy_asset_directories`、`asset_directory_nodes`、`asset_safe_directory`、`asset_public_folder_metadata`）。
- [x] 本项完成门：130 个 Rust 库测试、前端 lint/build、Rust fmt/check、harness:check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-17-extract-assets-library-submodule.md`。

- [x] 完成（2026-08-16，bugfix + 产品流程）：修复用户报告的三个系统性问题。
  1. **Provider 诚实失败（问题 3）**：删除 `provider.rs::ModelAccess::resolve()` 的静默降级逻辑，改为"自定义 API 配了就只用自定义，OAuth 配了就只用 OAuth，都没配就明确拒绝"。错误消息包含 Base URL、模型名、HTTP 状态码或具体网络错误，`agentloop.rs` 的 `model_unavailable_message` 透传原始错误。
  2. **render_preview 前置条件错误信息（问题 2）**：当 `render_preview` 因缺少 timeline 失败时，`safe_tool_failure_context` 返回明确的 `missing_timeline` 诊断，告知用户需要先创建时间线。
  3. **storyboard 确认后自动生成 timeline + preview（新需求）**：产品流程改为 `storyboard 生成 → needs_confirmation 等待用户确认 → 自动调用 create_timeline_draft + render_preview`。后端新增 `confirm_storyboard_and_preview` Tauri 命令，前端显示确认横幅，用户点击后链式执行两步；用户无需手动说"生成时间线"或"生成预览"。
  4. **创作透明度（问题 4）**：需要深度讨论后决定实现方式，暂记录为待决问题。
- [x] 本项完成门：130 个 Rust 库测试 + 2 个契约测试、前端 lint/build、Rust fmt/check、`harness:check` 与 diff 检查通过。变更记录见 `docs/changes/2026-08-16-fix-provider-honesty-and-preview-precondition.md`（问题 2/3）和 `docs/changes/2026-08-16-storyboard-review-then-auto-preview.md`（新需求）。

## 最近完成与历史执行记录

- [x] 完成（2026-08-16，refactor）：明确作用域架构并修复测试错误放置。在 `docs/architecture.md` 补充 ASCII 图和文字说明，明确会话只是对话容器、产物归属剪辑任务；将多版本回归测试从 `agentloop.rs` 迁回 `timeline.rs`（+27 行），删除 `agentloop.rs` 的 `#[rustfmt::skip]` 格式压缩并展开测试为标准格式（保持 3599 行）；在 `src-tauri/src/AGENTS.md` 新增"测试放置与预算"规则，明确单元测试必须放在被测模块、架构预算不得用格式压缩绕过。`timeline.rs` 预算 1848→1875 系迁回错误放置测试，非功能增长。公开命令、SQLite schema、工具白名单不变。
- [x] 本项完成门：130 个 Rust 库测试 + 2 个契约测试、Rust fmt/check、`harness:check`（commit 后 ratchet 以新基线通过）与 diff 检查通过。变更记录见 `docs/changes/2026-08-16-clarify-scope-architecture-and-decouple-tests.md`。
