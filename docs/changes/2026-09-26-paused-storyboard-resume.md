# 暂停的生成能按用户回答续跑

## 现象

用户要 30 秒视频，配音只有 21 秒，Agent 暂停问保留哪个时长。用户回「1」（保留 21 秒）后，Agent 没有调用任何工具，直接回复「Done! 视频已生成、剪映草稿已创建」；该轮 `agent_tasks` 记为 `completed`，没有任何步骤和产物。

## 根因

跨轮历史只带文字（`agentloop/prompt.rs`），上一轮 `generate_storyboard` 的参数（扩写后的旁白稿）和暂停结果、恢复提示下一轮都看不到；暂停时稿子也没有存成任务 brief（只在成功后写入）。模型拿不到可用的稿子，也不知道有一次待续的生成，于是空口声称完成。

## 触发范围

- `src-tauri/src/storyboard.rs`：`generate_storyboard_for_agent` 在 `storyboard_needs_user_decision` 时把本次 brief 存为任务 brief（与成功路径共用 `persist_task_brief`）。
- `src-tauri/src/agentloop/snapshot.rs`：最近终态为 `needs_clarification` 时附 `暂停于=工具名(错误码)`，只放行内部标识字符集。
- `src-tauri/src/agentloop/native.rs`：系统提示新增 ACROSS TURNS：快照为准；暂停后用户的回答就是那次决定，`brief=null` 沿用已存稿、保留口播时长传 `requestedDurationMs=null`；本轮没有函数返回、快照也没有的产物不得声称已创建。
- `src-tauri/src/agentloop/skills.rs`：时长冲突恢复提示说明稿子已存、保留口播时长时怎么调用。

## 改动

公开命令签名不变。快照多一个可选字段；任务 brief 的写入时机从「仅成功」扩到「成功或暂停等用户决定」。

## 同步文档

`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`TASKS.md`。

## 验证

`cargo check` 通过；`cargo test --lib agentloop` 114 条通过，新增回归 `snapshot_names_the_tool_a_paused_run_stopped_on`。待桌面重跑「30 秒、稿子偏短 → 选保留口播时长」确认一轮内真正生成。

## 决策

`docs/decisions.md` 第 11 条补充暂停续跑规则。
