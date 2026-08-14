# 模型中心的 storyboard 编排

日期：2026-08-10

## 变更

- 顶层 Agent 编排预算从 6 步提升为 10 步；storyboard 请求本身拥有最多 3 次仅在内存中的修订预算。失败候选与本地校验反馈回传给模型，模型决定修订，不会重新向用户索要已经给出的成片目标。
- 最后一个编排步骤要求模型对已经实际落地的产物和未完成项作自然语言总结；但完成事实仍以工具返回的后端验证摘要为准，防止模型把未创建的 preview 或 Jianying draft 说成已完成。部分完成时保留真实中间产物，不再用“没有修改时间线”的固定文案覆盖。
- storyboard 由模型提出 `targetDurationMs` 和 `scriptMode`（`full_script` 或 `key_message`）。镜头数、信息点数、成片时长不再使用固定创作规格；30 镜头/信息点和 120 秒仅是本地处理安全上限。完整文案仍受最低可读时长保护。
- 删除 preview 中 0.75–6 秒的固定节奏提示；节奏属于模型与用户的创作判断，不是程序规则。
- 新增 `request_asset_analysis` 受控技能。模型在 `list_assets` 发现项目内已导入、尚未分析或分析失败的素材时可以排队分析；Rust 持有文件、SQLite、FFprobe/FFmpeg/Tesseract 与异步任务执行权，模型不能直接操作它们。

## 边界

模型的自然语言总结不会改变真实产物门：只有后端已验证并持久化的 storyboard、内部时间线、preview 或 Jianying draft 才能作为完成事实。Provider 不可用或没有任何模型回复时仍使用固定安全降级消息。

## 验证

- `cargo fmt --check`
- `cargo test --lib`
- `npm run lint`
- `npm run build`
- `npm run harness:check`

同步文档：`README.md`、`docs/architecture.md`、`docs/api.md`、`docs/decisions.md`、`TASKS.md`。
