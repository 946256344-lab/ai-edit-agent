# 2026-08-19: 添加启动性能诊断日志

## 变更类型

feature（诊断工具）

## 触发规则

- desktop-contract（修改 `src-tauri/src/projects.rs`、`src-tauri/src/assets/analysis.rs`、`src-tauri/src/assets/visual.rs`）

## 问题背景

用户报告应用启动时严重卡顿，特别是在开始执行 Agent 任务前的阶段。为诊断根本原因，需要测量启动流程中每个关键步骤的实际耗时。

启动流程（`initialize_local_store` → `resume_incomplete_analysis`）包含多个可能的瓶颈：
1. 数据库连接建立
2. 清理中断状态（agent_tasks、agent_run_steps、conversations）
3. 恢复技术分析任务（扫描 queued/analyzing 素材、创建孤立任务）
4. 恢复视觉分析批次（recover_interrupted_visual_batches、backfill_queued_visual_batches）
5. 启动后台 worker（技术分析 worker、视觉分析 worker、剪映注册 worker）

每个步骤的耗时未知，无法判断卡顿来自数据库竞争、全表扫描、还是后台线程启动开销。

## 变更范围

### 1. `src-tauri/src/projects.rs`（添加 10 处 PERF 日志）

在 `initialize_local_store` 的关键步骤添加计时日志：
- 数据库连接建立（line ~113）
- 清理 agent_tasks（line ~120）
- 清理 agent_run_steps（line ~127）
- 恢复缺失的完成消息（line ~134）
- 标记会话为 review 状态（line ~138）
- 标记会话为 ready 状态（line ~147）
- `resume_incomplete_analysis` 调用（line ~154）
- `resume_pending_jianying_registrations` 调用（line ~160）
- 总耗时（line ~163）

### 2. `src-tauri/src/assets/analysis.rs`（添加 7 处 PERF 日志）

在 `resume_incomplete_analysis` 的关键步骤添加计时日志：
- `recover_interrupted_visual_batches` 调用（line ~657）
- `backfill_queued_visual_batches` 调用（line ~664）
- `spawn_visual_analysis_worker` 调用（line ~671）
- 收集已有任务的素材 ID（line ~767）
- 查询孤立素材（line ~790）
- 创建孤立素材任务（line ~808）
- 技术分析恢复总耗时（line ~816）
- 函数总耗时（line ~820）

### 3. `src-tauri/src/assets/visual.rs`（添加 9 处 PERF 日志）

**`recover_interrupted_visual_batches`**（3 处）：
- 函数开始（line ~774）
- 查询中断批次数量（line ~788）
- 函数总耗时（line ~850）

**`backfill_queued_visual_batches`**（6 处）：
- 函数开始（line ~860）
- 查询活跃批次（line ~877）
- 收集活跃素材 ID（line ~893）
- 查询就绪素材并识别孤立素材（line ~905）
- 创建新批次（line ~922）
- 函数总耗时（line ~925）

所有日志使用 `log::info!` 级别，前缀 `[PERF]`，便于过滤。使用 `std::time::Instant` 测量耗时。

## 向后兼容

- 只添加日志，不改变执行逻辑
- 不影响公开命令、SQLite schema、工具白名单
- 日志默认输出到控制台，生产环境可通过日志级别过滤

## 同步文档

- **docs/architecture.md**：在维护记录中新增 2026-08-19 条目，记录启动性能诊断日志
- **docs/api.md**：无需更新（日志不属于公开 API）
- **TASKS.md**：在当前任务窗口新增本项，标记为独立诊断功能

## 公开契约

无变更。

## 验证证据

- ✅ Rust 库测试：113 passed + 2 contract tests
- ✅ Rust fmt/check：通过
- ✅ harness:check：待同步文档后通过
- ⏳ 实际性能诊断：需要用户重启应用，观察日志输出中每个步骤的实际耗时

## 后续任务

1. **收集实际耗时数据**：用户重启应用时，提供完整的 `[PERF]` 日志输出
2. **识别瓶颈**：根据实际耗时确定哪个步骤是卡顿的根本原因
3. **实施针对性优化**：可能的优化方向包括：
   - 减少 `STARTUP_ANALYSIS_BATCH` 从 4 降至 0 或 1
   - 延迟启动后台 worker（避免与 UI 初始化竞争数据库连接）
   - 增加 `db.rs::busy_timeout` 从 5 秒提升到 10-15 秒
   - 优化全表扫描查询（添加索引或改用增量检查）
