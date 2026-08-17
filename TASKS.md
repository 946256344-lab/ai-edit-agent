# 任务清单

## 当前任务窗口

<!-- ACTIVE_TASKS_START -->
（暂无活动任务）
<!-- ACTIVE_TASKS_END -->

- [x] 完成（2026-08-17，refactor）：将 `confirm_storyboard_and_preview` 从 `agent.rs` 提取为独立 `confirmation.rs` 模块，并将前端确认逻辑从 `App.tsx` 迁入 `useArtifactWorkspaceController`。`agent.rs` 从 1505 行降至 1246 行；`confirmation.rs` 独立承载 storyboard 确认链路（resolve pending clarification → create agent task → timeline + preview）；前端 `artifactWorkspace.actions.confirmStoryboard()` 成为唯一确认入口，`App.tsx` 只做异常处理和加载状态管理。三个 helper 函数（`failed_agent_edit_result`、`persist_agent_completion_message`、`persisted_task_status`）改为 `pub(crate)` 供 `confirmation.rs` 复用；公开 Tauri 命令、SQLite schema、工具白名单不变。
- [x] 本项完成门：130 个 Rust 库测试 + 2 个契约测试、前端 lint/build、Rust fmt/check、diff 检查通过。`agent.rs` 预算调整至 1247 行，`policy.rs` 预算调整至 688 行。

- [x] 完成（2026-08-16，bugfix + 产品流程）：修复用户报告的三个系统性问题。
  1. **Provider 诚实失败（问题 3）**：删除 `provider.rs::ModelAccess::resolve()` 的静默降级逻辑，改为"自定义 API 配了就只用自定义，OAuth 配了就只用 OAuth，都没配就明确拒绝"。错误消息包含 Base URL、模型名、HTTP 状态码或具体网络错误，`agentloop.rs` 的 `model_unavailable_message` 透传原始错误。
  2. **render_preview 前置条件错误信息（问题 2）**：当 `render_preview` 因缺少 timeline 失败时，`safe_tool_failure_context` 返回明确的 `missing_timeline` 诊断，告知用户需要先创建时间线。
  3. **storyboard 确认后自动生成 timeline + preview（新需求）**：产品流程改为 `storyboard 生成 → needs_confirmation 等待用户确认 → 自动调用 create_timeline_draft + render_preview`。后端新增 `confirm_storyboard_and_preview` Tauri 命令，前端显示确认横幅，用户点击后链式执行两步；用户无需手动说"生成时间线"或"生成预览"。
  4. **创作透明度（问题 4）**：需要深度讨论后决定实现方式，暂记录为待决问题。
- [x] 本项完成门：130 个 Rust 库测试 + 2 个契约测试、前端 lint/build、Rust fmt/check、`harness:check` 与 diff 检查通过。变更记录见 `docs/changes/2026-08-16-fix-provider-honesty-and-preview-precondition.md`（问题 2/3）和 `docs/changes/2026-08-16-storyboard-review-then-auto-preview.md`（新需求）。

- [x] 完成（2026-08-16，refactor）：明确作用域架构并修复测试错误放置。在 `docs/architecture.md` 补充 ASCII 图和文字说明，明确会话只是对话容器、产物归属剪辑任务；将多版本回归测试从 `agentloop.rs` 迁回 `timeline.rs`（+27 行），删除 `agentloop.rs` 的 `#[rustfmt::skip]` 格式压缩并展开测试为标准格式（保持 3599 行）；在 `src-tauri/src/AGENTS.md` 新增"测试放置与预算"规则，明确单元测试必须放在被测模块、架构预算不得用格式压缩绕过。`timeline.rs` 预算 1848→1875 系迁回错误放置测试，非功能增长。公开命令、SQLite schema、工具白名单不变。
- [x] 本项完成门：130 个 Rust 库测试 + 2 个契约测试、Rust fmt/check、`harness:check`（commit 后 ratchet 以新基线通过）与 diff 检查通过。变更记录见 `docs/changes/2026-08-16-clarify-scope-architecture-and-decouple-tests.md`。

- [x] 完成（2026-08-16，bugfix）：修复 `timeline.rs::select_timeline_candidate` 在多版本时间线场景下返回 `None` 导致"生成预览"始终失败的问题。旧逻辑 `(timelines.len() == 1).then(...)` 仅单条候选时返回结果；改为 `timelines.first().cloned()` 取列表首条（`version_number DESC` 排序下即最新版）。精确 ID 匹配路径不变。公开命令、SQLite schema、工具白名单不变。变更记录见 `docs/changes/2026-08-16-fix-render-preview-multiple-timelines.md`。

- [x] 完成（2026-08-16，bugfix）：修复 `storyboard.rs::generate_storyboard_internal` 重试循环中校验无法收敛的问题。`normalize_storyboard_candidate` 从校验通过后移至校验前执行，自动夹紧 `duration_ms` 到源时间段宽度并修正 `target_duration_ms`；三次重试配额只用于结构性校验失败（非法 beat ID、资产不可用等），不再被纯数值偏差消耗。修复后新剪辑会话第一条 `generate_storyboard` 请求不再以 `invalid_source_time_range` 失败。公开命令、SQLite schema、工具白名单不变。
- [x] 本项完成门：129 个 Rust 库测试、Rust fmt/check、harness test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-16-storyboard-normalize-before-validate.md`。

## 最近完成与历史执行记录

- [x] 完成（2026-08-16，refactor）：移除后端 Rust 所有静默 fallback，补全被丢弃的错误日志。`agent.rs::persisted_task_status` 拆分 DB 不可用与任务失败两个 Err 路径并各自输出真实原因；`agentloop.rs` 的 `get_storyboard` handler 与 `build_timeline_snapshot` 序列化失败时记录真实错误而非静默返回 `Value::Null`；`assets.rs` 的时间戳读取失败（A9）、metadata_json 解析失败（A10）、Provider 访问失败（B15）、视觉模型请求失败（visual_req）均改为输出真实 `{error}` 变量；`spawn_visual_analysis_worker` IIFE 的 `Err` 路径改用 `.inspect_err()` 记录（B19）。所有降级仍封闭失败，不伪造成功结果。公开命令、SQLite schema、工具白名单、用户数据均未改变。
- [x] 本项完成门：129 个 Rust 库测试 + 2 个契约测试、前端 lint/build、Rust fmt/check、架构预算（所有文件在预算内）、harness test/check 与 diff 检查通过。变更记录见 `docs/changes/2026-08-16-remove-silent-fallbacks.md`，ADR-065 记录决策。

- [x] 完成（2026-08-15，robustness）：在 `agentloop.rs::decide_conversation_route` 和 `taskrouter.rs::resolve_conversation_task` 实现 validate-then-correct 模式。首次模型响应验证失败时，将错误原因作为纠偏提示反馈给模型并重试一次，而非直接 fail-closed；`try_build_route_decision` 封装路由决策构建与验证，失败原因字符串即作为纠偏提示。同步将 `fast_goal` 降级为纯提示、将 `AGENT_RUN_TIMEOUT` 从 90 s 提升至 300 s、从 `EDIT_VERBS` 移除"剪辑"避免过度触发，并对两个文件应用 `#[rustfmt::skip]` 保持紧凑布局通过架构预算。不修改公开命令、SQLite schema、工具白名单或用户数据。
- [x] 本项完成门：129 个 Rust 库测试 + 2 个契约测试、前端 lint/build、Rust fmt/check、harness test/check 与 diff 检查通过；架构预算 `agentloop.rs`（3598 行 / 148495 字符）与 `taskrouter.rs` 均在预算内。

