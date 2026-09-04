# 2026-09-04：去掉 Agent 关键词判定行动

## 问题

英文 brief 含 `only`（如 “Capacity is only one measure”）会被 `RequestToolPolicy` 误判为只读，拒绝 `generate_storyboard`，模型再要求用户「明确授权」。中文「只」同样过宽。这与「意图由模型判断、Rust 守真实边界」不一致。

## 变更

- 删除 `RequestToolPolicy.read_only` 及 `只`/`only` 子串匹配；移除执行门 `user_restricted_tool` 关键词路径。
- `request_requires_project_observation` 恒为 `false`：不再用「当前/已有/storyboard…」词表强制观察；权威快照已覆盖高层事实。
- 完整工具目录默认开放；白名单、作用域、参数与领域校验保留。
- 回归覆盖含 `only` 的英文 brief 与「只查看」类措辞不再关闭写工具。

## 不在本变更

- Storyboard 短 brief 时长词表（`分钟`/`longer` 等）仍是领域结构校验，不是 Agent 工具授权。
- Task Resolver 提示中的「新的视频」等例子仍是模型引导，不是 Rust 关键词门。

## 同步文档

- `docs/api.md`、`docs/architecture.md`
- 本变更记录

## 验证

- [x] `cargo test --manifest-path src-tauri/Cargo.toml agentloop::native::tests`（57 passed）
