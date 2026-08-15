# 前端领域拆分与架构预算

## 背景

素材工作区恢复后，审查确认素材组件本身已收敛，但 `App.tsx` 仍同时管理项目、会话、Provider、素材轮询、成果交付和 Agent task 终态恢复；`ConversationWorkspace` 同时承载 Agent 与成果模式并接收约 36 个扁平 props。仅依赖 Markdown 无法防止后续功能再次回流这些入口。

## 实现

- 将 Provider 状态、OAuth/自定义 API 副作用移入 `useProviderController`，模态框改为独立展示组件。
- 将素材分页、轮询、导入、健康、重链路和证据状态移入 `useAssetWorkspaceController`；目录树局部 `expandedFolderIds` 与单一 toggle 行为不变。
- 将 storyboard、timeline、preview、Jianying 状态和交付动作移入 `useArtifactWorkspaceController`。
- 将真实 task ID、早到事件、终态轮询与持久化 `working` 恢复对账原样收口到 `useAgentRunReconciliation`。
- 删除 `ConversationWorkspace`，以互斥的 `AgentWorkspace` 和 `ArtifactsWorkspace` 取代；侧栏、顶栏、Provider 与分析提示也拆成单一职责组件。
- 新增机器可读架构预算、检查脚本和单元测试；接入 `harness:check`、`harness:staged` 和既有 pre-commit。预算同时限制行数、字符总量与最长单行，计数所有 async 声明，并对无法解析或使用 rest 的受保护 props 签名 fail-closed，不能靠改写函数形态或压缩代码绕过。受保护文件改名必须留下 replacement、新预算和旧路径禁用记录，且新目标继承全部旧数值上限与跨层禁止规则，不能连文件与预算一起删除后逃逸。
- 新增无数据副作用的 Tauri WebView 回归脚本，覆盖真实项目恢复、互斥模式、目录开合、Provider 弹窗和运行时错误。

## 不变边界

本变更不修改 Tauri 公开命令、Rust、SQLite schema、项目数据、素材与分析证据、storyboard、timeline、preview、Jianying draft 或 Agent 工具副作用。Agent 终态对账继续满足真实 `agentTaskId` 优先、早到事件缓存、活动请求轮询、持久化 `working` 首次 terminal 快照恢复和同项目/剪辑任务可见更新门。

## 同步文档

- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/decisions.md`
- `docs/harness.md`

## 验证

- `npm run lint`、`npm run build`、`npm run harness:test`、`npm run harness:check` 与 `git diff --check` 通过。
- 真实 Tauri WebView 回归通过：SQLite local project 恢复 891 条素材；Agent/素材/成果模式严格互斥；安全导入根自动展开，折叠子目录可展开；成果页只有一套 Workflow；Provider 模态可开合且凭据字段存在；无 console/runtime 错误和全局横向溢出。
- 独立 Agent 完成三轮审查。先后发现 props/async/压缩代码检查绕过，以及预算删除/迁移逃逸；实现逐项修复并补充负向回归。第三轮指出的迁移继承缺口最终以独立的“缺数值指标”和“缺跨层禁令”失败用例封闭。