- [x] 完成（2026-08-15，bugfix）：修复 `preview.rs::render_timeline_clip` 的 FFmpeg `-t` 参数未收敛到源范围的问题。`-t` 改为 `min(source_end_ms - source_start_ms, timeline_end_ms - timeline_start_ms)`；将测试模块提取为独立 `preview_tests.rs`（`#[path]` 挂载），`preview.rs` 行数从 1094 降回 608；新增 `render_timeline_clip_clamps_duration_to_source_range` 回归测试。不修改公开命令、schema 或现有 preview 文件。
- [x] 完成（2026-08-15，只读审计）：对 timeline v6 进行只读媒体事实审计。全部 8 个 asset `ready`/`online`/未 excluded；v5→v6 变更仅 shot2 缩短 500 ms，与 TASKS.md 记录一致；时间线 0–31,689 ms 连续无间隙；所有 source_start ≥ 0，source_end ≤ asset_dur。发现系统性问题：`preview.rs::render_timeline_clip` 未将 `source_end_ms` 传给 FFmpeg，shot1/shot3 实际可用素材短于 timeline slot，shots 4–8 的 source_end 约束静默失效。完整证据见 `docs/audits/2026-08-15-timeline-v6-media-fact-audit.md`。
- [x] 本项完成门：只读查询 SQLite，未修改任何数据库记录；未重新分析素材、未修改 timeline、未生成 preview、未创建 Jianying draft、未导出。
- [x] 完成（2026-08-15，工程流程）：建立 Cursor、Codex、Claude Code、OpenCode 共用的协作标准。`CONTRIBUTING.md` 是分支、worktree、验证、提交、PR 与合并唯一事实源，工具入口只引用规则且不分配固定职责；pre-commit 新增受保护分支、允许前缀和本地 `origin/master` 祖先硬门，PR 模板要求边界与验证证据。未改变产品运行时、用户数据或 master。
- [x] 本项完成门：分支策略正负测试、Agent 入口引用与 ratchet、architecture/doc-sync harness、前端 lint/build 和 diff 检查通过；远端 GitHub master 保护与 Windows CI 仍为明确 TODO，本地 hook 可被 `--no-verify` 绕过。
- [x] 完成（2026-08-15，P0）：落地后端热点第一条物理边界。`agentloop/policy.rs` 独占工具白名单、负向约束、目标解析、真实产物完成门和固定诚实降级文案，只依赖 `AgentEditResult`；父 `agentloop.rs` 保留 Router、状态、prompt、有界循环和技能执行，从 4264 行降至 3599 行。公开命令、SQLite schema、工具名、最大步数、Provider、媒体处理和用户数据均未改变。
- [x] 本项完成门：全部手写 Rust、TypeScript/React、Node、Python、HTML、Shell 与 CSS 源码已有中文职责导航，关键权限/事务/恢复/算法补就地说明；`agent:check` 与 pre-commit 新增强制导航和只收紧 ratchet。前端 lint/build、Rust fmt/check、128 个单元测试 + 2 个契约测试、14 个 Python 测试、agent/harness/diff 与独立审查通过；仅保留既有 `PartiallyDone` dead-code warning。下一步按路线提取 `assets/library.rs`。
- [x] 完成（2026-08-15，P0）：把被动 Markdown 约束工程化。根、`src/`、`src-tauri/src/` 分层 Agent 指令与有界当前任务窗口负责按需加载；机器清单和 pre-commit 自动阻止代码地图增生、JS/TS 绕过 Tauri bridge、动态/间接/未注册 IPC、API 文档漂移、外部进程/凭据/网络所有权扩散、Agent 工具目录漂移，以及配置相对 `HEAD` 放宽。检查器/配置部分暂存会失败；本地 hook 仍不是不可绕过的安全沙箱，Windows CI 留作后续。
- [x] 本项完成门：`agent:test/check`、`harness:test/check/staged`、提交 hook、前端 lint/build、Rust fmt/check、128 个单元测试 + 2 个契约测试、14 个 Python 测试、七份代码地图 exact-file/evidence、diff 与独立审查均通过。审查发现并关闭嵌套文件、动态 import、bridge alias、裸命令注册、grouped Rust import、清单 ratchet、staged/worktree 和入口措辞问题；没有改变产品运行行为、SQLite schema 或用户数据。Rust 仅保留已记录的 `PartiallyDone` 既存 warning。
- [x] 完成（2026-08-15，P0）：建立全仓库代码地图与 IDE 导航注释。以真实源码为依据梳理 React/Tauri 启动、conversation/task 路由、Agent loop、素材分析、storyboard、timeline、preview、Jianying、Provider、SQLite 和测试边界；`docs/codebase/` 固定为七份学习文档，前后端关键入口补充只解释职责、调用方向和副作用边界的导航型注释。同步清除 `agent-tools.ts` 的历史接口草图，明确 9 个观察技能、12 个编辑/交付技能、fixture canonical controls 与 production alias 的真实关系；未改变公开命令、持久化 schema、业务行为或用户数据。
- [x] 本项完成门：七份代码库文档通过 exact-file/inquiry/evidence 检查；代码注释与实际调用链一致、不复述语法、不掩盖巨型模块债务。前端 lint/build、Rust fmt/check、128 个单元测试 + 2 个契约测试、14 个 Python adapter 测试、harness test/check 与 diff 检查通过；独立审查三轮发现并关闭 control alias、状态工具、可信边界、Task Resolver 澄清/receipt 时序、SQLite 与术语精度问题。后端巨型模块只形成有边界、有顺序的拆分路线，不在缺少完整 scripted Agent runner 时盲拆；Rust 仍有既存 `PartiallyDone` 未构造 warning，已记录为债务。
- [x] 完成（2026-08-15，P0）：收敛前端编排边界并建立可执行的架构约束。`App.tsx` 从约 1110 行降至 515 行，只保留项目、剪辑任务、conversation、消息路由和工作区组合；Provider、素材、成果交付、Agent task 终态对账分别进入具名 controller。删除 296 行且混合 Agent/成果模式的 `ConversationWorkspace`，改为互斥工作区与独立侧栏、顶栏、Provider、分析提示组件；现有项目、会话、素材、storyboard、timeline、preview、Jianying 与 Agent 任务契约不变。
- [x] 本项完成门：Provider 与 Agent task 对账内部状态不再由 `App.tsx` 持有；Agent/成果工作区各自只有 `model/actions` 两个领域入口。机器可读架构预算已接入 `harness:check`、`harness:staged` 与 pre-commit，以只降不升的行数/字符/最长单行、props/state/effect/async、禁止路径和跨层调用约束阻止反向膨胀；语法无法解析、rest props、删除/改名预算或不完整迁移均 fail-closed。前端 lint/build、harness test/check、diff 检查、真实 Tauri 项目恢复/互斥模式/素材目录开合/Provider 模态回归均通过；独立审查三轮发现的 props、async/压缩代码、预算删除与迁移绕过均已补回归并关闭。
- [x] 完成（2026-08-15，P0）：重建最小、Agent-first 的素材工作区。保留后端目录投影、素材分页、分析证据、源文件健康检查与显式重链路，不删除素材、分析结果或任何后端能力；前端删除搜索/组合筛选、收藏/评分/标签/集合、批量操作和任务中心，只保留导入、可开合目录树、当前目录直属素材、证据 Inspector 与异常恢复。`AssetManagementPanel` 从约 500 行、50 个扁平 props 收敛为 116 行和 `model/actions` 两个领域入口，`App.tsx` 素材 state 从约 20 个减至 8 个必要状态。
- [x] 本项完成门：安全导入根首次进入自动展开、子目录默认折叠，局部 `expandedFolderIds` 只保存真实开合状态，`toggleAssetFolder` 单一动作驱动条件渲染，`aria-expanded` 与画面一致。真实 Tauri 验证 891 条素材、根目录展开、子目录折叠/展开、跨两次 1.5 秒轮询保持状态；当前 106 条目录折叠后仍保留已加载的 100/106 条，切换目录立即清空旧卡片并落到 72/72 条直属素材；证据 Inspector 成功显示真实关键帧/OCR，控制台无错误。前端 lint/build、harness 与 diff 检查通过；harness 未触发独立审查规则，因为公开桌面命令、Rust、`local-store.ts` 和 Agent 契约均未变化。
- [x] 完成（2026-08-15，P0）：恢复按文件夹导入时的本地相对文件树。当前真实项目 891 条素材的旧 `folder_reference` 均指向没有可展示 `file_name` 的 UNC share 根，但全部源引用已通过安全卷分组和相对结构恢复为 1 个安全导入根、10 个一级子目录和 2 个二级子目录，无需重新导入或重新分析。修复严格目录投影把 891 条全部归为“未归类”、`list_asset_page` 只返回根文件夹名、前端又以完整目录键二次过滤造成子目录空列表和错误计数的问题；目录契约现显式区分安全目录键、相对文件路径和直属素材计数，每级只显示直属子文件夹与直属素材，不暴露 server/share、盘符或绝对路径。
- [x] 本项完成门：128 个 Rust 单元测试、2 个契约测试、前端 lint/build、Rust fmt、harness 与独立审查通过；真实 Tauri 素材页显示 13 个目录节点（1 个安全导入根、10 个一级目录、2 个二级目录），`unfiledCount=0`。进入一个子目录时后端和界面均显示 72 条直属素材；进入直属素材为 0 的父目录时仍显示 4 个子文件夹且不残留旧卡片；切换目录时会先清空旧页。891 条素材、分析证据、timeline 与 preview 均未改变。下一阶段再单独决定哪些素材管理控件删除。
> 2026-08-14 及更早的条目已归档至 [`docs/changes/TASK_HISTORY.md`](docs/changes/TASK_HISTORY.md)。

