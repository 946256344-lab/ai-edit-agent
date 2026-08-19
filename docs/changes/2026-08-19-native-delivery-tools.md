# 2026-08-19：NativeToolLoop 迁移文本、音乐与 Jianying 工具

Native Function Tool 目录现覆盖本轮指定的八项能力：

- `get_text_capabilities`
- `replace_text_tracks`
- `search_music`
- `download_music`
- `use_online_music`
- `replace_music_tracks`
- `render_preview`
- `create_jianying_draft`

其中三项原有观察/preview 定义保持原有契约；新增写工具的严格 schema 位于 `src-tauri/src/agentloop/tools.rs`。所有对象层级关闭额外属性，所有 schema 属性列入 `required`，语义可选值通过 nullable 表示。模型参数不含 project、conversation、本机路径、许可证或 Jianying 兼容性。

Native loop 的参数边界在 `agentloop/native.rs` 中重新校验字符串、时间范围、音量、文本样式/布局、动画及嵌套对象键；`null` 的文本默认样式或音乐默认循环/淡入淡出在调用既有 `apply_skill` 前归一为省略字段。执行没有复制音乐下载、许可证验证、文本能力矩阵、版本写入、事务、剪映兼容性或草稿实现。`use_online_music` 的时间线版本继续进入持久化步骤审计。

请求策略默认只暴露观察工具。只有中文或英文中明确要求字幕/文本、下载音乐、使用在线音乐、替换音乐或创建 Jianying draft 时，才向模型提供对应写工具；执行前使用同一策略再次拒绝伪造调用。`render_preview` 保留既有精确预览意图门。Storyboard 的 `needs_confirmation` 后，Native loop 继续拦截任何非观察写操作。

新增测试覆盖：完整工具目录、strict/closed/nested schema、nullable 参数、作用域与许可证字段拒绝、文本/音乐参数边界，以及中文/英文显式授权和执行层二次拒绝。无真实 Provider、网络下载、媒体渲染或 Jianying 写入发生在测试中。

同步文档：

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/api.md`
- `docs/architecture.md`
- `docs/decisions.md`
- `docs/harness.md`
