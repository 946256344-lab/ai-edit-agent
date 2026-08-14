# 2026-08-14 Timeline 与 preview 写链路恢复

## 目标与边界

在真实 Tauri 桌面、既有 local project 和当前剪辑任务中，通过显式 Agent 请求新增一个内部 timeline version 和对应 local preview，并验证作用域、版本、旧产物保留、实际播放、完成对账和重启恢复。未创建 Jianying draft、未执行最终导出、未删除或重新分析素材。

## 真实桌面结果

- 基线为同一 storyboard 的 timeline v4/v3，v4 已有磁盘 preview。
- “创建内部时间线”只执行并审计 `create_timeline_draft`：timeline 数量 2→3，最新版本 v4→v5，8 个片段；项目、task、conversation 和 storyboard ID 未变。
- “生成预览”只执行并审计 `render_preview`：preview 绑定 v5；v4 timeline 和 preview 文件仍存在。新 preview 为 540×960、29.47 秒，播放器 readyState=4，播放 1.8 秒后进度正常前进。
- 没有 Jianying registration、最终导出、删除、素材分析或额外 timeline 版本。

## 发现与修复

后端完成 preview、持久化 task/消息并恢复 conversation 为 ready 后，当前界面仍停在旧消息数且 `preview=null`。React 运行态检查确认 pending ref 的 scope 正确，但 `taskId` 为 undefined；数据库 task ID 正常。

原因是 `ConversationTurnResult` enum 的 `rename_all = "camelCase"` 只转换 variant 名，`Run { agent_task_id }` 仍序列化为 snake_case。前端公开类型读取 `agentTaskId`，因此无法把事件或终态任务与 pending 对上。前端同时存在两个放大器：任务列表暂未出现 pending ID 时会直接放弃；轮询 interval 生命周期受 active/terminal 状态影响。

修复内容：

- Rust 明确把 `Run.agent_task_id` 序列化为 `agentTaskId`，并新增精确 JSON 回归测试。
- 前端拒绝空 task ID，不再建立不可恢复的 pending ref。
- pending 不因一次列表快照缺失而清空；轮询由 composer 所有权或持久化 working conversation 保持，task status 变化不再控制 interval。
- 没有内存 pending 时，持久化 working conversation 允许最新同作用域 terminal task 触发一次恢复对账，覆盖首次任务快照已经 terminal 的窗口。

## 修复后验收

- Tauri 因 Rust 改动真实重启后，v5 timeline、preview 和 21 条历史消息完整恢复。
- 最终代码下再次刷新 WebView，23 条持久化消息与 23 条可见消息一致，v5 preview 继续恢复且可播放。
- 新只读 Agent run 的前端 pending ID 与数据库 task ID 完全一致。
- 该任务从 running 进入 completed 后，无刷新情况下后端与可见消息同步由 22→23，composer 自动恢复为“发送”，无同步错误提示。
- 只读任务步骤全部完成、操作日志为 0、确定性回复只有 1 条；timeline 仍为 v5，preview 与旧 v4 文件均存在且继续可播放。

## 验证命令

- `cargo fmt --all -- --check`
- `cargo test`
- `npm run lint`
- `npm run build`
- `npm run harness:check`
- `git diff --check`

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/roadmap.md`