### P0：端到端 MVP 重新验收

- [ ] 在恢复后的 Tauri 桌面应用中重新验证完整链路：实验性 Provider 登录、真实媒体分析、证据绑定 storyboard、内部时间线和可播放 preview。
- [ ] 重新验证启动时恢复的媒体分析会显示在右下角任务提示，且 FFmpeg、FFprobe、Tesseract 与 Python 子进程不显示命令行窗口。
- [ ] 重新验证实验性 OAuth 的登录、重启后凭据可用性、刷新令牌和模型访问；不得将其表述为官方 OpenAI 第三方 OAuth。
- [ ] 用真实导入媒体验证实验性视觉分析和 storyboard Provider 响应兼容性。

历史验收记录（2026-08-05）：当时版本曾通过真实媒体、实验性 OAuth、视觉分析、证据绑定 storyboard、内部时间线、preview、任务恢复提示与无窗口子进程的桌面验收。8 月 5 日之后的 Agent、路由、素材、timeline 和 UI 高风险变更已经使该证据失效，当前版本必须重新验收。

### P0：Agent 审计与持久化补齐

- [x] 完成（2026-08-12）：修复代码审查发现的 Rust 1.77.2 兼容性、活动任务被最近 12 条候选截断、Jianying draft 失败后遗留未跟踪目录，以及 Provider 凭据模块未纳入文档同步 harness 的问题。新增活动旧任务候选、draft 创建/轨道失败回滚、路径约束和凭据模块文档门回归测试；验证：97 个 Rust 单元测试、2 个契约测试、14 个 Python 测试、前端 lint/build、Rust fmt、MSRV Clippy 检查、harness test/check 与 diff 检查通过。
- [x] 完成（2026-08-12）：在 Conversation Router 之前增加项目内 Task Resolver。用户消息先基于受限任务状态快照决定继续当前任务、切换已有任务、原子创建新任务/会话或澄清，再凭绑定项目、task、conversation 与完整请求的一次性 route receipt 写入用户消息并进入提交入口；后端公开执行入口强制消费凭证，同一 pending 只能成功消费一次，`keep` 不覆盖旧请求。任务状态只从真实 storyboard/时间线/preview/Agent 状态派生，不把会话摘要冒充任务记忆。验证：`cargo test` 96 个库测试与 2 个契约测试、前端 lint/build、harness 与 diff 检查通过，并完成三轮独立审查及最终定向复核；真实桌面 Provider 跨任务路由与重启后 pending 问题恢复 UI 待手工验证/后续补齐。
- [x] 完成（2026-08-12）：将消息入口重构为 Conversation Router。普通即时回复/状态观察不创建 `agent_tasks`；只有单工具副作用或多步任务创建 Agent run。首轮模型决策在同一响应中决定直接回复或首个执行技能，执行型响应作为 run 的 step 1 复用，避免重复模型调用；前端改用判别式轮次结果。schema v7 以结构化 `pending_clarifications` 保存 router/Agent run 澄清及 `pending/resolved/superseded` 生命周期；路由同轮声明 `keep/resolve`，Agent 状态快照携带安全澄清字段。完整独立 `ConversationRouterSnapshot` 仍可后续提取。验证：`cargo test` 87 个库测试与 2 个契约测试、前端 lint/build、harness、diff 检查及两轮独立审查通过；真实桌面 Provider 路由待验证。
- [x] 完成（2026-08-12）：优化交互 Agent 的意图与 Provider 调度。移除模糊请求独立分类模型往返，让首次模型响应同时声明产物目标并选择技能；结合最近待澄清状态理解用户补充文案；交互模型请求优先于后台粗视觉请求，视觉 Provider 连续三次失败后熔断 60 秒；增加 90 秒模型决策总预算和不含用户内容的安全耗时诊断。真实产物完成门、作用域校验和失败封闭保持不变。验证：`cargo test` 82 个库测试与 2 个契约测试通过；真实桌面 Provider 行为待手工验证。
- [x] 完成（2026-08-12）：将“剪好了吗/完成了吗”等精确状态问题改为只读确定性查询，不再进入模型 loop；状态只来自同作用域上一条 Agent 任务和真实产物标识，后台视觉任务不会污染剪辑完成状态。当前配置模型不支持图片输入时，粗视觉失败仍封闭在素材分析任务内；需配置支持视觉的粗视觉 Model 才能获得视觉证据。
- [x] 完成（2026-08-10）：可靠 Agent 第一阶段——schema v5 增加 payload-free `agent_run_steps` 与三重作用域查询；每轮重建统一状态快照并提供确定性前置条件提示；显式/循环技能持久化步骤终态；终态区分部分完成与待澄清；增加版本化工具契约和 10 个 Agent 回归用例，并修复显式无效 timeline ID 回退。验证：`cargo test` 36+2 项通过，`npm run lint` 与 `npm run build` 通过。
- [x] 修复代码审查发现的异步完成事件竞态与跨会话 UI 污染、Provider 凭据错误回退、模型原文日志、未达目标任务终态和时间线源结束点不一致，并补充回归测试与文档。验证：`npm run lint`、`npm run build`、`npm run harness:check`、`git diff --check` 通过；`cargo test` 28 项通过。
- [x] 为通用 Agent 工具调用实现持久化的任务/调用状态、步骤状态和可查询的操作日志；步骤记录不保存参数、模型原文、对话或媒体证据。
- [x] 明确并实现已持久化任务、运行步骤、时间线版本和操作日志的作用域查询契约，供 UI 和 Agent 审计使用。
- [ ] 将当前受限 Agent 控制器扩展为可恢复的本地 Agent 运行时，同时保持工具白名单、作用域校验和副作用审计。
- [x] 迭代（2026-08-10）：修复大批量素材导入导致的启动卡死与 `database is locked`。`db.rs::open_connection` 启用 WAL + 5 秒 busy_timeout；`assets.rs` 阈值场景检测限扫视频前 90 秒（`SCENE_SCAN_CAP_SECONDS`），新增全局 `ANALYSIS_WORKER_ACTIVE` 单 worker 守卫，启动恢复只处理前 4 条（`STARTUP_ANALYSIS_BATCH`），`list_assets` 轮询时按 4 条渐进排空当前项目队列（`DRAIN_ANALYSIS_BATCH`）；`jianying.rs` 剪映注册 worker 无待办时退避到 10 秒。验证：`cargo build --lib`、`cargo test` 48 项通过（46 单元 + 2 集成，exit 0）、`npm run lint` 0 警告、`npm run harness:check` 通过；`git diff --check` 通过。修复后需重新构建安装，重启后大量 `queued` 素材按每次 4 条逐步分析，不再冻结。
- [x] 迭代（2026-08-11）：定位 1008 素材项目启动 UI Hang 的直接原因：`list_assets` 每 1.5 秒对每条源路径同步 `Path::is_file()`，实测 1008 次检查耗时 89.235 秒（891 条失联、225 次单项超过 100ms）。移除 `sourceAvailable` 即时返回字段和全量探测，列表只返回持久化分析状态；分析、storyboard、preview 与 Jianying draft 仍在使用前校验源文件。验证：46 单元 + 2 集成测试、前端 lint/build、harness 和 diff 检查均通过；新 NSIS 安装版覆盖后连续运行 60 秒仍为 `Responding=True`。
- [x] 迭代（2026-08-11）：缩短首次素材分析的有界采样，保留完整 `ready` 证据门：视频场景扫描从前 90 秒缩至前 30 秒，关键帧从最多 8 张缩至 4 张，视频 OCR 和远端视觉建议各只处理前 2 张关键帧；不提前把半完成素材标记为 `ready`。更深度的按需采样仍为 TODO。验证：`cargo fmt --check`、`cargo test --lib`（47 通过）、`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 通过；完整 `cargo test` 因已有 `replace_text_tracks` fixture 不一致失败，详见变更记录。
- [x] 迭代（2026-08-11）：将远端视觉分析从导入关键路径移到 `analyze_asset_visual_batch` 后台队列。技术分析完成即可 `ready`；单一 worker 每批最多识别 6 条素材的中间代表帧，payload 仅保存素材 ID，结果仅保存计数、安全错误码与总耗时。视觉状态独立保存，失败不回退技术状态；启动会恢复中断批次并回填旧技术 `ready` 素材；storyboard 仅使用已有批量视觉证据的素材，避免盲选。候选多帧精检仍为 TODO。验证：`cargo test --lib`（55 通过）、前端 lint/build、harness 与 diff 检查通过；真实 Provider 桌面批量响应待验证。
- [x] 迭代（2026-08-11）：补齐批量视觉分析的候选完整性门。任一源文件可用的图片/视频仍处于视觉 `queued`/`running` 时，`generate_storyboard` 拒绝生成，而不是只在先完成的少数素材内选镜；视觉已失败、跳过或源文件失联的素材会诚实排除。验证见本次变更记录。
- [x] 迭代（2026-08-12）：针对本机 `767 queued + 1 running`、最早任务约 19 小时的分析堵塞，将技术分析改为最多 2 worker，并为 FFprobe、FFmpeg 缩略图/场景/回退抽帧及 Tesseract 增加 20-45 秒硬超时和子进程回收；超时素材失败后队列继续，启动会重排中断的本地运行任务。阶段级安全耗时指标仍为 TODO。验证见本次变更记录。
- [x] 迭代（2026-08-12）：场景检测改为先降至 4 fps、再 fast bilinear 缩到 320 像素宽、最后计算 scene；保留 `showinfo pts_time` 源时间。90 秒 1080p 基准的前 30 秒扫描平均从 3317ms 降至 2280ms，提升约 31.3%。验证见本次变更记录。
- [x] 迭代（2026-08-12）：实现远端识别优化 4/5/6。storyboard brief 在本地用显示名、文件夹组织 hint 与 OCR 对 queued 视觉批次排序，仅持久化数字优先级并最多等待最高相关批次 65 秒；Provider 请求复用进程级 HTTP Agent；自定义 API 新增可选粗视觉 Model，空值使用主 Model，OAuth 不猜测替代模型。路径/文件名/文件夹/OCR 不发送视觉 Provider、不作为 storyboard 证据。验证见本次变更记录。
- [x] 迭代（2026-08-11）：实现用户主动触发的素材根目录重新定位。`preview_asset_relink` 仅预览选定目录中唯一的旧相对路径 + 媒体类型匹配；前端显示匹配/未匹配数量并明确确认后调用 `confirm_asset_relink`。确认操作重新计算候选、更新源引用、清除旧分析证据、取消旧 active 分析任务并按最多 4 条重排；不确定项保持不变。验证：`cargo test` 48 项通过、`npm run lint`/`npm run build` 通过。待新安装版手工选择真实素材根目录验证。
- [x] 迭代（2026-08-12）：`confirm_asset_relink` 新增 `preserveAnalysis`：为 true 时仅在事务内更新源引用并保留既有分析证据（派生图片位于 app data 目录、不依赖源路径），不再清除 `metadata_json`、取消任务或重排分析；为 false 时保持既有重分析行为。前端确认对话框先询问是否同一批文件，确定走保留、取消则二次确认后重新分析。验证：`npm run lint` 通过；`cargo check --lib` 无新增错误（既有 `agentloop.rs` WIP 编译错误与本变更无关）。
- [x] 迭代（2026-08-07）：挖出“无可执行编辑决定”根因并补强决策 schema。将 `AgentEditDecision` 从扁平字段改为内部 `tool` 标签的关闭枚举，每个工具带独立 `deny_unknown_fields` 的 `params`；新增 `request_clarification`（澄清，不产任何产物）；把单一 `replace_timeline_clip` 拆为 `replace_clips`（批量替换保持镜头时长）、`change_clip_duration`（在已验证源范围内重定时长/起止）、`reorder_clips`（`order` 必须是完整排列）。`timeline.rs` 新增三个作用域函数与各自单测；`agent.rs` 决策流水线改为匹配枚举变体，`request_clarification`/`no_action` 不写副作用。验证：`cargo build --lib` 编译通过；`cargo test --lib` 15 通过、3 项依赖认证 Provider 的集成测试跳过。待重新构建安装后新工具才在桌面生效。
- [x] 迭代（2026-08-07）：先将 `execute_agent_edit` 异步化，不再阻塞桌面 UI。命令同步插入 `queued` 任务并立即返回任务 ID；在后台线程执行完整的模型决策、工具调用与副作用审计，完成后通过 `agent-edit-completed` 事件回传结果。前端订阅该事件应用产出、追加回复并定期轮询任务状态。作用域校验、工具白名单、`needs_review` 策略与操作日志保持不变；可恢复运行时（队列、暂停/恢复）仍在后续迭代实现。
- [x] 迭代（2026-08-07）：把决策 schema 改为顶层平铺的宽容形式（参数放在 JSON 最顶层、无嵌套 `params` 包装，多余键被容忍，`Unknown` 变体保证未识别工具不解析失败），并新增 `agentloop.rs` 有界技能循环：`execute_agent_edit` 只走确定的快速路径，遇到未识别工具时升级为按步多技能循环（观察 `list_assets`/`get_storyboard`/`get_timeline`，编辑/交付 `generate_storyboard`/`create_timeline_draft`/`replace_clips`/`change_clip_duration`/`reorder_clips`/`render_preview`/`create_jianying_draft`，`finish`/`ask_user`/步数上限 6 结束）。验证：`cargo build --lib` 编译通过；`cargo test --lib` 18 通过、3 项依赖认证 Provider 的集成测试跳过。待真实 Provider 响应验证升级路径。
- [x] 迭代（2026-08-07）：把请求决策彻底统一为单一目标驱动的有界技能循环。删除 `AgentEditDecision`/`AgentEditCommon` 开放式 schema 与 `ToolDecisionProvider`/`ModelToolDecisionProvider`，不再有“已识别快速路径 + Unknown 升级”分叉；`agent.rs` 只保留 `explicit_command_tool` 精确匹配的显式单命令（“创建剪映草稿”“创建内部时间线”“生成预览”等）走 `run_explicit_command` 确定性直通路径，其余所有自然语言请求直接进入 `agentloop.rs::run_agent_loop`。循环先按 `derive_loop_goal` 派生产物目标（问答/storyboard/时间线/预览/剪映草稿），`finish`/`no_action`/`done` 只有在该目标产物真实存在（`satisfied_by`）时才结束，否则给纠偏消息继续至步数上限（6）；所有技能复用既有作用域与范围校验、生成新版本并写审计，失败只回读错误；终端回复由真实产物组装，无产物时用固定诚实文案。验证：`cargo build --lib`；`cargo test --lib`（19 通过，含 agentloop 新增 6 项）；`npm run lint` 与 `npm run build` 通过。待真实 Provider 响应验证循环目标判定与产物落地。

- [x] 迭代（2026-08-07）：修复“右下角一直显示正在分析媒体但从不推进”的卡死。`assets.rs::resume_incomplete_analysis` 增加启动对账：对状态为 `queued`/`analyzing` 但没有任何 `analyze_asset` 任务行的孤立素材（例如导入中断未持久化任务），每次启动补建并排队分析，避免这类素材永久停在“正在分析媒体”；已有任何任务行的素材绝不重复排队。验证：`cargo build --lib`、`cargo test --lib`（19 通过）通过；本地查见 `dc81c11e` 项目 54 个自 07-31 起孤立的 `queued` 素材，重启桌面应用后这些素材会被补齐并真正完成分析。
- [x] 迭代（2026-08-07）：让 Agent 能像自然语言对话一样连续交流（ADR-033）。目标派生从纯关键词改为“确定性快路径 + 模型分类”：`agentloop.rs` 新增 `fast_goal`（`EDIT_VERBS`/`CREATE_VERBS`/`QUESTION_PHRASES`，明确命令/编辑/提问直接判定，`EDIT_VERBS` 不再含“镜头”等名词），无法确定的请求才用一次轻量模型调用分类（`classify_goal_with_model` 携带会话历史，`isQuestion` 为真一律归问答，分类失败默认问答）；同时新增多轮记忆 `load_message_history`/`render_history`，把该会话最近消息（上限 12 条、字符预算 8000、排除当前请求）拼进 `build_step_prompt` 与分类提示。补充（同批）：`run_step`/`classify_goal_with_model`/`storyboard::generate_storyboard` 的模型请求分别带 120s/30s/120s 超时，超时按失败保存安全结果并返回固定降级回复，杜绝 Provider 不响应时的无限挂起；`run_agent_loop` 捕获 `run_step` 的模型/provider 失败，按目标返回 `model_unavailable_message` 的诚实降级回复而非冒泡成通用失败。验证：`cargo build --lib`（无警告）、`cargo test --lib`（25 通过，新增 7 项 `fast_goal`/`parse_classified_goal`/`load_message_history`/`render_history` 单测）。前端无改动。待真实 Provider 正常响应后桌面验证“请告诉我选择每个镜头的逻辑”能返回自然回答。

执行记录（2026-08-06）：将 `store.rs` 单体模块按职责拆分为 `db`、`models`、`process`、`provider`、`audit`、`projects`、`assets`、`storyboard`、`timeline`、`preview`、`jianying`、`agent` 模块；Agent 控制器抽成独立 `agent.rs`，并引入 `ToolDecisionProvider` trait 将模型决策层与副作用执行层解耦。`execute_agent_edit` 保持单一入口与既有 Tauri 命令契约不变。验证：`cargo test` 通过 13 项、3 项依赖认证实验性 Provider 的集成测试按设计跳过；`npm run lint`、`npm run build`、`npm run harness:check` 通过。异步可恢复的本地 Agent 运行时（队列、暂停/恢复）仍为未完成项。

执行记录（2026-08-05）：已实现通用 Agent 调用持久化、作用域化查询、关联操作日志、时间线版本查询、`needs_review` 中断恢复策略和当前会话审计界面。`cargo test` 已通过 12 项；3 项依赖认证实验性 Provider 的集成测试按设计跳过，仍待桌面手工验收后关闭本组 P0 项。

执行记录（2026-08-05）：受限工具失败不再将技术校验错误直接交给 UI；后端会记录安全失败代码并请求模型生成自然语言后续回复，且该回复回合不自动重试失败工具。

执行记录（2026-08-05）：为视觉分析请求增加 30 秒超时并返回失败原因（`visualAnalysisNote`），消除远端接口不响应导致素材卡在“正在分析媒体”的永久阻塞；新增 `clear_experimental_openai_oauth` 退出登录命令并在模型弹窗提供按钮。

执行记录（2026-08-06）：修复重复渲染同一内部时间线版本时 preview 不更新的问题。`render_preview` 对同一 timeline version 写入相同路径，浏览器按 URL 缓存旧文件；现通过 `previewNonce` 在每次新 preview 结果落地时递增，并作为视频 src 的查询参数强制重新加载。验证：`npm run lint`、`npm run build`、`npm run harness:check` 通过。

### P1：模型接入与生产运行时

- [ ] 核实适用于桌面应用的官方 OpenAI OAuth 机制、scope、模型能力和刷新行为。
- [ ] 为托管模型 API 增扩展更多 Provider 适配器；不可让项目逻辑绑定某个厂商的响应格式。
- [ ] 为生产安装包捆绑或可靠地配置 FFmpeg/FFprobe、Tesseract 与语言数据、Python 和 Jianying 适配器依赖。

执行记录（2026-08-07）：增加自定义 OpenAI 兼容模型 API 入口。新增 `custom_api.rs` 模块与 `get_custom_api_status`/`save_custom_api`/`clear_custom_api` 命令，凭据（Base URL + Model + API Key）存 Windows Credential Manager；`provider.rs` 引入 `ModelAccess`（OAuth Responses 或 Chat Completions）与 `post_model_payload`/`model_response_json_text` 决策分派，自定义 API 走 `{baseUrl}/chat/completions` 并把 Responses 载荷转换为 `messages`/`response_format`。`agent.rs`、`storyboard.rs`、`assets.rs` 改经 `ModelAccess::resolve()`（自定义 API 优先，否则回退 OAuth）。前端 Provider 弹窗新增自定义 API 表单（Base URL/Model/API Key，保存/清除/状态）。验证：`cargo build --lib`、`cargo test --lib`（15 通过、3 依赖认证 Provider 跳过）、`npm run lint`、`npm run build`、`npm run harness:check` 通过。自定义 API 的真实托管模型响应仍待桌面手工验证。

### P1：媒体与创作能力

- [x] 完成（2026-08-13）：素材库实现名称/文件夹/相对路径搜索，媒体类型、技术状态、视觉状态、storyboard 可用性和文件夹范围组合筛选，以及结果计数、无结果状态和清空入口。`StoredAsset` 增加真实 `visualAnalysisStatus`，不暴露绝对源路径、不做同步源探测。验证：前端 lint（仅既有 Hook 依赖警告）/build、98 个 Rust 库测试、harness 与 diff 检查通过；未启动或重启桌面程序。下一项为后端分页、摘要查询与前端虚拟列表。
- [x] 完成（2026-08-13）：新增 `list_asset_page` 有界摘要分页契约，搜索与类型/技术/视觉/storyboard 可用性/文件夹筛选下推 SQLite，返回匹配总数、项目级状态计数和安全文件夹 facet；前端默认加载 100 条并滚动续页，以固定 74px 行窗口化渲染可视区。保留内部 Agent `list_assets` 语义。验证：分页组合条件定向 Rust 测试、前端 build 和 98 个既有 Rust 库测试通过；未启动或重启桌面程序。下一项为批量选择与素材任务中心。
- [x] 完成（2026-08-13）：素材库支持最多 200 条批量选择、技术分析批量重试和确认后的视觉分析批量跳过；新增任务中心分别展示技术/视觉运行、排队、失败、跳过与安全的最近失败，并可定位或重试最近技术失败。两项批量副作用校验项目归属并写用户操作审计；显式视觉跳过保留技术证据、清除视觉标签且不会被在途视觉批次覆盖。验证：前端 lint/build、100 个 Rust 库测试、Rust fmt、harness 与 diff 检查通过；未启动或重启桌面程序。下一项为收藏、标签、集合与禁止使用。
- [x] 完成（2026-08-13）：schema v12 将收藏、0–5 评分、备注、禁止使用、用户标签和素材集合与分析证据分表持久化；重新分析不会覆盖用户整理结果。素材库支持批量设置、按用户状态/集合过滤，搜索命中备注和标签；禁止使用素材从新 storyboard 候选中硬排除。写操作校验同项目 1–200 条并记录不含用户正文的审计。验证：前端 lint/build、104 个 Rust 库测试、Rust fmt、harness 与 diff 检查通过。未启动或重启当前程序，当前运行数据库未执行迁移。下一项为 Agent 素材检索工具。
- [x] 完成（2026-08-13）：Agent 循环新增受限只读 `search_assets`，支持查询、类型、时长、评分、收藏、标签、集合与 offset/limit 游标，单页最多 20 条，自动排除禁止使用素材；仅返回安全候选摘要和命中原因码，不返回路径、备注/OCR 正文或媒体内容。执行卡增加“检索素材候选”映射。验证：前端 lint/build、105 个 Rust 库测试、Rust fmt、harness 与 diff 检查通过；未启动或重启程序。下一项为异步源文件健康扫描。
- [x] 完成（2026-08-13）：新增 schema v13 源文件健康快照和显式触发的可取消后台扫描。列表只读持久化状态；扫描仅读取大小/修改时间并区分正常、缺失、已变化、不可读、未检查，新导入和确认重链路建立基线。素材面板展示汇总和启动/取消入口；未启动或重启程序。下一项为片段级检索。
- [x] 完成（2026-08-13）：Agent 新增受限只读 `search_asset_segments`，基于真实场景段和时间点证据返回明确源时间范围、安全视觉标签、原因码与游标；排除禁止使用以及已知缺失、变化、不可读素材，不泄露路径或 OCR 正文。执行卡增加片段检索映射；未启动或重启程序。下一项为收集项目素材。
- [x] 完成（2026-08-13）：新增项目素材收集预览和执行入口。用户确认文件数、不可用数及估算体积并选择目录后，系统创建不可覆盖的 UUID 新包，复制可读源文件、生成不含原始路径的 manifest，只审计计数且不修改当前项目引用。最终验证：前端 lint（仅既有 Hook 依赖警告）/build、107 个 Rust 库测试、Rust fmt、harness 与 `git diff --check` 通过；未启动或重启程序。
- [x] 修复显式“生成预览”在前端未携带 storyboard ID 时错误拒绝同一剪辑任务有效时间线的问题：按项目和剪辑任务查询候选并保持跨任务拒绝；上下文不完整时将已验证事实回读模型决定下一步，不把渲染动作写死；补充作用域回归测试。验证：`cargo test --lib`、`npm run lint`、`npm run build`、`npm run harness:check`。
- [x] 完成（2026-08-10）：将 Agent 循环改为十步分层预算、模型主导的 storyboard 内存修订与部分完成沟通；新增对项目内未分析素材的受控分析请求，并把创作阈值改为模型提案加安全上限。验证见 `docs/changes/2026-08-10-model-centered-storyboard-runtime.md`。
- [x] 完成（2026-08-10）：将 storyboard 生成从一次性素材拼接改为文案信息点驱动的选镜与质量门；`insufficient` 只记录为未覆盖信息点，不得作为成片镜头落入时间线。明确文案的 storyboard 校验失败会诚实结束，不会重新索要成片目标。验证：Rust 单元测试新增 5 项；`cargo fmt --check`、`cargo test --lib`、`npm run lint` 与 `npm run build` 通过。真实桌面素材选镜待手工验证。
- [ ] 实现收集项目媒体（collect-project-media），包括可移植项目目录与缺失源文件的用户处理流程。
- [ ] 完成视觉质量评分和语义重复检测；当前仅有低分辨率、单帧的重复候选提示。
- [ ] 定义缺失源媒体时的恢复、重定位或跳过策略。

### P1：Jianying 与声音

- [x] 完成（2026-08-12）：版本化内部时间线新增 `MusicTrack`/`MusicCue` 与 `replace_music_tracks` 受限 Agent 工具；音乐 cue 只接受分析完成的本地音频素材，校验源/时间线范围、循环、0–2 音量和淡入淡出。FFmpeg preview 在固定采样率下按 cue 源范围裁剪/循环/延迟并本地混音且不修改源媒体；已用合成音频回归验证循环音频轨落地和禁用轨跳过。Jianying 适配器现可创建含音乐轨的新实验草稿，已验证草稿结构；仍待 Jianying UI 试听验收，绝不覆盖既有 draft。
- [x] 完成（2026-08-12）：Jamendo Provider 通过 Windows Credential Manager 接入受限 `search_music`、`download_music` 与 `use_online_music`。仅接受 API 明示可下载且为 CC0 或 CC-BY 的单曲；CC-BY 署名随 music cue 持久化。`use_online_music` 只下载一首到当前 local project 的唯一受控文件、等待本地分析完成，再新建音乐时间线版本并保留审计。真实目录 API 已返回许可字段；完整桌面 Agent + Jianying UI 试听仍待人工验收，最终导出仍须用户确认。

- [ ] 进行中（2026-08-11）：实现剪映兼容优先的文本轨。已增加版本化 `TextTrack` 与 `replace_text_tracks` 受限 Agent 工具，后端严格校验时间、颜色、样式/布局、基础动态及唯一 ID；`render_preview` 已通过 ASS + FFmpeg/libass 渲染文本轨与基础动态。Jianying Pro 8.0 UI 已验收 `jianying_default` 字体下的静态、淡入/淡出、向上滑入、向下滑入、弹入，因此这些无描边/阴影/背景/循环的 cue 可写入新草稿；当前剪映 11.2 通过嵌套文本 JSON 的 Unicode 转义后，已实机验收中文 cue 正确显示。适配器已能写入描边、背景、阴影和五个剪映内置字体资源，但它们仍为 `local_preview_only` 且会明确拒绝，避免静默丢失。新增带视频轨的五层样式矩阵草稿，已确认剪映中有一条视频轨和五条独立文本轨，并在 UI 中看见描边阴影与背景卡片；适配器层级映射与 Unicode 文本序列化均有回归测试。待逐项视觉验收字体、描边阴影与背景卡片模板后再扩大可交付矩阵。

  模型可通过 `get_text_capabilities` 选择后端解析的 `subtitle_safe`、`headline_rise`、`headline_pop`、`headline_drop`、`callout_card`、`cta_card` 文本预设；前四项为已验证可交付配方，并固定已验证的淡出；后两项保留为 local preview。前端当前会显示文本 cue 的内容、时段、字体、入场/出场模板与 Jianying 兼容状态，供审阅 Agent 已落地的文本设计。已完成（2026-08-12）：能力目录的每个预设均输出 `selectionHint`；模型首次制作文本前必须先读取目标 timeline 和能力目录，必要时再读取 storyboard，并按对白/旁白、递进/揭示、反差/结果、结论/警示的语义选择配方。同一视觉 beat 至多一个 headline；未获用户明确接受不得使用 local-preview-only 卡片。文本轨 QA 已扩展为：阅读密度、两行限制、动画占比和相邻重复文案以 `qualityWarnings` 回读模型，跨轨 headline 重叠则拒绝。验证：`cargo test --lib`（60 通过）、`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 通过。

  `TextTrack.layer` 现在会映射为 preview 的 ASS layer 与 Jianying draft 的独立命名文本轨；同一文本轨的 cue 不得重叠，跨轨重叠则按 layer 合法叠放。
