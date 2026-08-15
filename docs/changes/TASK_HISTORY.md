# 历史执行记录

本文件归档已完成的任务条目。当前任务窗口与最近七条完成记录见根目录 [`TASKS.md`](../../TASKS.md)。

## 2026-08-14

- [x] 恢复现场（2026-08-14）：在 `codex/recovery-baseline-20260814` 分支提交当前完整工作区，快照 commit 为 `8020d73`。该提交用于回退和审计，不代表可交付版本。
- [x] 完成（2026-08-14）：恢复绿色构建基线。仅修复前端 TypeScript 契约、未使用代码和 Hook 依赖问题，并统一现有 Rust 格式；未调整 UI 信息架构，未改变 Agent、媒体分析、timeline、preview 或 Jianying 行为。
- [x] 本项完成门：`npm run lint`、`npm run build`、`cargo fmt --check`、`cargo test`（112 个单元测试 + 2 个契约测试）、`npm run harness:check` 与 `git diff --check` 全部通过；变更形成独立 commit，提交后工作区恢复干净。
- [x] 桌面事实基线（2026-08-14）：真实 Tauri 应用成功恢复旧项目的 891 个素材、8 镜头 storyboard、8 片段 timeline 和本地 preview；首个核心阻断是工作模式没有隔离。点击"故事板"只改变标签状态，完整对话、composer 与审计仍在前面，需连续翻页才能到达无基础样式的 storyboard；Workflow 同时在 `App.tsx` 和 `ConversationWorkspace` 渲染。完整证据与根因见 `docs/audits/2026-08-14-desktop-product-baseline.md`。
- [x] 桌面审计完成门：已按阻断级别记录恢复状态、首个断点、代码根因和历史缺口。真实 preview 画面可加载；Provider、Agent 新请求、完整播放、媒体重新分析和 Jianying 仍未在本轮关闭，继续保留为 P0。
- [x] 完成（2026-08-14）：恢复互斥顶层工作模式。Agent、素材、成果一次只渲染一个主工作区；素材管理进入完整宽度工作区；成果页集中展示 storyboard、timeline/审计与 preview；只保留一套 Workflow，未改变后端、Agent 工具或持久化行为。
- [x] 本项完成门：真实桌面 1440×900 验收确认三个模式立即替换主内容；Agent 模式只显示消息、执行卡与 composer，素材模式显示 308/520/360px 三栏和 100 条有界素材页，成果模式只显示一套 Workflow 与 8 个 storyboard 镜头。`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 全部通过。
- [x] 完成（2026-08-14，P0）：在不生成新产物的前提下完成 Provider 与 Agent 只读链路验收。真实后端状态为自定义 API 已连接、主 Model 已配置、实验性 OAuth 未连接，和 UI 一致；没有读取或输出 API Key。精确"剪好了吗？"增加一对 user/agent 消息但 Agent task 数不变；项目事实问题进入当前 task/conversation，只执行 `get_storyboard → get_timeline → finish`，准确报告 8 个镜头、8 个片段和现有 local preview，且 storyboard/timeline/preview ID 全部不变。
- [x] 本项完成门：修复状态查询只看最近 Agent task 而忽略当前真实 preview，以及成功 Agent run 未持久化最终回复、conversation 卡在 `working` 的两个 P0 缺陷。终态任务与 `agent-task-result-{agentTaskId}` 回复现于同一事务提交；启动会把历史"终态但无回复"的 working 会话恢复为 `needs_review` 并补固定消息。真实桌面切换模式和刷新后，13 条持久化消息与 13 条可见消息一致，最新完成回复只有 1 条，conversation 保持 `ready`。
- [x] 完成（2026-08-14，P0）：在当前真实剪辑任务中用显式 Agent 请求创建内部 timeline v5 和对应 local preview。项目/task/conversation/storyboard 作用域未变，timeline 仅由 2 个增至 3 个、版本由 v4 增至 v5；旧 v4 timeline 与 preview 文件保留，新旧 preview 均存在。新 preview 为 540×960、29.47 秒，真实播放器进度可前进；未创建 Jianying draft、未最终导出、未删除或重新分析素材。
- [x] 本项完成门：修复 `submit_conversation_turn.run` 实际返回 `agent_task_id`、前端却读取 `agentTaskId` 导致 pending ID 为 undefined，以及任务快照暂缺、active→terminal、首次快照已 terminal 时过早放弃轮询的 P0 竞态。新增精确序列化测试和前端空 ID 失败门。修复后真实只读 Agent run 的 pending ID 与数据库 task ID 一致，completed 后后端/可见消息同步为 23、回复仅 1 条、conversation 为 ready；WebView 刷新和 Tauri 重启均恢复 v5 preview 与全部消息。
- [x] 完成（2026-08-14，P0）：以 timeline v5 为基线，用 `change_clip_duration` 只创建 v6；第 2 镜头从 3000 ms 缩短至 2500 ms，源范围从 250–2900 ms 收敛为 250–2750 ms，其他镜头素材与顺序不变，后续片段统一前移 500 ms。timeline 数量仅从 3 增至 4，旧 v5/v4/v3 均保留。v6 local preview 为 540×960、29.3 秒并可实际播放；Tauri 重启和 WebView 刷新后 27 条消息及 v6 preview 恢复，v5/v6 preview 文件同时存在。
- [x] 本项完成门：首次自然语言调整因模型参数未通过后端校验而安全失败，未产生版本或操作日志；第二次绑定真实 v5 ID 与唯一 adjustment 后成功。该请求明确"不生成 preview"，旧 `fast_goal` 却把否定词中的 preview 锁为完成目标并强制渲染，已新增请求级 `RequestToolPolicy`：负向 preview/Jianying/素材分析约束同时限制路由工具、目标声明与每步执行；排除素材分析也会禁用触发分析的在线媒体获取工具。`fast_goal` 只锁定带明确动作的产物请求或清晰问题，名词/状态短句留给首轮主模型；Agent `list_assets` 现为无调度快照，Agent `generate_storyboard` 只消费已就绪证据。"只读/readonly"按分句解释，禁用全部编辑与交付工具并阻止 `taskBrief` 持久化；路由失败回退仍保留当前项目事实观察门。修复后真实只读回归只执行 `get_timeline → finish`、操作日志 0、版本仍为 v6/v5/v4/v3、preview 文件时间戳不变，后端/界面消息同步为 29；未创建 Jianying draft、未最终导出、未删除或重新分析素材。
