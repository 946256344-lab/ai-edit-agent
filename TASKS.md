# 任务清单

## 当前任务窗口

<!-- ACTIVE_TASKS_START -->
- [x] 完成（2026-08-19，provider）：改造 Provider 数据结构以统一解析 Responses 与 Chat Completions 的 message/function_call/tool call，并透传原生 tools/tool_choice/parallel_tool_calls；保留旧 JSON decision 接口，不接入 Agent Runtime。固定 JSON fixture、完整 Rust/前端/契约验证通过；变更记录见 `docs/changes/2026-08-19-provider-native-tool-turns.md`。
- [x] 完成（2026-08-19，performance）：优化 recover_missing_agent_completion_messages 查询性能。性能诊断日志显示该函数耗时 ~300ms，占启动总时间（~380ms）的 80%，是启动卡顿的根本原因。原查询使用相关子查询对每行外部结果重新执行一次子查询，导致 O(N²) 复杂度。用窗口函数（ROW_NUMBER() OVER PARTITION BY）+ CTE 替代相关子查询，一次性标记每个 conversation 的最新任务，避免重复扫描。查询语义完全等价（最新任务判定、NOT EXISTS 判定逻辑保持不变），预期耗时从 ~300ms 降至 <20ms。不影响公开命令或 SQLite schema。实际性能验证需要用户重启应用，观察优化后的 [PERF] 日志。变更记录见 `docs/changes/2026-08-19-optimize-recovery-query-performance.md`。
- [x] 完成（2026-08-19，feature）：添加启动性能诊断日志。用户报告应用启动时严重卡顿（打开程序特别卡、执行任务开始跑 agent 更是卡的不得了、agent 跑起来后倒是没那么卡）。为诊断根本原因，在启动流程关键步骤添加 [PERF] 前缀的性能日志：`initialize_local_store`（10 处，测量数据库连接、清理 agent_tasks/agent_run_steps/conversations、恢复缺失完成消息、标记会话状态）、`resume_incomplete_analysis`（7 处，测量 recover_interrupted_visual_batches、backfill_queued_visual_batches、spawn_visual_analysis_worker、收集已有任务素材 ID、查询孤立素材、创建孤立任务）、`recover_interrupted_visual_batches`（3 处）、`backfill_queued_visual_batches`（6 处）。所有日志使用 `log::info!` 级别，使用 `std::time::Instant` 测量耗时。只添加日志，不改变执行逻辑、公开命令或 SQLite schema。变更记录见 `docs/changes/2026-08-19-add-startup-performance-logging.md`。
- [x] 完成（2026-08-18，bugfix）：在 Phase 3 prompt 中明确 matchLevel 枚举值。Phase 3 是独立模型调用，原 prompt 只说 "Each shot must contain: ... matchLevel" 未列举合法值，可能导致模型返回其他字符串（如 `"high"`、`"medium"`）引发验证失败。补充 "matchLevel must be 'direct' (evidence visibly supports the beat) or 'contextual' (honest scene-setting)"，与 Phase 2 prompt 保持一致。不改变 `StoryboardContent` schema、公开命令或工具白名单。
- [x] 完成（2026-08-18，bugfix）：在路由决策 prompt 中明确列举 goal 枚举值。修复模型漏填 `goal` 字段或返回不合法值（如 `"storyboard_generation"` 而非 `"storyboard"`）导致的路由验证失败。Prompt 从模糊描述（"Include goal"）改为明确列举 5 个合法值（question, storyboard, timeline, preview, jianying）+ 对应推荐工具，降低模型猜测错误的概率。不改变 `ConversationRouteResponse` schema、公开命令或工具白名单。
- [x] 本项完成门：113 个 Rust 库测试 + 2 个契约测试、前端 lint/build、Rust fmt/check、harness:test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-18-clarify-route-goal-enum-in-prompt.md`。
- [x] 完成（2026-08-18，feature）：添加路由决策诊断日志。在 `agentloop/runtime.rs` 的路由决策流程新增三处 info/warn 级别日志：首次路由决策时记录模型返回的原始 route/goal/isQuestion/tool 值和 backend 识别的 pinnedGoal；纠偏重试后记录修正值；验证失败时记录导致失败的原始字段值。用于诊断 storyboard 生成失败时的路由验证问题（模型漏填 goal、返回不合法值、还是 fast_goal 关键词识别遗漏）。不改变执行逻辑或公开命令。
- [x] 本项完成门：113 个 Rust 库测试、前端 lint/build、Rust fmt/check、harness:test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-18-add-route-decision-logging.md`。
<!-- ACTIVE_TASKS_END -->

