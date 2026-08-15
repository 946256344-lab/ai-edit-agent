# 文档同步 Harness

## 多 Agent 分支与入口门

`CONTRIBUTING.md` 是分支、worktree、验证、提交和 PR 的唯一流程。Claude Code、Cursor、OpenCode 的薄入口必须持续引用 `AGENTS.md`、`CONTRIBUTING.md` 和 `TASKS.md`；`.harness/agent-context.json` 会检查这些引用并以只收紧 ratchet 防止移除。

`.harness/branch-policy.json` 规定受保护分支和允许前缀，并相对 Git `HEAD` 拒绝降低版本、更换远端基线、移除受保护分支或扩大允许前缀。`npm run branch:check` 拒绝 detached HEAD、`master`/`main`、未知前缀和未包含本地 `origin/master` 的任务分支，`npm run branch:test` 提供纯负向回归。pre-commit 先执行分支检查，再检查暂存快照。它不会自动 fetch，也可被 `--no-verify` 绕过，不能替代 GitHub 远端分支保护。

## 目的

本 harness 同时约束代码结构增长、Agent 编程上下文和文档同步。它将架构、公开工具契约、持久化、Provider/凭据安全和桌面运行时的高影响改动，与同一 Git 变更集中的 Markdown 更新绑定；以机器可读预算阻止入口、组件、controller 和已有后端热点继续膨胀，并阻止编码 Agent 绕过已声明的 IPC、进程、凭据、网络与工具目录边界。可预测脚本负责硬门，独立上下文的 Agent 审查负责语义一致性。

Markdown 提供产品意图、架构背景和决策记录；脚本、测试、后端校验和权限确认负责不可绕过的执行约束。两者互补，不能互相替代。

## 规则

`.harness/doc-sync-policy.json` 是唯一的机器可读规则来源。当前规则：

| 触发范围 | 必须同步的文档 |
| --- | --- |
| Tauri 命令与存储入口（`main.rs`、`lib.rs`、`src-tauri/src/*.rs`、`oauth.rs`、`commands/`）、`local-store.ts`、`agent-tools.ts` | `docs/architecture.md`、`docs/api.md`、`TASKS.md` |
| `oauth.rs`、`custom_api.rs`、`music_provider.rs`、`provider.rs`、`agent.rs` | `AGENTS.md`、`docs/decisions.md` |
| `package.json`、Cargo、Tauri 配置 | `README.md`、`docs/architecture.md`、`docs/decisions.md`、`TASKS.md` |
| 架构预算配置或检查脚本 | `docs/architecture.md`、`docs/decisions.md`、`docs/harness.md`、`TASKS.md` |
| 根/前端/Rust Agent 指令、Agent 上下文清单或契约检查脚本 | `AGENTS.md`、`README.md`、`docs/architecture.md`、`docs/decisions.md`、`docs/harness.md`、`TASKS.md` |

每个触发规则的变更集还必须有一份 `docs/changes/YYYY-MM-DD-主题.md` 记录，并在其中列出实际同步的文档。规则必须保持窄且可解释；新增高影响区域时才扩展策略表。

`.harness/architecture-budgets.json` 是结构预算的唯一机器可读来源。当前硬门包括：

- `App.tsx` 的行数、字符总量、最长单行、`useState`、`useEffect` 和所有 async 声明上限；
- `src/components/**/*.tsx` 与 `src/hooks/**/*.{ts,tsx}` 的单文件行数、字符总量和最长单行上限，避免通过压缩代码绕过行数门；
- 核心工作区组件的一至两个顶层领域 props；props 签名无法解析时检查直接失败，不把未知结构误当作零 props；
- `local-store.ts`、父 `agentloop.rs`、纯 `agentloop/policy.rs` 和其他 Rust 热点的行数、字符总量与最长单行只降不升棘轮；
- 受保护文件删除或改名时必须通过永久保留的 `budgetReplacements` 显式指向新预算，把旧路径加入 `forbiddenPaths`，并让新目标以不放宽的值继承全部数值指标和原路径跨层禁止规则；目录预算即使暂时为空也作为防回归墓碑保留；
- 已删除 `ConversationWorkspace` 不得恢复，Agent 对账、素材分页、成果交付和 Provider 状态不得回流 `App.tsx`。

预算值是当前基线的防回退门，不是“达到上限前可以继续堆”的目标。检查脚本会与 Git `HEAD` 中的预算比较：已有数值不得提高、已有指标和禁止项不得移除；受保护文件仍存在时也不得删除其预算。需要增加行为但将超过预算时，必须先拆出具有领域名称的模块或组件，不能靠压缩代码、超长行、提高数字或删除测试绕过。真正替换整个架构边界时应通过删除旧文件、建立受保护的新边界、记录 ADR 和独立审查完成，而不是放宽原文件。

