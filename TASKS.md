# 任务清单

## 当前任务窗口

<!-- ACTIVE_TASKS_START -->
- [x] 完成（2026-08-20，feature/elevenlabs-voiceover）：「生成视频/配音」步骤耗尽修复。授权 storyboard+时间线+配音；有界 list_assets；空串当 null；Chat 工具消息合并；配音失败码。198 个库测试通过。见 `docs/changes/2026-08-20-elevenlabs-voiceover.md`。
- [x] 完成（2026-08-20，feature/elevenlabs-voiceover）：ElevenLabs 文案转配音。配音是时钟、字幕跟 alignment、失败封闭、密钥进 Credential Manager。191 个 Rust 库测试、lint/build、agent/harness 通过；真机合成未跑。见 `docs/changes/2026-08-20-elevenlabs-voiceover.md`。
- [x] 完成（2026-08-20，codex/cleanup-legacy-runtime）：工具成功后瞬时失败有界重试且不重放工具；单步超时按剩余次数拆分。桌面问素材数量已返回自然语言计数。见 `docs/changes/2026-08-20-native-provider-followup-recovery.md`。
- [x] 完成（2026-08-20，chore/native-provider-inspector）：debug + `NATIVE_PROVIDER_FULL_TRACE=1` 把 Native 每次 HTTP 的 INPUT/OUTPUT 写入 `src-tauri/target/native-provider-full-trace.jsonl`，不进前端、不写 SQLite。见 `docs/changes/2026-08-20-native-provider-full-trace.md`。
- [x] 完成（2026-08-20，fix/isolate-resolver-from-sibling-tasks）：Task Resolver 只看见当前激活任务；兄弟任务 title/brief/active_subgoal 不再进入路由模型。见 `docs/changes/2026-08-20-isolate-resolver-from-sibling-tasks.md`。
- [x] 完成（2026-08-20）：会话隔离 JOIN `editing_task_id` 失败封闭；Provider 诊断保留原始错误。负向测试收尾见独立分支。变更记录见 `docs/changes/2026-08-20-fix-session-isolation-message-history.md`。
- [x] 完成（2026-08-19，remove-fixed-loop-goal）：移除固定 LoopGoal；原生 function_call 继续、自然语言结束，RunReceipt 裁决终态。见 `docs/changes/2026-08-19-remove-fixed-loop-goal.md`。
- [x] 完成（2026-08-19，remove-conversation-router）：删除前置对话 Router；普通聊天与工具执行统一进 NativeToolLoop。见 `docs/changes/2026-08-19-remove-conversation-router.md`。
<!-- ACTIVE_TASKS_END -->

- [x] 完成（2026-08-19，native-observation-tools）：迁移剩余只读观察工具到 Native Function Tool 目录。见 `docs/changes/2026-08-19-native-observation-tools.md`。

- [x] 完成（2026-08-19，native-preview）：Native 安全接入 render_preview。见 `docs/changes/2026-08-19-native-render-preview.md`。

- [x] 完成（2026-08-19，native-memory）：NativeToolLoop 从 SQLite 按时间读取真实 user/assistant 消息，以原生 function_call/function_call_output 维持观察上下文；保留 Legacy 默认路径。133 个 Rust 库测试、前端 lint/build、Python unittest、agent/harness 检查和 diff 检查通过；变更记录见 `docs/changes/2026-08-19-native-session-messages.md`。
- [x] 完成（2026-08-19，native-loop）：在显式 `NativeToolLoop` 开关下接入只读原生 Agent Loop；仅允许 `get_asset_health_summary`、`list_assets`、`get_timeline`，保留 Legacy 默认路径、最大步骤数、总超时和取消边界；使用固定 fixture 覆盖普通回答、项目事实观察、get_timeline 和安全工具错误恢复。128 个 Rust 库测试 + 2 个契约测试、前端 lint/build、Python unittest、agent/harness 检查和 diff 检查通过；变更记录见 `docs/changes/2026-08-19-native-readonly-agent-loop.md`。

- [x] 完成（2026-08-19，native-delivery-tools）：将 `get_text_capabilities`、`replace_text_tracks`、`search_music`、`download_music`、`use_online_music`、`replace_music_tracks`、`render_preview`、`create_jianying_draft` 迁移到 Native Function Tool 目录与受限执行入口；strict 嵌套 Schema、参数边界、工具选择及执行前授权测试通过，保留许可证、下载、文字能力矩阵、剪映兼容性、确认边界和领域算法，不迁移其他工具或改变领域实现。变更记录见 `docs/changes/2026-08-19-native-delivery-tools.md`。

- [x] 完成（2026-08-19，native-main-chain-tools）：将 `request_asset_analysis`、`generate_storyboard`、`create_timeline_draft`、`replace_clips`、`change_clip_duration`、`reorder_clips` 迁移到 Native Function Tool 目录；保留素材证据、源时间范围、作用域、版本与事务校验，并新增“分析素材 → storyboard → timeline”固定 fixture 复合测试。提交 `05cefdb`；真实 Provider 在工具调用后偶发未生成最终回复，确认门桌面验收仍待后续可靠性任务处理。


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