- [ ] 将 Jianying Pro 8.0 适配器从当前视频、受限文本和实验性音乐轨扩展到图片、完整字幕和 logo 轨道；完成音乐播放 UI 验收后才提升其交付状态。
- [ ] 获得并集成用户提供的 voice API 契约。

- [x] 迭代（2026-08-11）：修复“分析素材”被模型误分类为 storyboard，并落实工具优先 Agent 规则。轻量分类器把明确素材分析请求设为无产物门的观察目标，不替模型执行工具；模型在受限循环内按真实状态选择 `list_assets`、`request_asset_analysis`、澄清或其他合法工具。验证：更新模型分类/技能提示，`cargo test --lib` 通过。

- [x] 迭代（2026-08-11）：扩大模型工具操作空间。未限定“草稿”不再直通 Jianying；preview/Jianying draft 不再隐式创建内部时间线，模型必须显式选择 `create_timeline_draft`；前置提示改为真实状态和可用工具而非固定步骤顺序。验证：46 单元 + 2 集成测试、前端 lint/build、harness 与 diff 检查通过；新安装版启动后 `Responding=True`。

## 已完成基础能力

- [x] 收敛素材前端（2026-08-13）：素材搜索、状态筛选、批量整理、集合和任务明细仅保留为 Agent/后台能力；主工作区使用只读层级文件夹目录，每级保留直属子文件夹并只展示直属素材。早期缺少文件夹根记录的批量单文件导入会在内存中从共同父级重建安全相对目录，无法成树的素材才归入“未归类素材”；目录键不含绝对路径。