`.harness/agent-context.json` 是编码 Agent 上下文和跨层允许列表的机器来源。`agent:check` 当前强制：

- 根、`src/`、`src-tauri/src/` 三份指令存在；`docs/codebase/` 只能包含指定七份地图且每份有证据章节；
- `TASKS.md` 的 `ACTIVE_TASKS` 标记唯一、顺序正确，且非空行数和总字符数都不超过上限；
- Rust、TypeScript/React、Node、Python、HTML、Shell 与 CSS 受控手写源码的顶部 16 行内存在中文职责导航；
- `src/lib/local-store.ts` 是唯一前端 `invoke` 所有者，所有被调用命令必须在 `lib.rs` 注册，所有已注册公开命令必须出现在 `docs/api.md`；
- 外部进程、`keyring`/`Entry::new` 与 HTTP/网络传输只能出现在清单允许的 Rust 边界；
- `agentloop/policy.rs` 的 Rust 观察/编辑工具白名单、TypeScript IDE 镜像和版本化 Agent fixture 的名称完全一致。

桌面契约文档触发器同时匹配 `src-tauri/src/*.rs` 与 `src-tauri/src/**/*.rs`，因此后续提取的 Rust 子模块不能绕过架构/API/TASKS/变更记录同步。

这些规则故意只覆盖可以确定性判断的架构事实。新增合法边界时必须同时修改机器清单、测试、ADR 和变更记录；不能为了让检查通过而扩大通配允许范围。

只读媒体事实审计（`docs/audits/`）不触发上述写规则，但审计发现的系统性问题须同步更新 `docs/decisions.md` 和 `TASKS.md`，并作为 P0 修复项在独立分支中跟踪。

Agent 上下文清单还与 Git `HEAD` 基线执行只收紧 ratchet：既有指令、作用域、必读文档、验证命令、受检扩展名、中文导航范围和暂存运行文件不得移除；任务窗口与导航头部行数上限不得提高；可信边界不得增加允许路径。pre-commit 直接运行三份 staged 检查脚本，不再通过可改写的 npm 别名间接调用；检查器要求 hook、三份检查器及其机器配置在部分暂存时与 index 完全一致，避免使用 working-tree 强版本放过 staged 弱版本。

本地 Git hook 不是对恶意修改仓库本身的安全沙箱，开发者也可用 `--no-verify` 绕过；当前仓库尚无 CI，这是 `docs/codebase/CONCERNS.md` 已记录的剩余风险。这里的目标是阻止编码 Agent 的无意漂移，并让合法放宽必须显式留下配置、测试、ADR 和审查证据。

## 命令

```powershell
npm run harness:install
npm run architecture:check
npm run agent:check
npm run agent:test
npm run harness:check
npm run harness:staged
npm run harness:test
```

- `harness:install`：将 Git 的本地 hooks 路径设为版本控制的 `.githooks`。
- `architecture:check`：只检查当前工作区的结构预算。
- `agent:check` / `agent:test`：检查 Agent 上下文、跨层所有权与工具目录，及其虚拟仓库负向回归。
- `harness:check`：依次检查当前工作区的结构预算、Agent 契约和未暂存/未跟踪文件的文档同步。
- `harness:staged`：对 Git 暂存内容运行三层检查，供提交前 hook 使用；包含删除操作，并只读取将被提交的内容。
- `harness:test`：运行架构预算、Agent 契约和文档同步脚本的单元测试。

首次初始化后应先建立一个干净的 Git 基线提交；在基线提交之前，`harness:check` 会把所有未跟踪项目文件视为当前变更。

## Agent 审查 Loop

触发规则的任务完成前，Agent 必须重复以下步骤，最多三轮：

1. 运行 `npm run harness:check`，修复所有硬检查失败。
2. 用新的 Agent 上下文审查需求、Git diff、`docs/changes/` 记录以及受影响的长期文档。
3. 审查者只报告可验证的问题：代码契约与 API 文档不一致、持久化行为未写入架构、产品规则与后端保护不一致、任务状态不准确，或变更记录遗漏。
4. 实现 Agent 修复发现，重新运行相关测试和 `npm run harness:check`。
5. 连续通过硬检查、独立审查和验证后才结束；三轮后仍有阻塞问题时，记录未决项并交给用户判断。

不要让实现 Agent 以自己的结论作为唯一验收。独立审查者应在新上下文中仅获取需求、diff、变更记录和相关文档，以降低自我确认偏差。
