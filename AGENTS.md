# Assembly Video Agent：Agent 入口

本文件是所有代码任务的最小入口。不要把整份仓库文档一次性塞进上下文；先读取当前任务窗口和当前目录适用的指令，再按变更范围补充事实文档。

## 开始任何修改

1. 阅读 `CONTRIBUTING.md` 和 `TASKS.md` 的 `ACTIVE_TASKS` 标记区；前者是分支、验证、提交与 PR 的唯一流程，后者只有标记区是当前工作。
2. 修改 `src/` 时完整阅读 `src/AGENTS.md`；修改 `src-tauri/src/` 时完整阅读 `src-tauri/src/AGENTS.md`。同时修改两侧时两份都读。
3. 用 `docs/codebase/STRUCTURE.md` 定位实现；只有碰到相应边界时才读取长期文档：
   - 产品与运行链路：`docs/architecture.md`
   - Tauri 命令、Agent 工具或公开类型：`docs/api.md`
   - 架构取舍或依赖：`docs/decisions.md`
   - 验证与机器约束：`docs/harness.md`
   - 当前实现风险：`docs/codebase/CONCERNS.md`
   - 只读媒体事实审计报告：`docs/audits/`
4. 非简单任务先更新当前任务窗口。未知行为写成 `TODO` 或请求确认，不得补写想象中的事实。
5. 每个独立任务使用独立任务分支；并行 Agent 使用独立 worktree。不得直接在 `master`/`main` 提交或推送。

## 全局不可破坏规则

- 产品是 Windows 优先的本地视频剪辑 Agent，不是传统剪辑器套聊天框。模型在有界工具循环内选择动作；Rust 校验作用域、副作用和真实完成状态。
- 媒体语义必须来自真实分析和明确源时间范围；不得从文件名、目录名或模型自述推测内容。
- 原始媒体、项目数据、内部时间线、preview 和 Jianying draft 留在本机。不得把凭据、本机路径、会话原文、模型原文或媒体证据写入日志、源码、浏览器存储或文档示例。
- 最终导出、覆盖既有导出，或删除项目、素材、版本前必须明确确认。内部新版本、低清 preview 和新的 Jianying draft 可由 Agent 在既有边界内创建。
- Provider 必须可替换；凭据读取异常时失败封闭，不得静默切换到用户未预期的 Provider。
- Jianying 交付单向且只创建新 draft；不得覆盖旧 draft，也不反向同步 Jianying 内编辑。
- 模型文本不是事实。产物、任务终态和恢复状态必须由持久化记录及磁盘事实验证。

## 代码与文档原则

- 保持 React 19、TypeScript 和 Rust 现有模式；纯类型导入使用 `import type`，避免无用途的泛化抽象和依赖。
- `src/App.tsx` 只做组合；领域状态进 controller，展示进 component。应用 Tauri command/raw `invoke` 只经 `src/lib/local-store.ts`；dialog、opener、event 与 `convertFileSrc` 等既有 Tauri service API 按目录规则使用。
- Rust 是可信执行边界。外部进程、凭据、网络、SQLite 事务和工具副作用遵守 `src-tauri/src/AGENTS.md`。
- 全部手写源码模块顶部保留中文职责导航；注释解释“为什么存在边界、谁拥有事实、失败如何恢复”，不逐行翻译代码。
- 架构、公开契约、持久化或任务状态变化时，同步长期文档和 `docs/changes/`。`.harness/doc-sync-policy.json` 是最低同步要求。

## 完成门

先运行 `npm run agent:check`，再按范围运行：

```powershell
npm run lint
npm run build
npm run harness:test
npm run harness:check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
python -m unittest discover -s src-tauri/scripts -p "test_*.py"
```

高风险路径变化后必须重新执行受影响的端到端验收；绿色静态基线不能冒充产品验收。`harness:check` 触发架构规则时，按 `docs/harness.md` 完成独立 Agent 审查闭环。

## 给其他 AI Agent 的兼容说明

支持分层 `AGENTS.md` 的 Agent 会自动获得就近规则；不支持时，必须手动读取本文件、`TASKS.md` 当前窗口及目标目录的 `AGENTS.md`。机器检查只能证明可验证边界没有漂移，不能证明模型理解了全部产品语义。
