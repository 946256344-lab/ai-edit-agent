# fix: render_preview 多版本时间线场景下错误返回"受限操作未完成"

**日期**：2026-08-16  
**类型**：bugfix  
**范围**：`src-tauri/src/timeline.rs`、`src-tauri/src/agentloop.rs`

## 问题

用户点击"生成预览"时，若当前剪辑任务下存在多个时间线版本（version_number > 1），
后端日志显示 "Completed local preview render"，但前端始终收到  
"这次受限操作没有完成…" 的失败回复。

## 根因

`timeline.rs::select_timeline_candidate` 在无显式 ID 时的逻辑为：

```rust
// 旧逻辑
(timelines.len() == 1).then(|| timelines[0].clone())
```

仅当候选列表只有一条时才返回结果，多条时返回 `None`。  
`agentloop.rs::select_timeline_for_tool` 把 `None` 转为  
`Err("Agent must select a timeline…")`，  
`finalize_agent_task` 将 `Err` 持久化为失败并向前端回复失败文案。

候选列表由 `timeline_candidates_for_storyboard` 按 `version_number DESC` 排序，  
第一条始终是最新版本。

## 修复

```rust
// 新逻辑
timelines.first().cloned()
```

无 ID 时取候选列表第一条（最新版本），保持精确 ID 匹配路径不变。

## 回归测试

`agentloop.rs::delivery_tools_require_a_scoped_timeline_instead_of_creating_one`  
新增断言：多版本候选时取 `version_number` 最大的版本。

## 验证

- 129 个 Rust 库测试 + 2 个契约测试 ✓  
- `cargo fmt --check` ✓  
- `cargo check` ✓  
- `npm run harness:check` ✓（架构预算 27 文件通过，ratchet 未放宽）

## 公开契约影响

无。`select_timeline_candidate` 是内部函数，不修改 Tauri 命令签名、  
SQLite schema、工具白名单或用户数据。
