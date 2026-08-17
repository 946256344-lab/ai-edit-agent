# 2026-08-17 提取 assets 子模块（analysis / visual / health）

## 目标

将 `assets.rs`（session 开始时约 3337 行）按职责拆分为四个子模块加一个精简核心，降低单文件认知负担并明确各部分的事实所有者。

上一 session 已完成 `assets/library.rs` 的提取（545+ 行）。本 session 完成其余三个子模块并将核心收敛到约 793 行。

## 变更范围

### 新文件 / 已在前一 session 创建的文件（本 session 完成）

| 文件 | 行数 | 职责 |
|---|---|---|
| `src-tauri/src/assets/analysis.rs` | 1036 | 技术分析 worker、队列入队、drain、恢复、任务中心 |
| `src-tauri/src/assets/visual.rs` | 1032 | 视觉批次分析、批次优先级、`skip_asset_visual_analysis_batch` |
| `src-tauri/src/assets/health.rs` | 339 | 健康扫描命令、`get_asset_health_summary_for_agent` |
| `src-tauri/src/assets/library.rs` | 833 | 库查询、目录投影、分页（前一 session 已提取） |

### 修改文件

- **`src-tauri/src/assets.rs`**（793 行，前为 3337 行）：重写为精简核心，保留导入、重链路、媒体收集和 Agent 检索；声明四个 `pub mod`，并通过 `pub(crate) use` re-export 外部调用方需要的符号。
- **`src-tauri/src/lib.rs`**：将 6 条 Tauri 命令注册路径从 `assets::*` 更新为子模块路径：
  - `assets::get_asset_task_center` → `assets::analysis::get_asset_task_center`
  - `assets::start_asset_health_scan` → `assets::health::start_asset_health_scan`
  - `assets::cancel_asset_health_scan` → `assets::health::cancel_asset_health_scan`
  - `assets::get_asset_health_scan_summary` → `assets::health::get_asset_health_scan_summary`
  - `assets::retry_asset_analysis_batch` → `assets::analysis::retry_asset_analysis_batch`
  - `assets::skip_asset_visual_analysis_batch` → `assets::visual::skip_asset_visual_analysis_batch`

### 顺带修复（health.rs）

1. `get_asset_health_scan_summary` 构建 `AssetHealthScanSummary` 时使用了不存在的字段（`last_checked_at`、`active_scan_status`）和 `i64` 类型而非 `usize`；已按 `models.rs` 中的正确字段（`checked`、`active_task_id`、`active_task_status`）和类型修复。
2. `get_asset_health_summary_for_agent` 中 `statement.query_map(…).collect()` 表达式因借用生存期过短导致编译错误；将收集结果赋值给具名 `rows` 绑定后修复。

## 不变量

- 全部 Tauri 命令名称、参数、返回值不变。
- SQLite schema、工具白名单、Agent 可见 API 不变。
- 原始媒体、项目数据、用户整理结果不变。

## 完成门

- `cargo test`：118 个库测试 + 2 个契约测试通过。
- `cargo fmt --check`：通过。
- `cargo check`：通过（仅保留已记录的 `PartiallyDone` dead-code 警告）。
- `npm run lint`：通过。
- `npm run build`：通过。
- `npm run harness:test/check`：全部通过。
- `git diff --check`：通过。