- [x] 本项完成门：113 个 Rust 库测试 + 2 个契约测试、前端 lint/build、Rust fmt/check、harness:test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-18-clarify-phase3-matchlevel-enum.md`。

- [x] 完成（2026-08-18，refactor）：重构 storyboard 生成为三阶段架构。原有实现对整个素材池（426 个视频）做一次全局排序，取 TOP-5 候选，导致整条时间线只能从同一组 5 个素材中反复选择。新架构分为三个阶段：**Phase 1（叙事结构生成）** - 模型根据 brief 和内容的自然节奏、节奏要求和叙事复杂度拆分为合适数量的 beats（简单消息可能 3-4 个，故事驱动内容可能 8-12 个或更多，由内容引导而非人为限制），每个 beat 包含 id/purpose/requiredVisual，不涉及素材选择；**Phase 2（逐 beat 粗选镜）** - 对每个 beat 单独对素材池排序（使用 `scoring::rank_segment_candidates`），提供该 beat 专属的 TOP-5 候选素材（带关键帧网格），模型为该 beat 选择 1 个素材 + 时间范围；**Phase 3（精剪与节奏优化）** - 模型调整精确时间范围（对齐场景边界、避免重叠）、节奏控制、镜头组合和过渡优化，输出最终可执行的 `StoryboardContent`。重试循环只在 Phase 3，验证失败时带反馈重新精剪，最多 3 次。新增 `src-tauri/src/storyboard/phases.rs` 子模块（231 行）定义三阶段函数和中间数据结构（`NarrativeStructure`、`RoughStoryboard`）；主流程 `storyboard.rs:generate_storyboard_internal` 重构为三阶段顺序调用（Phase 1/2 不重试，Phase 3 重试）。架构优势：素材多样性提升（每个 beat 独立 TOP-5，不再受全局 5 个素材限制）、语义匹配精度提升（排序针对每个 beat 的 `requiredVisual` 计算）、重试效率提升（Phase 3 验证失败时只重新精剪，Phase 1/2 结果保持稳定）。Tauri 命令签名不变，SQLite schema 不变，最终持久化的 `StoryboardVersion` 结构不变。
- [x] 本项完成门：113 个 Rust 库测试、前端 lint/build、Rust fmt/check、harness:test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-18-three-phase-storyboard-refactor.md`。

- [x] 完成（2026-08-18，bugfix）：修复素材重链接时 `kind` 字段未更新导致的数据不一致问题。根本原因：`confirm_asset_relink` 在更新 `source_reference` 时未同步更新 `kind` 字段，导致用户将图片替换为同名视频后，数据库仍记录旧的 `kind = 'image'`；分析结果回写路径 `update_analysis_status` 也存在同样问题。修复：在 `confirm_asset_relink` 的两个 UPDATE 分支（`preserve_analysis = true/false`）都重新计算 `kind = asset_kind(&source)` 并更新到数据库；在 `update_analysis_status` 的两个分支（有/无 `metadata_json`）也同步更新 `kind` 字段。修复后用户 relink 到不同类型文件或分析回写时，`kind` 会自动同步，避免"数据库显示 451 个图片但文件系统实际是 426 个视频"的不一致。
- [x] 本项完成门：113 个 Rust 库测试、前端 lint/build、Rust fmt/check、harness:test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-18-fix-asset-kind-sync-on-relink.md`。

- [x] 完成（2026-08-18，feature）：为 storyboard 生成流程添加详细日志并增强素材池诊断。在 `storyboard.rs` 的关键决策点添加 info/error 级别日志：入口参数（project_id、editing_task_id、brief 长度）、素材库存统计（总数、视觉就绪数、视频/图片/音频/其他计数）、素材样本（前 10 个的 ID/类型/时长）、候选排序与 TOP-5 清单、多模态内容构建、模型请求/响应、重试循环进度、候选接收（shots/beats/时长）、归一化修正（视频范围修正、脚本模式降级）、验证结果及最终失败总结。覆盖 `generate_storyboard_internal`（7 处）、`request_storyboard`（4 处）和 `normalize_storyboard_candidate`（4 处），共 15 处日志点，支持后续调试验证失败原因和候选素材选择过程。新增素材样本日志可快速识别素材池中视频/图片的实际比例，用于诊断"451 个图片 vs 4 个视频"等异常情况。
- [x] 本项完成门：113 个 Rust 库测试、前端 lint/build、Rust fmt/check、harness:test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-18-add-storyboard-generation-logging.md`。

- [x] 完成（2026-08-18，feature）：实现关键帧网格拼接与固定 4 帧采样策略。替换场景检测（前 30 秒，最多 6 帧）为固定时间采样（整个视频，精确 4 帧：第 1 秒、1/3、2/3、最后 1 秒）。实现 `storyboard/multimodal.rs` 的 `generate_keyframe_grid`（2×2 网格拼接，640×360 JPEG）和 `build_multimodal_content`（base64 编码为 image block + 元数据 text block）。素材导入后自动调用网格生成，路径记录到 `TechnicalMetadata.keyframe_grid_path`。向后兼容：旧素材读取为 `None`，网格生成失败只记录警告不阻塞导入。新增 `image = 0.25` 依赖（仅 jpeg feature）。
- [x] 本项完成门：113 个 Rust 库测试、前端 lint/build、Rust fmt/check、harness:test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-18-keyframe-grid-generation-implementation.md`，架构决策见 `docs/decisions.md` ADR-065。
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
