# Native 失败恢复与质量精炼续步

## 目标

NativeToolLoop 在写工具返回可重试失败或产物质量警告时，不应接受模型立即用自然语言声称完成；同时不能把所有授权工具当成固定完成清单，也不能自动重放副作用。

## 实现

- 新增 `agentloop/continuation.rs`，分别保存待恢复失败与待精炼质量警告，每类最多拦截两次提前自然语言结束。
- 只有同一工具的新结果能关闭其失败或警告；无关观察、前置工具或其他编辑成功不会误清除事实。
- `render_preview` 成功收据绑定 timeline 版本。后续时间线写工具创建新版本或缺少可验证版本时，旧 preview 收据失效并使本轮至多部分完成。
- 文本轨授权只接受添加、替换、编辑等动作短语；单独询问字幕、caption、subtitles 或 text track 保持只读。
- preview 质量报告只把 warning 级检查映射为 `qualityWarnings`，info 不触发精炼。

## 保持不变

- 不新增公开命令、Agent 工具、前端类型、SQLite schema 或依赖。
- 不自动重放写工具，不扩大 RequestToolPolicy 权限，不改变确认门。
- 达到续步预算后允许模型诚实解释无法恢复的结果，真实终态仍由 Rust 收据裁决。

## 审核与测试

- Bugbot 初审发现字幕名词误授权、preview 收据未绑定版本及续步事实被无关步骤清除，三项均已修复。
- 回归覆盖无关观察/前置成功后的恢复保持、调整后警告保持、同工具验证后关闭、preview 后新时间线版本使完成收据失效，以及字幕主题查询保持只读。

## 同步文档

- `docs/architecture.md`
- `docs/api.md`
- `TASKS.md`
