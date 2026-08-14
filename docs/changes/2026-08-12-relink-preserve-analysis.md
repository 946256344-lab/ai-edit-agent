# 重链接素材时保留既有分析证据

2026-08-12

## 问题

`confirm_asset_relink` 之前无条件清除所选素材的 `metadata_json`、取消 active 分析任务并重新排队分析。素材确实只是迁移位置时，重链接会白白重跑一遍本地媒体分析。

## 决策

为 `confirm_asset_relink` 增加 `preserveAnalysis` 布尔参数，作为显式契约而非模型/关键词判断：

- `preserveAnalysis=true`：只在同一事务内更新 `source_reference` 与 `folder_reference`，不清除 `metadata_json`、不取消任务、不排队新分析；派生证据（缩略图、关键帧、OCR）位于 `app_data_dir/derived`，不依赖源路径，因此继续有效。
- `preserveAnalysis=false`：保持既有行为（清除旧证据、取消旧 active 任务、按有界批次重排分析）。
- 前端确认对话框先询问「同一批文件还是可能不同内容」：确定走保留，取消则二次确认后走重新分析。选择权交给用户，不隐式假定文件内容一致。

## 验证

- 前端 `npm run lint` 通过（0 错误 0 警告）。
- 后端 `cargo check --lib` 仅剩既有的 `agentloop.rs` Conversation Router 重构期编译错误（与本变更无关），`assets.rs` 无新增错误。
- 再次提醒：重链接按唯一相对路径匹配，不能证明新文件与旧文件内容一致；保留分析仅适用于用户确认「同一批文件迁移」的场景，默认安全路径仍是重新分析。