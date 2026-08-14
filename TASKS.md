# 任务清单

## 当前优先级

- [x] 恢复现场（2026-08-14）：在 `codex/recovery-baseline-20260814` 分支提交当前完整工作区，快照 commit 为 `8020d73`。该提交用于回退和审计，不代表可交付版本。
- [x] 完成（2026-08-14）：恢复绿色构建基线。仅修复前端 TypeScript 契约、未使用代码和 Hook 依赖问题，并统一现有 Rust 格式；未调整 UI 信息架构，未改变 Agent、媒体分析、timeline、preview 或 Jianying 行为。
- [x] 本项完成门：`npm run lint`、`npm run build`、`cargo fmt --check`、`cargo test`（112 个单元测试 + 2 个契约测试）、`npm run harness:check` 与 `git diff --check` 全部通过；变更形成独立 commit，提交后工作区恢复干净。
- [x] 桌面事实基线（2026-08-14）：真实 Tauri 应用成功恢复旧项目的 891 个素材、8 镜头 storyboard、8 片段 timeline 和本地 preview；首个核心阻断是工作模式没有隔离。点击“故事板”只改变标签状态，完整对话、composer 与审计仍在前面，需连续翻页才能到达无基础样式的 storyboard；Workflow 同时在 `App.tsx` 和 `ConversationWorkspace` 渲染。完整证据与根因见 `docs/audits/2026-08-14-desktop-product-baseline.md`。
- [x] 桌面审计完成门：已按阻断级别记录恢复状态、首个断点、代码根因和历史缺口。真实 preview 画面可加载；Provider、Agent 新请求、完整播放、媒体重新分析和 Jianying 仍未在本轮关闭，继续保留为 P0。
- [x] 完成（2026-08-14）：恢复互斥顶层工作模式。Agent、素材、成果一次只渲染一个主工作区；素材管理进入完整宽度工作区；成果页集中展示 storyboard、timeline/审计与 preview；只保留一套 Workflow，未改变后端、Agent 工具或持久化行为。
- [x] 本项完成门：真实桌面 1440×900 验收确认三个模式立即替换主内容；Agent 模式只显示消息、执行卡与 composer，素材模式显示 308/520/360px 三栏和 100 条有界素材页，成果模式只显示一套 Workflow 与 8 个 storyboard 镜头。`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 全部通过。
- [x] 完成（2026-08-14，P0）：在不生成新产物的前提下完成 Provider 与 Agent 只读链路验收。真实后端状态为自定义 API 已连接、主 Model 已配置、实验性 OAuth 未连接，和 UI 一致；没有读取或输出 API Key。精确“剪好了吗？”增加一对 user/agent 消息但 Agent task 数不变；项目事实问题进入当前 task/conversation，只执行 `get_storyboard → get_timeline → finish`，准确报告 8 个镜头、8 个片段和现有 local preview，且 storyboard/timeline/preview ID 全部不变。
- [x] 本项完成门：修复状态查询只看最近 Agent task 而忽略当前真实 preview，以及成功 Agent run 未持久化最终回复、conversation 卡在 `working` 的两个 P0 缺陷。终态任务与 `agent-task-result-{agentTaskId}` 回复现于同一事务提交；启动会把历史“终态但无回复”的 working 会话恢复为 `needs_review` 并补固定消息。真实桌面切换模式和刷新后，13 条持久化消息与 13 条可见消息一致，最新完成回复只有 1 条，conversation 保持 `ready`。
- [x] 完成（2026-08-14，P0）：在当前真实剪辑任务中用显式 Agent 请求创建内部 timeline v5 和对应 local preview。项目/task/conversation/storyboard 作用域未变，timeline 仅由 2 个增至 3 个、版本由 v4 增至 v5；旧 v4 timeline 与 preview 文件保留，新旧 preview 均存在。新 preview 为 540×960、29.47 秒，真实播放器进度可前进；未创建 Jianying draft、未最终导出、未删除或重新分析素材。
- [x] 本项完成门：修复 `submit_conversation_turn.run` 实际返回 `agent_task_id`、前端却读取 `agentTaskId` 导致 pending ID 为 undefined，以及任务快照暂缺、active→terminal、首次快照已 terminal 时过早放弃轮询的 P0 竞态。新增精确序列化测试和前端空 ID 失败门。修复后真实只读 Agent run 的 pending ID 与数据库 task ID 一致，completed 后后端/可见消息同步为 23、回复仅 1 条、conversation 为 ready；WebView 刷新和 Tauri 重启均恢复 v5 preview 与全部消息。
- [x] 完成（2026-08-14，P0）：以 timeline v5 为基线，用 `change_clip_duration` 只创建 v6；第 2 镜头从 3000 ms 缩短至 2500 ms，源范围从 250–2900 ms 收敛为 250–2750 ms，其他镜头素材与顺序不变，后续片段统一前移 500 ms。timeline 数量仅从 3 增至 4，旧 v5/v4/v3 均保留。v6 local preview 为 540×960、29.3 秒并可实际播放；Tauri 重启和 WebView 刷新后 27 条消息及 v6 preview 恢复，v5/v6 preview 文件同时存在。
- [x] 本项完成门：首次自然语言调整因模型参数未通过后端校验而安全失败，未产生版本或操作日志；第二次绑定真实 v5 ID 与唯一 adjustment 后成功。该请求明确“不生成 preview”，旧 `fast_goal` 却把否定词中的 preview 锁为完成目标并强制渲染，已新增请求级 `RequestToolPolicy`：负向 preview/Jianying/素材分析约束同时限制路由工具、目标声明与每步执行；排除素材分析也会禁用触发分析的在线媒体获取工具。`fast_goal` 只锁定带明确动作的产物请求或清晰问题，名词/状态短句留给首轮主模型；Agent `list_assets` 现为无调度快照，Agent `generate_storyboard` 只消费已就绪证据。“只读/readonly”按分句解释，禁用全部编辑与交付工具并阻止 `taskBrief` 持久化；路由失败回退仍保留当前项目事实观察门。修复后真实只读回归只执行 `get_timeline → finish`、操作日志 0、版本仍为 v6/v5/v4/v3、preview 文件时间戳不变，后端/界面消息同步为 29；未创建 Jianying draft、未最终导出、未删除或重新分析素材。
- [ ] 下一步（P0）：对当前 timeline v6 进行只读媒体事实审计，逐镜头核对 storyboard 目的、素材证据、明确源时间范围与 preview 抽帧，确认“选择了什么、为什么选择、实际画面是否一致”；不得重新分析素材、修改 timeline、生成新 preview、创建 Jianying draft 或导出。

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

- [ ] 哪种官方 OpenAI OAuth 机制、scope 和模型端点可用于此桌面应用？
- [ ] 收集媒体后的本地项目目录结构和长期 SQLite 迁移策略应如何定义？
- [ ] 哪些视觉分析模型应本地运行，哪些可发送到托管 Provider？
- [ ] voice API 的鉴权、端点、请求/响应、音色目录和异步任务模型是什么？
- [ ] 除本地媒体存储外，生产应用是否还需要完整离线模式？
