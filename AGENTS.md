# Assembly Video Agent：Agent 上下文

## 产品规则

Assembly Video Agent 是 Windows 优先的本地视频剪辑 Agent。用户主要通过自然语言协作；视频分析、storyboard、时间线、preview 和 Jianying draft 创建是 Agent 可调用的工具。

- 构建 Agent，不构建带聊天框的传统剪辑器。
- 必须使用真实媒体分析和明确源时间范围；不得从文件名推测媒体内容。
- Agent 可不经逐项确认创建内部时间线、低清 preview 和新的 Jianying draft。
- 自然语言 Agent 的能力应以工具、受限状态快照和明确副作用边界提供给模型，而不是不断增加关键词驱动的业务直通分支或少量写死选项。除外部安全确认、作用域校验和真正无需模型判断的显式单命令外，模型应在有界循环中自主选择观察、编辑和交付工具；分类器只能决定产物完成门，不得代替模型决定具体工具。
- 最终导出、覆盖既有导出，或删除项目、素材、版本前必须获得明确确认。
- 原始媒体、项目数据、preview、内部时间线和 Jianying draft 必须保留在本机。
- Provider 必须可替换；OpenAI OAuth 只是支持入口之一。
- Jianying 交付是单向的：MVP 中不得覆盖既有 Jianying draft，也不得尝试同步用户在 Jianying 内的编辑。

## 当前实现边界

仓库包含 React/Vite 前端和 Tauri 2 Windows 后端，具备本地 SQLite、原生素材导入、FFprobe/FFmpeg/Tesseract 分析、右下角媒体分析任务提示、实验性 OAuth 与自定义 OpenAI 兼容 API 两种模型调用入口、证据绑定 storyboard、源时间绑定时间线、本地 preview、通用 Agent 调用与操作审计，以及实验性的 Jianying Pro 8.0 仅视频草稿创建。模型请求统一经 `ModelAccess` 决策：自定义 API（Base URL + Model + API Key，存 Windows Credential Manager）已配置时优先，否则回退到实验性 OAuth。受限工具技能失败时只回读错误给模型供下一步决策，绝不自动无限重试；Provider 或模型不可用、或循环最终无法达成目标时保存安全结果并给出固定诚实降级回复。不得把技术错误直接显示给用户，也不得把模型捏造的“已完成”当真实产物。中断的 Agent 调用转为待审阅，不得自动重放未知副作用。外部命令在 Windows 上必须无窗口运行。更多自定义 Provider、生产安装包媒体运行时、多轨音频/字幕、最终视频导出和反向同步尚未实现，不得声称已经具备。

自定义 API 凭据只有明确不存在时才允许回退到实验性 OAuth；Windows Credential Manager 读取失败或凭据损坏必须阻止模型请求，避免改发到用户未预期的 Provider。模型响应原文、会话内容和媒体证据不得写入运行日志。Agent run 的 task 终态、可选产物审计、以任务 ID 派生的最终回复和 conversation 终态必须在同一 SQLite 事务中提交，成功后才能发出异步 `agent-edit-completed` 通知；`submit_conversation_turn` 的 `run` 结果必须以公开 camelCase 字段 `agentTaskId` 返回真实任务 ID，前端不得接受空 ID。前端必须同时支持命令返回竞态对账和持久化终态轮询恢复；轮询生命周期由仍归该请求所有的 composer 或持久化 `working` conversation 决定，不得因一次任务列表缺失或 task 从 active 变 terminal 而提前停止。持久化 conversation 仍为 `working` 且没有内存 pending 时，即使第一次任务快照已经 terminal，也必须对最新同作用域 terminal task 执行一次恢复对账；只有仍处于同一项目和剪辑会话时才可更新当前可见产物。启动时发现 `working` conversation 的最新 Agent task 已终态却缺少确定性最终回复时，必须转为 `needs_review`、写入固定恢复消息并等待审阅，不得猜测丢失的模型回答或静默标记完成。

Agent 请求统一经一个封闭、有界的目标驱动技能循环（`agentloop.rs::run_agent_loop`）处理：明确请求由确定性快路径锁定目标；模糊请求的首次主模型响应必须同时声明 `goal`/`isQuestion` 并选择首个技能或直接回答，不再单独调用分类模型。最近一次同作用域待澄清状态会作为结构化上下文提供给首次决策。模型随后在受限观察、编辑和交付技能中按步选择单一技能并执行、回读结果；技能参数放在 JSON 顶层，最多 10 步。`finish`/`no_action`/`done` 只有在目标产物已真实存在时才结束；所有技能继续复用作用域与范围校验、版本和操作审计。交互模型决策总预算为 90 秒，交互请求在 Provider 边界优先于尚未开始的粗视觉请求；粗视觉连续三次失败后熔断 60 秒并保留 queued 任务。显式单命令以及“剪好了吗/完成了吗”等只读状态查询继续走确定性路径，模型回复不能替代真实产物。完成状态必须以当前 task 的最新 storyboard、该 storyboard 的最新 timeline 状态和磁盘实际 preview 文件为产物事实；最近 Agent task 只说明运行/失败/澄清状态，不得用较旧的 task result 否定更新的真实产物。

