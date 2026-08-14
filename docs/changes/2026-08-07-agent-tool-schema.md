# 2026-08-07：严格 per-tool 决策 schema 与时间线编辑工具集

## 背景

近期“生成一条新的视频”连续返回“Agent 本次没有形成可执行的剪辑决定”。根因是决策层使用扁平结构解析模型 JSON：未知工具名或多余字段无法在解析期被识别，反序列化失败被统一降级为 `agent_decision_unavailable`，真实决策错误被隐藏。同时工具集缺少批量替换、改时长、排序与澄清反问的表达能力。

## 变更

- `models.rs`：`AgentEditDecision` 由扁平结构改为内部以 `tool` 加标签的关闭枚举，每个变体携带独立的 `deny_unknown_fields` `params` 对象，共享字段（`reason`、`reply`、`taskBrief`）经 `AgentEditCommon` 扁平化；新增 `name()`、`common()`、`decision_timeline_id()` 访问器。顶层 JSON 形态为 `{ tool, reason, reply, taskBrief?, params }`。
- 工具集合：保留 `generate_storyboard`、`create_timeline_draft`、`render_preview`、`create_jianying_draft`、`no_action`；新增 `request_clarification`（不产任何产物，仅返回澄清问题）；将单一 `replace_timeline_clip` 拆为 `replace_clips`、`change_clip_duration`、`reorder_clips`。
- `timeline.rs`：删除 `create_replaced_timeline_version`，新增作用域函数 `replace_clips`（批量替换，保持每个镜头时间线时长）、`change_clip_duration`（在已验证源范围内重定时长/起止）、`reorder_clips`（`order` 必须是全部既有 `shot_index` 的完整排列），新增 `ClipReplacement`/`ClipAdjustment` 与公共 helper，并为每个函数补充单元测试。
- `agent.rs`：重写决策 prompt（9 工具逐项参数说明）；决策流水线改为 `match` 枚举变体；新增 `retarget_decision_for_draft` 沿用“创建草稿/内部时间线”精确指令归一化；`record_agent_operation` 跳过 `no_action`、`request_clarification` 及三个时间线变更工具（三者各自写操作日志）。真实解析错误仍写入 `log::warn!` 与 `agent_tasks`，UI 保持固定安全文案。
- `src/lib/agent-tools.ts`：`AgentToolName` 更新为超集，新增 `replace_clips`、`change_clip_duration`、`reorder_clips`、`request_clarification`，保留历史 `replace_timeline_clip` 别名。

## 同步文档

- AGENTS.md
- TASKS.md
- docs/architecture.md
- docs/api.md
- docs/decisions.md

## 验证

- `cargo build --lib` 编译通过。
- `cargo test --lib` 通过 15 项（含三个新的时间线变更测试）；3 项依赖认证实验性 Provider 的集成测试按设计跳过。
- `npm run lint`（0 警告 0 错误）、`npm run build`（tsc + vite）通过。
- `npm run harness:check` 通过（触发 desktop-contract、provider-security）。

## 遗留 TODO

- 已安装的桌面 exe 仍是旧 app（4 工具 prompt），需要重新 `tauri build` 并安装后新工具与严格 schema 才在桌面生效。
- 3 项实验性 Provider 集成测试需在桌面以真实素材与认证 Provider 手工验证。
- 可恢复的本地 Agent 运行时（队列、暂停/恢复）仍未实现。
