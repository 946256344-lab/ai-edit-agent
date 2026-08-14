# 2026-08-07：平铺宽容决策 schema 与有界技能循环

## 变更

- `src-tauri/src/models.rs`：`AgentEditDecision` 改为顶层平铺的宽容 schema，参数直接放在 JSON 最顶层（无嵌套 `params` 包装），`#[serde(flatten)]` 吸收多余键，`#[serde(other)]` 的 `Unknown` 变体保证任何未识别工具都不解析失败；`StoryboardVersion` 增加 `Clone`。
- `src-tauri/src/agent.rs`：快速路径只处理确定的工具；新增 `escalated` 判定，遇到 `Unknown` 时以 `agent_loop` 为工具名升级到技能循环。
- `src-tauri/src/agentloop.rs`（新增）：有界技能循环 `run_agent_loop`，模型按步选择单一技能并执行、回读结果，直到 `finish`/`ask_user`/步数上限（6）。观察技能：`list_assets`/`get_storyboard`/`get_timeline`；编辑/交付技能：`generate_storyboard`/`create_timeline_draft`/`replace_clips`/`change_clip_duration`/`reorder_clips`/`render_preview`/`create_jianying_draft`，全部复用既有作用域与范围校验、写操作审计。
- `src-tauri/src/lib.rs`：注册 `mod agentloop;`。

## 同步文档

本变更同步了以下长期文档：`AGENTS.md`、`TASKS.md`、`docs/architecture.md`、`docs/api.md`、`docs/decisions.md`。

## 验证

- `cargo build --lib` 编译通过；`cargo test --lib` 18 通过、3 项依赖认证 Provider 的集成测试按设计跳过。
- 待真实 API 响应验证升级路径（未识别请求升级到技能循环）。