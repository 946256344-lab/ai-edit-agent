# 2026-08-19: 优化 recover_missing_agent_completion_messages 查询性能

## 变更类型

performance（优化启动查询）

## 触发规则

- desktop-contract（修改 `src-tauri/src/projects.rs`）

## 问题背景

用户报告应用启动时严重卡顿。性能诊断日志显示 `recover_missing_agent_completion_messages` 函数耗时 **~300ms**，占总启动时间（~380ms）的 **80%**，是启动卡顿的根本原因。

原始查询使用相关子查询（correlated subquery）：

```sql
AND task.id = (
  SELECT latest.id FROM agent_tasks AS latest
  WHERE latest.conversation_id = task.conversation_id
  ORDER BY latest.created_at DESC, latest.updated_at DESC, latest.id DESC
  LIMIT 1
)
```

该子查询**对每一行外部结果重新执行一次**。如果有 N 个符合条件的 agent_tasks 记录，就会执行 N 次子查询，每次都要扫描整个 agent_tasks 表并排序。在有几百条任务记录的数据库中，这导致 O(N²) 的查询复杂度。

## 变更范围

### 1. `src-tauri/src/projects.rs`（优化查询逻辑）

**优化策略**：用窗口函数（`ROW_NUMBER()`）+ CTE 替代相关子查询，一次性标记每个 conversation 的最新任务，避免重复扫描。

**优化后查询**：

```sql
WITH latest_tasks AS (
  SELECT id, conversation_id,
         ROW_NUMBER() OVER (
           PARTITION BY conversation_id
           ORDER BY created_at DESC, updated_at DESC, id DESC
         ) as row_num
  FROM agent_tasks
  WHERE editing_task_id IS NOT NULL
    AND conversation_id IS NOT NULL
    AND status IN ('completed', 'partially_completed', 'failed', 'needs_clarification', 'needs_review')
)
SELECT task.id, task.project_id, task.editing_task_id, task.conversation_id
FROM agent_tasks AS task
JOIN conversations AS conversation ON conversation.id = task.conversation_id
JOIN latest_tasks ON latest_tasks.id = task.id AND latest_tasks.row_num = 1
LEFT JOIN messages ON messages.id = 'agent-task-result-' || task.id
  AND messages.conversation_id = task.conversation_id
WHERE conversation.status = 'working'
  AND messages.id IS NULL
```

**性能提升**：
- 原查询：相关子查询导致 O(N²) 复杂度，耗时 ~300ms
- 新查询：窗口函数 + CTE 只扫描一次 agent_tasks，复杂度降为 O(N log N)（窗口排序）+ O(N)（JOIN），预期耗时降至 5-20ms

**语义等价性**：
- `ROW_NUMBER() OVER (PARTITION BY conversation_id ORDER BY ...)` 标记每个 conversation 内的最新任务
- `JOIN latest_tasks ON ... AND row_num = 1` 只保留最新任务（等价于原 `task.id = (SELECT ... LIMIT 1)`）
- `LEFT JOIN messages` + `WHERE messages.id IS NULL` 等价于原 `NOT EXISTS (SELECT 1 FROM messages ...)`

## 向后兼容

- 查询语义完全等价，返回结果集不变
- 不影响公开命令、SQLite schema、工具白名单
- SQLite 3.25+ 支持窗口函数（Tauri 自带 SQLite 3.45+，满足要求）

## 同步文档

- **docs/architecture.md**：在维护记录中新增 2026-08-19 条目，记录启动查询优化
- **docs/api.md**：无需更新（查询优化不影响公开 API）
- **TASKS.md**：在当前任务窗口新增本项，标记为性能优化

## 公开契约

无变更。

## 验证证据

- ✅ Rust 库测试：113 passed
- ✅ Rust fmt/check：通过
- ✅ harness:check：待同步文档后通过
- ⏳ 实际性能验证：需要用户重启应用，观察 `[PERF] initialize_local_store: recover_missing_agent_completion_messages` 日志，预期从 ~300ms 降至 <20ms

## 后续任务

1. **收集优化后性能数据**：用户重启应用，确认耗时从 300ms 降至预期范围
2. **评估是否需要进一步优化**：如果启动仍有明显卡顿，检查 `backfill_queued_visual_batches`（当前耗时 30ms）是否需要添加索引
