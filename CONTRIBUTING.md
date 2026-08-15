# 多 Agent 协作开发规范

本文件是分支、编码、验证、提交和合并流程的唯一事实源。Cursor、Codex、Claude Code、OpenCode 都遵守同一流程；各工具入口只引用本文件，不复制规则，也不分配固定职责。

## 开始任务

1. 执行 `git fetch origin`，确认本地 `origin/master` 是最新远端基线。
2. 每个独立任务使用一个独立分支；并行任务必须使用独立 worktree，禁止多个 Agent 同时修改同一工作目录。
3. 从 `origin/master` 创建分支，命名为 `<类型>/<简短主题>`。允许类型：`codex/`、`cursor/`、`claude/`、`opencode/`、`feature/`、`fix/`、`refactor/`、`docs/`、`chore/`。
4. 阅读根 `AGENTS.md`、`TASKS.md` 当前窗口和目标目录的 `AGENTS.md`；非简单修改先登记 `TASKS.md`。

```powershell
git fetch origin
git worktree add ..\worktrees\素材目录 -b feature/asset-tree origin/master
```

只有单 Agent 且当前工作区干净时，才可在当前目录创建分支：

```powershell
git switch -c fix/preview-recovery origin/master
```

## 修改边界

- 一个分支只解决一个可说明的目标；发现无关问题时记录到 `TASKS.md`，不要顺手扩大范围。
- 修改前先确认事实所有者、公开契约、持久化和副作用边界；不得让多个 Agent 同时编辑同一文件。
- 保留用户已有改动；不得用 reset、checkout 或覆盖方式清理不属于当前任务的变更。
- 架构、公开契约、任务状态或机器规则变化时，同步长期文档和 `docs/changes/`。
- 合并冲突由当前分支作者在更新到最新 `origin/master` 后解决，并重新运行完整验证。

## 提交前

先查看范围，再运行适用验证：

```powershell
git status --short
git diff --check
npm run branch:check
npm run harness:test
npm run harness:check
```

前端变化追加 `npm run lint` 与 `npm run build`；Rust 变化追加 `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo check --manifest-path src-tauri/Cargo.toml` 和 `cargo test --manifest-path src-tauri/Cargo.toml`。高风险路径按 `docs/harness.md` 完成独立 Agent 审查。

提交必须是可审查、可回退的完整单元。提交信息使用 `<类型>: <结果>`，例如 `fix: restore asset folder expansion`。禁止直接在 `master` 或 `main` 提交，禁止把多个 Agent 的未审查结果打包成一个无边界提交。

## PR 与合并

1. 推送当前功能分支并创建 PR，不直接推送 `master`。
2. 按 PR 模板写清目标、范围、禁止变化、契约影响和验证证据。
3. 合并前重新获取远端基线；若 `origin/master` 不是当前分支祖先，先 rebase 或 merge，再重新验证。
4. 至少一名未实现该变更的审查者确认；高风险变更同时满足 `docs/harness.md` 的独立审查闭环。
5. 只在检查通过、审查结论关闭后合并。合并后删除功能分支和对应 worktree。

本地 hook 可以被 `--no-verify` 绕过，因此远端仓库仍应启用 master 分支保护、必需状态检查和 PR 审查；这项远端设置不由仓库脚本自动修改。