- [x] Tauri 2 Windows 桌面壳、MSI/NSIS 打包验证、应用数据目录 SQLite 迁移。
- [x] 项目、剪辑任务、会话、消息、素材、storyboard 和内部时间线版本的本地持久化与作用域隔离。
- [x] 原生文件/文件夹导入、递归媒体发现、源媒体可用性检测和文件夹层级展示。
- [x] FFprobe 技术元数据、FFmpeg 缩略图/关键帧/场景候选、英文 OCR 和可检查的素材证据。
- [x] 实验性 OAuth 凭据通过 Windows Credential Manager 存储，以及最小帧视觉分析。
- [x] 证据校验的 storyboard、源时间绑定内部时间线、批量片段替换、改时长、排序、新版本和 540 x 960 preview。
- [x] preview 黑帧、精确重复源范围、低分辨率相似候选、节奏异常和未渲染 storyboard 文本检查。
- [x] 受限自然语言工具选择、后端作用域校验，以及唯一的新 Jianying Pro 8.0 仅视频草稿创建。
- [x] Git 变更集驱动的文档同步 harness：高影响源码路径规则、架构变更记录、提交前检查和独立 Agent 审查 loop。

## 待决问题

- [ ] 创作透明度（问题 4）：模型自主添加的文案、音乐、时长调整必须在生成时明确声明，状态查询时区分用户要求的内容和模型添加的内容。这是双刃剑，需要深度讨论后再决定实现方式。
- [ ] 哪种官方 OpenAI OAuth 机制、scope 和模型端点可用于此桌面应用？
- [ ] 收集媒体后的本地项目目录结构和长期 SQLite 迁移策略应如何定义？
- [ ] 哪些视觉分析模型应本地运行，哪些可发送到托管 Provider？
- [ ] voice API 的鉴权、端点、请求/响应、音色目录和异步任务模型是什么？
- [ ] 除本地媒体存储外，生产应用是否还需要完整离线模式？
- [ ] 自定义模型 API 是否必须支持局域网 `http://`/localhost？（影响 Base URL 校验策略；见 `docs/codebase/CONCERNS.md` §7，阻断 `custom_api.rs` URL 校验加固）
- [ ] `src/lib/agent-tools.ts` 应成为可发布 SDK 契约还是仅作 IDE 镜像？（建议由 Rust fixture 自动生成；见 `docs/codebase/CONCERNS.md` §7，阻断 agent-tools 生成/删除决策）
