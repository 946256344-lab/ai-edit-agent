# 素材列表路径探测导致 UI Hang 修复

2026-08-11

## 问题

素材库每 1.5 秒调用 `list_assets`。该命令为返回 `sourceAvailable`，对项目内每个 `source_reference` 同步执行 `Path::is_file()`。在实际 1008 素材项目中，891 条源路径已失联，整次调用实测耗时 89.235 秒、225 次单项检查超过 100ms，导致 Tauri 窗口未响应并触发 Windows `Application Hang`（Event ID 1002）。

## 决策

- 从 `Asset` / `StoredAsset` 和 Agent `list_assets` 摘要移除 `sourceAvailable` 字段。
- `list_assets` 仅返回持久化分析状态，不再在 UI 轮询路径上探测文件系统。
- 媒体分析、storyboard、preview 和 Jianying draft 保留实际使用前的源文件校验；源文件缺失仍不能进入产物。

## 验证

- 变更前实测：1008 次 `is_file` 89.235 秒。
- `cargo test`：46 单元 + 2 集成测试通过；`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 通过。
- 新 NSIS 安装版覆盖后，使用同一 1008 素材库连续运行 60 秒，窗口保持 `Responding=True`。
- 当前生产安装包没有捆绑 FFprobe，日志中的 `FFprobe could not read this media file` 是独立的已知运行时依赖限制，不会再阻塞 UI。
