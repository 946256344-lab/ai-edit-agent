# 批量视觉素材识别

2026-08-11

## 决策

- `analyze_asset` 完成本地技术分析后即标记素材 `ready`。
- 独立 `analyze_asset_visual_batch` 在单一后台 worker 中每批处理最多 6 条素材，每条发送一张低分辨率中间代表帧及素材 ID/源时间标签。
- 批量结果严格校验素材 ID 和源时间；任务 payload 不含路径或媒体内容，结果不含模型原文、OCR 正文或图片，仅保存计数、安全错误码与总 `durationMs`。
- 视觉状态独立于技术状态。Provider 或响应失败不回退技术 `ready`，没有无限重试。
- 启动恢复会重排有效的中断视觉批次、把无效 batch payload 的关联素材封闭为失败，并为旧技术 `ready` 素材补建缺失视觉批次，避免素材永久停在视觉 `running` 或 `queued`。
- storyboard 仅使用视觉状态 `ready` 且保存了视觉证据的素材，避免以文件名、OCR 或猜测替代画面证据。
- `generate_storyboard` 在任一源文件可用的图片/视频仍处于视觉 `queued` 或 `running` 时拒绝生成，避免先完成的小部分素材被过度使用。

## 同步文档

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `TASKS.md`

## 验证

- `cargo fmt --check`、`cargo test --lib`（55 通过）、`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 通过。
- 两轮独立审查分别发现并修复了中断批次恢复，以及旧技术 `ready` 素材的视觉批次回填。
- 真实 Provider 桌面批量响应仍待人工验证。
