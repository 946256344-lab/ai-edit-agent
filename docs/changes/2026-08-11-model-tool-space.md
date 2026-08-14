# 扩大模型工具操作空间

2026-08-11

## 问题

自然语言处理仍有三个会替模型缩小操作空间的节点：未限定“草稿”被直通到 Jianying、preview/Jianying draft 隐式创建时间线、以及前置提示用固定顺序指定工具。

## 决策

- 仅明确包含“剪映”或 `Jianying draft` 的精确命令可直通；未限定草稿进入模型工具循环。
- `render_preview` 与 `create_jianying_draft` 只能使用现有且已作用域化的时间线；模型必须自己显式选择 `create_timeline_draft`，后端不再隐式创建版本。
- 前置提示改为真实状态与可用工具，不再要求固定调用顺序。

## 验证

- 新增/更新未限定草稿、交付工具缺少时间线和非强制提示的回归测试。
- 46 单元 + 2 集成测试通过；前端 lint/build、harness 与 diff 检查通过。
- 新安装版启动后窗口保持 `Responding=True`。
