# 文档同步 Harness

## 目的

本 harness 将架构、公开工具契约、持久化、Provider/凭据安全和桌面运行时的高影响改动，与同一 Git 变更集中的 Markdown 更新绑定。它由可预测的脚本负责文件级约束，由独立上下文的 Agent 审查负责语义一致性。

Markdown 提供产品意图、架构背景和决策记录；脚本、测试、后端校验和权限确认负责不可绕过的执行约束。两者互补，不能互相替代。

## 规则

`.harness/doc-sync-policy.json` 是唯一的机器可读规则来源。当前规则：

| 触发范围 | 必须同步的文档 |
| --- | --- |
| Tauri 命令与存储入口（`main.rs`、`lib.rs`、`store.rs`、`oauth.rs`、`commands/`）、`local-store.ts`、`agent-tools.ts` | `docs/architecture.md`、`docs/api.md`、`TASKS.md` |
| `oauth.rs`、`store.rs` | `AGENTS.md`、`docs/decisions.md` |
| `package.json`、Cargo、Tauri 配置 | `README.md`、`docs/architecture.md`、`docs/decisions.md`、`TASKS.md` |

每个触发规则的变更集还必须有一份 `docs/changes/YYYY-MM-DD-主题.md` 记录，并在其中列出实际同步的文档。规则必须保持窄且可解释；新增高影响区域时才扩展策略表。

## 命令

```powershell
npm run harness:install
npm run harness:check
npm run harness:staged
npm run harness:test
```

- `harness:install`：将 Git 的本地 hooks 路径设为版本控制的 `.githooks`。
- `harness:check`：检查当前工作区，包括未暂存和未跟踪文件。
- `harness:staged`：检查暂存区，供提交前 hook 使用；包含删除操作，并只读取将被提交的暂存内容。
- `harness:test`：运行策略检查脚本的单元测试。

首次初始化后应先建立一个干净的 Git 基线提交；在基线提交之前，`harness:check` 会把所有未跟踪项目文件视为当前变更。

## Agent 审查 Loop

触发规则的任务完成前，Agent 必须重复以下步骤，最多三轮：

1. 运行 `npm run harness:check`，修复所有硬检查失败。
2. 用新的 Agent 上下文审查需求、Git diff、`docs/changes/` 记录以及受影响的长期文档。
3. 审查者只报告可验证的问题：代码契约与 API 文档不一致、持久化行为未写入架构、产品规则与后端保护不一致、任务状态不准确，或变更记录遗漏。
4. 实现 Agent 修复发现，重新运行相关测试和 `npm run harness:check`。
5. 连续通过硬检查、独立审查和验证后才结束；三轮后仍有阻塞问题时，记录未决项并交给用户判断。

不要让实现 Agent 以自己的结论作为唯一验收。独立审查者应在新上下文中仅获取需求、diff、变更记录和相关文档，以降低自我确认偏差。
