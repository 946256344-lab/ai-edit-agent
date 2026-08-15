# 文档同步 Harness

## 目的

本 harness 同时约束代码结构增长和文档同步。它将架构、公开工具契约、持久化、Provider/凭据安全和桌面运行时的高影响改动，与同一 Git 变更集中的 Markdown 更新绑定；并以机器可读预算阻止入口、组件、controller 和已有后端热点继续膨胀。可预测脚本负责硬门，独立上下文的 Agent 审查负责语义一致性。

Markdown 提供产品意图、架构背景和决策记录；脚本、测试、后端校验和权限确认负责不可绕过的执行约束。两者互补，不能互相替代。

## 规则

`.harness/doc-sync-policy.json` 是唯一的机器可读规则来源。当前规则：

| 触发范围 | 必须同步的文档 |
| --- | --- |
| Tauri 命令与存储入口（`main.rs`、`lib.rs`、`src-tauri/src/*.rs`、`oauth.rs`、`commands/`）、`local-store.ts`、`agent-tools.ts` | `docs/architecture.md`、`docs/api.md`、`TASKS.md` |
| `oauth.rs`、`custom_api.rs`、`music_provider.rs`、`provider.rs`、`agent.rs` | `AGENTS.md`、`docs/decisions.md` |
| `package.json`、Cargo、Tauri 配置 | `README.md`、`docs/architecture.md`、`docs/decisions.md`、`TASKS.md` |
| 架构预算配置或检查脚本 | `docs/architecture.md`、`docs/decisions.md`、`docs/harness.md`、`TASKS.md` |

每个触发规则的变更集还必须有一份 `docs/changes/YYYY-MM-DD-主题.md` 记录，并在其中列出实际同步的文档。规则必须保持窄且可解释；新增高影响区域时才扩展策略表。

`.harness/architecture-budgets.json` 是结构预算的唯一机器可读来源。当前硬门包括：

- `App.tsx` 的行数、字符总量、最长单行、`useState`、`useEffect` 和所有 async 声明上限；
- `src/components/**/*.tsx` 与 `src/hooks/**/*.{ts,tsx}` 的单文件行数、字符总量和最长单行上限，避免通过压缩代码绕过行数门；
- 核心工作区组件的一至两个顶层领域 props；props 签名无法解析时检查直接失败，不把未知结构误当作零 props；
- `local-store.ts` 和现有 Rust 热点的行数、字符总量与最长单行只降不升棘轮；
- 受保护文件删除或改名时必须通过永久保留的 `budgetReplacements` 显式指向新预算，把旧路径加入 `forbiddenPaths`，并让新目标以不放宽的值继承全部数值指标和原路径跨层禁止规则；目录预算即使暂时为空也作为防回归墓碑保留；
- 已删除 `ConversationWorkspace` 不得恢复，Agent 对账、素材分页、成果交付和 Provider 状态不得回流 `App.tsx`。

预算值是当前基线的防回退门，不是“达到上限前可以继续堆”的目标。检查脚本会与 Git `HEAD` 中的预算比较：已有数值不得提高、已有指标和禁止项不得移除；受保护文件仍存在时也不得删除其预算。需要增加行为但将超过预算时，必须先拆出具有领域名称的模块或组件，不能靠压缩代码、超长行、提高数字或删除测试绕过。真正替换整个架构边界时应通过删除旧文件、建立受保护的新边界、记录 ADR 和独立审查完成，而不是放宽原文件。

## 命令

```powershell
npm run harness:install
npm run architecture:check
npm run harness:check
npm run harness:staged
npm run harness:test
```

- `harness:install`：将 Git 的本地 hooks 路径设为版本控制的 `.githooks`。
- `architecture:check`：只检查当前工作区的结构预算。
- `harness:check`：先检查当前工作区的结构预算，再检查未暂存和未跟踪文件的文档同步。
- `harness:staged`：对 Git 暂存内容运行结构预算和文档同步，供提交前 hook 使用；包含删除操作，并只读取将被提交的内容。
- `harness:test`：运行架构预算和文档同步检查脚本的单元测试。

首次初始化后应先建立一个干净的 Git 基线提交；在基线提交之前，`harness:check` 会把所有未跟踪项目文件视为当前变更。

## Agent 审查 Loop

触发规则的任务完成前，Agent 必须重复以下步骤，最多三轮：

1. 运行 `npm run harness:check`，修复所有硬检查失败。
2. 用新的 Agent 上下文审查需求、Git diff、`docs/changes/` 记录以及受影响的长期文档。
3. 审查者只报告可验证的问题：代码契约与 API 文档不一致、持久化行为未写入架构、产品规则与后端保护不一致、任务状态不准确，或变更记录遗漏。
4. 实现 Agent 修复发现，重新运行相关测试和 `npm run harness:check`。
5. 连续通过硬检查、独立审查和验证后才结束；三轮后仍有阻塞问题时，记录未决项并交给用户判断。

不要让实现 Agent 以自己的结论作为唯一验收。独立审查者应在新上下文中仅获取需求、diff、变更记录和相关文档，以降低自我确认偏差。
