# 有界本地媒体分析

2026-08-12

## 观察

本机项目有 767 条本地分析任务排队、1 条运行中，最早活跃任务已约 19 小时。单一 FFmpeg 进程占住唯一技术 worker，导致后续素材无法推进。

## 决策

- 技术分析最多并行 2 个 worker；视觉批量 worker 保持单一。
- FFprobe、缩略图 FFmpeg、场景扫描 FFmpeg、回退抽帧 FFmpeg、Tesseract 分别限制为 20、30、45、20、20 秒。
- 超时会在 Windows 通过隐藏的 `taskkill /T /F` 请求终止进程树，并只在短清理窗口内回收直接子进程；即使系统拒绝终止请求，调用也不会无限等待。受影响素材标记失败，队列继续。
- 启动时将中断的本地 `running` 任务重排为 `queued`。

## 同步文档

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `TASKS.md`

## 验证

- `cargo fmt --check`、`cargo test --lib`（68 通过）、`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 通过。
- 独立审查发现并修复了超时清理、重定位的旧 worker 回写、恢复守卫以及非 FFprobe 阶段超时状态不一致。
