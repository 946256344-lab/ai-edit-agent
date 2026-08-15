# 编码约定

## 1）命名规则

| 项目 | 规则 | 示例 | 证据 |
| --- | --- | --- | --- |
| React 文件 | PascalCase | `AgentWorkspace.tsx` | `src/components/` |
| React hook | `use` + PascalCase，文件 camelCase | `useAssetWorkspaceController` | `src/hooks/` |
| TS 类型 | PascalCase | `ConversationTurnResult` | `src/lib/local-store.ts` |
| Rust 文件/函数 | snake_case | `resolve_conversation_task` | `src-tauri/src/taskrouter.rs` |
| Rust 类型 | PascalCase | `AgentStateSnapshot` | `src-tauri/src/agentloop.rs` |
| Rust 常量 | SCREAMING_SNAKE_CASE | `MAX_STEPS` | `src-tauri/src/agentloop.rs` |
| 持久化/IPC JSON | 对外 camelCase、数据库 snake_case | `agentTaskId` / `agent_task_id` | `models.rs`、`db.rs` |

## 2）格式化、lint 与严格度

- TypeScript 当前**没有**显式开启总开关 `strict`；已开启 `noUnusedLocals`、`noUnusedParameters`、`noFallthroughCasesInSwitch` 和 `verbatimModuleSyntax`，纯类型导入必须 `import type`。[TODO] 应单独评估并逐步开启 `strict`，不能把现有局部严格选项误报为完整严格模式。证据：`tsconfig.app.json`。
- React hooks 规则由 Oxlint 强制。证据：`.oxlintrc.json`。
- 仓库没有 Prettier 配置；现有 TS 使用无分号、单引号风格，但这不是独立 formatter 强制的完整规则。[TODO] 如需格式统一，应先形成明确 formatter 决策。
- Rust 使用标准 `cargo fmt`；MSRV 在 Cargo manifest 中为 1.77.2。
- 架构预算限制核心文件行数、字符数、最长单行、hooks 和 props；不得提高预算代替拆分。

## 3）导入与模块约定

- TypeScript 使用相对路径，无 `paths` alias、无 barrel export。
- React 组件只接收少量领域 `model/actions`；展示组件不直接 `invoke`。
- `src/lib/local-store.ts` 是前端唯一 Tauri 命令桥；函数名用前端 camelCase，命令字符串保持 Rust snake_case。
- Rust 模块默认私有，以 `pub(crate)` 暴露跨模块能力；只有 Tauri 命令或 crate 入口使用 `pub`。
- `lib.rs` 是 IDE 模块索引和命令注册事实来源。
- 编码 Agent 先读根 `AGENTS.md` 和 `TASKS.md` 当前窗口，再读目标路径最近的 `AGENTS.md`；长期文档按入口路由加载，不把历史记录全部当当前需求。

## 4）错误和日志约定

| 层 | 当前策略 |
| --- | --- |
| React | 捕获 Tauri 错误，显示固定用户文案；不展示原始技术错误 |
| Tauri 命令 | `Result<T, String>`，先校验项目/task/conversation/asset 作用域 |
| Agent loop | 工具失败转安全码/结构化上下文，供同一有界循环决定下一步 |
| Agent 终态 | 无产物时固定诚实回复；不能采用模型自称完成 |
| 日志 | 固定阶段、时长、状态和安全 ID；不得记录 prompt、响应原文、凭据、路径或媒体证据 |
| 凭据错误 | 读取损坏/不可用时失败封闭，不回退到非预期 Provider |

直接新增 `log::*` 前，应检查插值值是否可能含源路径、用户请求或 Provider 响应。

## 5）注释约定

- 注释解释“为什么有此边界、谁拥有事实、失败如何恢复”，不逐行复述语法。
- Rust 模块职责在 `lib.rs` 的 module doc 上维护，方便 IDE hover。
- 对有竞态或安全要求的 hook/函数使用 JSDoc/Rustdoc；普通 setter 和明显 JSX 不加噪声注释。
- 架构重构后必须同步删除或修改失真的注释；文档不能替代测试和后端校验。

## 6）测试约定

- Rust 单元测试与模块共置在 `#[cfg(test)] mod tests`。
- 跨模块 Agent 契约放 `src-tauri/tests/`，fixture 带版本号。
- Python 适配器用 `unittest.mock` 隔离 Jianying/进程/文件系统。
- Node harness 用 `node:assert/strict`，真实桌面用 WebView2 CDP。
- `agent:check` 对分层指令、当前任务窗口、IPC、Rust 外部边界和工具目录执行 fail-closed 检查；合法边界变化必须修改配置与负向测试。
- 当前没有覆盖率阈值或前端组件测试框架。[TODO]

## 7）证据

- `tsconfig.app.json`
- `.oxlintrc.json`
- `src/App.tsx`
- `src-tauri/src/lib.rs`
- `src-tauri/src/audit.rs`
- `AGENTS.md`
- `.harness/agent-context.json`
