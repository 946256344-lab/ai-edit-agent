# 批量素材导入卡死与数据库锁竞争修复

2026-08-10

## 问题

一次递归导入 891 个媒体文件后，桌面应用启动即卡死、反复出现 `database is locked`：

- 导入后自动分析全量排队，但分析 worker 单线程串行，且每个视频的阈值场景检测会以 `-loglevel info` 整片解码，单个 iPhone MOV 就能打满多核数分钟；启动时 `resume_incomplete_analysis` 会把全部 `queued` 任务一次性恢复并立即开跑，CPU 持续饱和导致 UI 失去响应。
- `db.rs::open_connection` 未设置 `busy_timeout`，任何并发写一撞锁就立刻 `SQLITE_BUSY`：部分素材永久 `failed`（`database is locked`），剪映注册 worker 每 2 秒刷一条失败日志。

## 决策

1. **DB 并发（`db.rs`）**：`open_connection` 设置 5 秒 `busy_timeout`，启用 `journal_mode = WAL` 与 `synchronous = NORMAL`。
2. **场景检测降载（`assets.rs`）**：`generate_video_keyframes` 的场景扫描加 `-t 90` 限长（`SCENE_SCAN_CAP_SECONDS = 90.0`），只解码视频前 90 秒；关键帧 `pts_time` 为源时间戳绝对值，回退关键帧逻辑不变。
3. **有界批次（`assets.rs`）**：新增全局 `ANALYSIS_WORKER_ACTIVE` 守卫，任何时刻最多一个分析 worker；`resume_incomplete_analysis` 启动只截取前 `STARTUP_ANALYSIS_BATCH = 4` 条任务；新增 `drain_pending_analysis`，在 `list_assets` 轮询时按 `DRAIN_ANALYSIS_BATCH = 4` 渐进排空当前项目队列，前端无需改动。
4. **剪映 worker 退避（`jianying.rs`）**：`process_pending_jianying_registrations` 返回是否处理过任务；无待办时空转 10 秒、有待办时隔 2 秒重试，不再每 2 秒盲目碰 DB。

## 验证

- `cargo build --lib` 通过；`cargo test` 48 项通过（46 单元 + 2 集成），exit 0；`npm run lint` 0 警告；`npm run harness:check` 通过（desktop-contract、provider-security、desktop-runtime）。
- 同步文档：`docs/architecture.md`、`docs/api.md`、`TASKS.md`。
- 遗留：`tests/fixtures/agent_tool_contracts.v1.json` 缺 `request_asset_analysis`（白名单 11 个 vs fixture 10 个），与该修复无关，另行处理。