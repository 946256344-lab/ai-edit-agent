# 长期文档历史补充

原实现与验证记录按当时文档保留，不代表当前契约。

## 原长期文档补充：2026-08-19：独立维护记录

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

2026-08-19：为诊断用户报告的应用启动卡顿问题，在 `projects.rs::initialize_local_store`、`assets/analysis.rs::resume_incomplete_analysis`、`assets/visual.rs::recover_interrupted_visual_batches` 和 `backfill_queued_visual_batches` 添加性能诊断日志（26 处 [PERF] 标记点），测量数据库连接、清理中断任务、恢复分析批次、启动后台 worker 等关键步骤的实际耗时。所有日志使用 `log::info!` 级别，使用 `std::time::Instant` 计时。只添加诊断日志，不改变执行逻辑、公开命令签名或 SQLite schema。

## 原长期文档补充：2026-08-19：独立维护记录

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

2026-08-19：优化 `projects.rs::recover_missing_agent_completion_messages` 查询性能。用窗口函数（`ROW_NUMBER() OVER PARTITION BY`）+ CTE 替代相关子查询，将查询复杂度从 O(N²) 降至 O(N log N)。原查询在有几百条任务记录时耗时 ~300ms（占启动总时间 80%），优化后预期降至 <20ms。查询语义完全等价，不影响公开命令或 SQLite schema。
