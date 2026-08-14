# 孤立的待分析素材恢复

2026-08-07

## 问题

右下角“正在分析媒体”任务提示可能永久停住：当一批素材在导入时已写入 `assets` 表（`analysis_status = 'queued'`），但对应的 `analyze_asset` 任务行从未持久化（例如导入过程中被中断、或历史版本未排队），则这些素材没有任何任务行可被恢复。

- `resume_incomplete_analysis` 只从 `agent_tasks` 表挑选状态为 `queued`/`running` 的 `analyze_asset` 行并恢复；对“有素材、无任务”的孤立 `queued`/`analyzing` 素材没有任何路径可恢复。
- 前端把任何非 `ready`/`failed` 的素材状态映射为 `analyzing`（`App.tsx` 的 `toAsset`），因此这些孤立素材会永久显示为“正在分析媒体”。

已在活动项目 `dc81c11e` 观察到 54 个自 2026-07-31 起停留在 `queued`、无任何 `analyze_asset` 任务的孤立素材。

## 决策

扩展 `assets.rs::resume_incomplete_analysis` 为真正的启动期对账：保留原有“恢复待完成任务并取消同一素材重复任务”的行为，并新增孤立方扫描——凡是 `analysis_status IN ('queued','analyzing')` 且不存在任何（`queued`/`running`/`completed`/`failed`/`cancelled`）`analyze_asset` 任务的资产，都会在启动时补建并排队一条 `analyze_asset` 任务。

- 用“该素材是否已有任意 `analyze_asset` 行”守护，绝不重复排队已分析或已有任务的素材。
- 不新增 Tauri 命令、不改变命令契约、不触碰 Provider 或安全边界；仅扩充既有的一次性启动恢复逻辑。
- 仍以单进程一次（`RECOVERY_STARTED` 原子）执行，且仅在此处生效，不进入桌面 UI。

## 验证

- `cargo build --lib` 通过；`cargo test --lib` 19 项通过、0 失败。
- 桌面端需重新启动（触发 `initialize_local_store`）后，这些孤立素材才会被重新排队并真正完成分析；启动恢复由 `projects.rs` 的 `initialize_local_store` 每进程调用一次。