对话前端统一调用 `submit_conversation_turn`：即时 `respond`/`clarify` 不创建 `agent_tasks`，只有执行型 `run` 创建异步 Agent 任务并发出 `agent-edit-completed`。路由模型在同一首轮响应中选择首个执行技能；后端把该技能作为 run step 1 复用，不能再次调用模型重复选择。schema v7 的 `pending_clarifications` 是待澄清状态的权威来源；存在待澄清时路由必须同轮声明 `keep/resolve`，新澄清 supersede 旧问题，不能再依赖最近消息角色猜测。

用户消息进入 `submit_conversation_turn` 前必须先经项目内 `resolve_conversation_task`：基于 `task_state_snapshots` 的目标、当前子目标、真实 storyboard/时间线/preview 与任务状态，决定继续当前任务、切换已有任务、创建新任务或澄清。目标 task 确定前不得把用户消息写入任一 conversation，也不得执行副作用；低置信度自动归属必须封闭为 `pending_task_routes` 澄清。Task Resolver 只决定任务归属，不选择具体工具；确定目标后签发绑定项目、task、conversation 与请求的一次性 route receipt，`submit_conversation_turn` 和兼容 `execute_agent_edit` 必须在后端消费该凭证后才能运行。待归属请求只在凭证消费时标记 resolved。精确单命令可在没有待路由澄清时确定性使用用户当前显式选择的 task。任务快照不得使用 `conversations.summary` 冒充任务记忆。

进行非简单修改前，必须阅读 `docs/architecture.md`、`docs/decisions.md`、`docs/api.md`、`docs/roadmap.md`、`docs/harness.md` 与 `TASKS.md`。涉及架构决策、公开工具契约或任务状态时，必须更新对应文档。

## 编码标准

- 使用仓库已有的 TypeScript 和 React 19 模式。
- 保持 `src/App.tsx` 聚焦组合；实现真实可复用功能时提取领域类型、UI 组件与服务。
- `verbatimModuleSyntax` 已启用，纯类型导入使用 `import type`。
- 严格 TypeScript 检查中，未使用的局部变量与参数都是错误。
- 优先小而明确的函数和领域名称，避免泛化工具函数。
- 没有明确需要不得新增依赖；新增依赖必须在 `docs/decisions.md` 记录理由。
- 保持既有深色、信息密集的视觉语言和响应式断点，除非有明确设计变更。
- 面向用户的中英文文案统一使用 Agent、storyboard、draft、preview、Jianying draft、local project 等产品词汇。
- 不得将 token、API key、含用户数据的本机路径或媒体内容写入源码、浏览器存储、日志或文档示例。

## 开发流程

修改前：

1. 检查相关实现并阅读上下文文档。
2. 判断变更是否影响产品规则、数据所有权、工具契约或持久化。
3. 开始实质任务前检查并更新 `TASKS.md`。
4. 未知项标记为 `TODO`；不得编造 API 行为、OAuth scope、Jianying JSON 字段或兼容性结论。
5. 触发 `.harness/doc-sync-policy.json` 的改动必须在同一变更集中更新要求的文档和 `docs/changes/` 记录。

修改中：

1. 浏览器原型状态与生产持久化保持分离。
2. 每个副作用必须是具名、可审计的工具调用。
3. 作用域受限请求只能修改目标时间线或 storyboard 区域。
4. 不得为了便利而覆盖或删除用户资料。

修改后：

1. 前端变更运行 `npm run lint` 和 `npm run build`。
2. 运行 `npm run harness:check`；触发架构规则时按 `docs/harness.md` 完成独立 Agent 审查 loop。
3. 更新 `TASKS.md` 和相关 `docs/` 文件。
4. 精确报告实现行为、验证结果和剩余 `TODO`。
5. 恢复绿色基线不得冒充产品验收；高风险路径发生变化后，必须重新打开并执行受影响的端到端验收项。

## 目录职责

- `src/App.tsx`：当前原型 UI 与本地展示状态。
- `src/App.css`、`src/index.css`：应用和全局样式。
- `src/lib/agent-tools.ts`：Agent 工具目标契约。
- `src-tauri/`：Tauri 2 Windows 壳与 Rust 命令边界。
- `docs/`：长期产品和工程上下文。
- `TASKS.md`：当前执行状态。
- `README.md`：入门与本地运行说明。

## 命令

```powershell
npm install
npm run dev
npm run lint
npm run build
npm run tauri:dev
npm run tauri:build
```
