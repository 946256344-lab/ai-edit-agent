# Agent 执行过程任务卡

## 变更

- Agent 对话区显示当前剪辑会话最近一次执行型任务的可折叠任务卡；运行中默认展开，完成后保留摘要和可检查步骤。
- 任务卡通过既有三重作用域查询轮询 payload-free `agent_run_steps`，展示当前动作、已完成步骤数、运行时长，以及步骤记录中已经落地的 storyboard、内部时间线、local preview 或 Jianying draft。
- 内部工具名统一映射为产品文案。未知工具只显示“执行受限操作”，不回显模型推理、工具参数、错误原文、本机路径或媒体证据。
- `failed`、`partially_completed`、`needs_clarification` 与 `needs_review` 使用不同的诚实终态说明；单步失败不会提前伪造整个任务失败。
- 远端模型阶段只使用不确定进度动画和真实步骤计数，不显示无法验证的百分比。项目级后台媒体分析继续保留在右下角，不与当前对话任务混用。

## 验证

- `npm run lint`
- `npm run build`
- `npm run harness:check`
- 定向 `git diff --check`

以上检查通过；lint 仅保留 `App.tsx` 既有的 `syncDeliveryStatus` Hook 依赖警告。未启动或重启桌面程序；桌面视觉交互仍待用户当前运行任务结束后自行查看。
