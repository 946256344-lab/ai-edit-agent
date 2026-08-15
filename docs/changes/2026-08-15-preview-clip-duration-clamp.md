# preview 片段渲染时长收敛到源范围

## 背景

`preview.rs::render_timeline_clip` 一直用 `timeline_end_ms - timeline_start_ms` 作为 FFmpeg `-t` 参数，而不是 `min(source_range, timeline_slot)`。当素材在 seek 点后剩余内容短于时间线槽位时，FFmpeg 会静默填充黑帧或静帧；受影响的镜头包括 v1–v6 全部版本中源范围短于槽位的片段。

## 实现

- `render_timeline_clip`：`-t` 改为 `min(source_end_ms - source_start_ms, timeline_end_ms - timeline_start_ms)`，图片循环分支同样受益。
- 将原 `#[cfg(test)] mod tests { ... }` 提取为独立文件 `src-tauri/src/preview_tests.rs`，通过 `#[path]` 挂载，使 `preview.rs` 的行数/字符数降回旧预算以内（1015 行 → 608 行）。
- 新增回归测试 `render_timeline_clip_clamps_duration_to_source_range`：源范围 500 ms、时间线槽位 3000 ms，验证渲染时长被截止于源范围。
- 更新 `.harness/architecture-budgets.json`：`preview.rs` 预算收紧至 608 行，新增 `preview_tests.rs` 预算（489 行）。

## 不变边界

不修改公开 Tauri 命令、SQLite schema、Agent 工具、项目数据、素材分析证据、storyboard、现有 timeline 版本或 Jianying draft。现有 preview 文件不重新渲染。

## 同步文档

- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`

## 验证

- `cargo test --lib preview`：12 项全部通过，含新增回归测试。
- 架构预算检查通过：`preview.rs` 预算收紧，`preview_tests.rs` 新增独立预算。
