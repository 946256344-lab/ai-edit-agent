# 2026-08-05 后台媒体任务

## 触发范围

- `src-tauri/src/store.rs`：外部 Windows 子进程创建行为。
- `src/App.tsx`：活动媒体分析状态的桌面展示。

## 改动

为 `store.rs` 的外部命令创建统一应用 Windows 无控制台窗口标志，覆盖 FFmpeg、FFprobe、Tesseract、Python 适配器和 `tasklist`。前端复用 `list_assets` 已返回的分析状态，在右下角显示活动任务数量和最多三个素材显示名。

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`

## 验证

- `npm run lint`
- `npm run build`
- `npm run harness:check`

## 决策

新增 ADR-024；尚待在 Windows 桌面应用中完成手工无闪窗验证。
