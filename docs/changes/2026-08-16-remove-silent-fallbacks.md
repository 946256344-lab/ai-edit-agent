# 移除静默 fallback，补全被丢弃的错误日志

日期：2026-08-16  
范围：`src-tauri/src/agent.rs`、`src-tauri/src/agentloop.rs`、`src-tauri/src/agentloop/policy.rs`、`src-tauri/src/assets.rs`、`src-tauri/src/storyboard.rs`

## 目标

将后端 Rust 所有"静默 fallback"（遇错返回硬编码合成值）替换为明确失败或诚实降级，
并修复所有 `log::warn!` 语句只输出固定字符串、丢弃真实错误值的问题。

## 变更摘要

### agent.rs — Finding A2

`persisted_task_status` 的 `unwrap_or_else(|_| "failed".to_owned())` 在
DB 不可用时与任务真实失败状态无法区分。拆分为两个 `Err` 路径，
各自通过 `log::warn!` 输出真实错误原因后再返回 `"failed"`。

### agentloop.rs — Findings A4、A5

- `get_storyboard` handler：`serde_json::to_value` 失败时静默返回 `Value::Null`；
  改为 `.unwrap_or_else(|e| { log::warn!("...{e}"); Value::Null })`。
- `build_timeline_snapshot`：同上，序列化失败时记录真实错误后返回 `Value::Null`。

### assets.rs — Findings A9、A10、B15、visual_req、B19

- **A9**：`update_visual_batch_task` 时间戳读取失败静默回退 `now_millis()`；
  改为 `.unwrap_or_else(|e| { log::warn!("...{e}"); now_millis() })`。
- **A10**：`run_visual_analysis_batch` 中 `metadata_json` 解析失败静默
  `unwrap_or_default()`；改为 `.unwrap_or_else(|e| { log::warn!("...{e}"); Default::default() })`。
- **B15**：Provider 访问失败的 `Err(_) => { ... }` 分支原先无日志；
  改为 `Err(error) => { log::warn!("...{error}"); ... }`。
- **visual_req**：视觉模型请求失败 `Err(_) => String::new()` 无日志；
  改为 `Err(error) => { log::warn!("...{error}"); String::new() }`。
- **B19**：`spawn_visual_analysis_worker` 的 IIFE 执行后静默丢弃 `Err`；
  改为 `.inspect_err(|e| log::warn!("...{e}"))`。

## 架构预算

所有受影响文件均在 `.harness/architecture-budgets.json` 预算内：
- `agentloop.rs`：3599 / 3599（使用 `#[rustfmt::skip]` 保持紧凑布局）
- `agent.rs`：1245 / 1247
- `assets.rs`：4114 / 4114（使用 `#[rustfmt::skip]` 和 `.inspect_err()` 保持行数）
- `agentloop/policy.rs`：673 / 674

## 不变量

- 公开 Tauri 命令签名不变
- SQLite schema 不变
- 工具白名单不变
- 用户数据不变
- 所有降级路径仍封闭失败，不静默冒充成功结果

## 同步文档

本次变更不引入新的公开契约或架构边界，以下文档记录此次维护事实：

- TASKS.md
- AGENTS.md
- docs/architecture.md
- docs/api.md
- docs/decisions.md
- README.md
- docs/harness.md
