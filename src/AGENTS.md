# 前端 Agent 规则

本文件适用于 `src/`。开始前先读仓库根 `AGENTS.md`、`TASKS.md` 当前窗口、`docs/codebase/STRUCTURE.md`、`docs/codebase/CONVENTIONS.md` 和 `docs/codebase/ARCHITECTURE.md`。

## 所有权

- `App.tsx` 只拥有当前项目、任务、conversation 的组合与跨工作区接线。
- `hooks/use*Controller.ts` 各自拥有一个领域的异步状态、竞态防护和命名动作。
- 核心领域工作区通过明确的 `model/actions` 展示；叶组件可以使用少量具名 props。局部展开、折叠、tab 等纯 UI 状态可留在组件。
- `lib/local-store.ts` 是唯一应用 Tauri command/raw `invoke` 桥；React 组件和 hooks 不得直接调用应用命令。现有 dialog、opener、event 与 `convertFileSrc` 是受限 Tauri service API，不属于该 command bridge。
- `lib/agent-tools.ts` 只是 IDE 可读的 Agent 工具目录镜像，不是执行权限来源；Rust 白名单与版本化 fixture 才是运行事实。

## 状态与恢复

- 浏览器原型状态不得伪装为生产持久化。SQLite 是任务、消息、素材和产物的恢复事实。
- Tauri 事件只负责低延迟通知；Agent 终态必须同时支持命令返回竞态对账和持久化轮询恢复。
- pending 请求、轮询和终态对账必须绑定同一 project、editing task、conversation 和真实 task id；旧任务不得被误认作当前任务。
- 只有仍处于同一作用域时才更新可见 storyboard、timeline、preview 或 Jianying draft。
- 素材目录的展开状态由 `AssetDirectoryTree` 的 `expandedFolderIds` 与 `toggleAssetFolder` 拥有；目录选择和展开是两个动作，`aria-expanded` 必须反映真实状态。

## 数据与展示

- 面向用户只显示固定、安全的错误文案，不透出 Rust/Provider 原始错误、本机路径或凭据。
- 可以在受控展示处使用 `convertFileSrc` 映射后端已返回的派生媒体 URL；不得借此绕过 bridge 读取任意文件。
- 保持现有深色、信息密集视觉语言和响应式断点，除非任务明确要求设计变更。
- 产品词汇统一使用 Agent、storyboard、draft、preview、Jianying draft、local project。

## 修改完成

运行 `npm run lint`、`npm run build`、`npm run agent:check` 和 `npm run harness:check`。如果改变 IPC 名称、公开字段或 Agent 工具镜像，必须同步 Rust 注册/fixture、`docs/api.md` 和变更记录。
