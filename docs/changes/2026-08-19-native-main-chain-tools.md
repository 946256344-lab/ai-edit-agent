# 2026-08-19：NativeToolLoop 迁移主链工具

NativeToolLoop 现在按请求权限为非只读请求注册六个主链原生 Function Tool：

- `request_asset_analysis`
- `generate_storyboard`
- `create_timeline_draft`
- `replace_clips`
- `change_clip_duration`
- `reorder_clips`

工具定义集中在 `src-tauri/src/agentloop/tools.rs`。每项使用 `strict: true`、`additionalProperties: false` 和完整 `required`；可选参数使用 nullable，镜头数组和源时间字段有数量、字符串、非负时间及调整边界。模型不接收 `projectId`、`conversationId`、本地路径或 FFmpeg 参数。

Native 参数边界与安全结果包络位于 `agentloop/native.rs`。执行仍只调用现有 `skills::apply_skill`，因此素材证据、源时间范围、项目/任务/时间线作用域、版本化写入、SQLite 事务和审计校验保持在原有领域函数中。素材分析的 `queued` 与 storyboard 的 `needs_confirmation` 是既有成功状态，允许作为安全 `function_call_output` 继续交给模型；未迁移的文本、音乐下载/编辑和 Jianying 工具不进入原生目录。

新增固定 JSON fixture 覆盖复合顺序“分析素材 → storyboard → timeline”，并补充六个工具的 strict schema、参数边界、目录选择和未迁移写工具排除测试。Legacy Runtime、Router、LoopGoal、确认门和 SQLite schema 保持不变。

7B 审查修复：Native 请求现在默认只携带观察工具，只有用户明确请求对应的分析、创建或修改能力时才逐项暴露主链工具；执行层对伪造调用再次拒绝。项目事实终态由 `NativeRunReceipt` 的成功只读观察门决定，失败观察不会被当作事实，工具失败后的模型解释保留但结构化终态为失败或部分完成。Storyboard 的 `needs_confirmation` 继续阻止时间线写入，持久化确认必须绑定项目、任务、会话、来源任务和 storyboard，过期或重复确认封闭失败。

新增回归覆盖：普通聊天、中文/英文只读与明确授权、未授权 function call、提前事实回答、成功/失败观察、写工具替代观察、模型虚假完成、工具失败后的自然回复、确认作用域、过期与重放。

同步文档：

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `README.md`
- `TASKS.md`
- `AGENTS.md`
- `docs/harness.md`
