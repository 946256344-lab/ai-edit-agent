# 更快的首次素材分析

2026-08-11

## 问题

首次视频分析在单 worker 中顺序执行场景扫描、OCR 和远端视觉建议。此前场景扫描最多解码前 90 秒、生成 8 张关键帧，并最多为 3 张帧请求视觉分析，导致刚导入素材长时间保持不可用。

## 决策

- 场景扫描上限降为前 30 秒，最多生成 4 张关键帧。
- 视频 OCR 与远端视觉建议各只处理前 2 张代表帧。
- `ready` 门保持不变：技术元数据、有限关键帧、OCR 和视觉分析步骤完成后才可供 storyboard 使用。更深度的按需采样仍为 TODO，不新增绕过工具边界的后台入口。

## 同步文档

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `TASKS.md`

## 验证

- `cargo fmt --check`、`cargo test --lib`（47 通过）、`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 通过。
- 完整 `cargo test` 的 `agent_contract_assets` 集成测试失败：已有 fixture 未包含当前白名单已存在的 `replace_text_tracks`，与本次素材分析采样变更无关，未修改该并行工作区改动。
- 独立文档审查发现旧数据流与 ADR-014/ADR-017 仍声明 8 张/3 张采样；已更新为当前 4 张/2 张限制，并标明 ADR-039 覆盖采样上限。
