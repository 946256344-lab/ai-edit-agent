# 协作开发规范

本文件是编码、验证和提交流程的唯一事实源。Cursor、Codex、Claude Code、OpenCode 都遵守同一流程；各工具入口只引用本文件，不复制规则。

## 开始任务

1. 阅读根 `AGENTS.md`、`TASKS.md` 当前窗口和目标目录的 `AGENTS.md`；非简单修改先登记 `TASKS.md`。
2. 默认在当前 `master` 上改、提交、推送。提交前若落后 `origin/master`，先 `git pull --ff-only`。
3. 只有多个 Agent 并行时，才各自开独立分支和 worktree，禁止同时改同一工作目录。

```powershell
git fetch origin
git worktree add ..\worktrees\<task-slug> -b feature/<task-slug> origin/master
```

并行分支命名为 `<类型>/<简短主题>`。允许类型：`codex/`、`cursor/`、`claude/`、`opencode/`、`feature/`、`fix/`、`refactor/`、`docs/`、`chore/`。

## 修改边界

- 一次改动只解决一个可说明的目标；发现无关问题时记录到 `TASKS.md`，不要顺手扩大范围。
- 修改前先确认事实所有者、公开契约、持久化和副作用边界；不得让多个 Agent 同时编辑同一文件。
- 保留用户已有改动；不得用 reset、checkout 或覆盖方式清理不属于当前任务的变更。
- 架构、公开契约、任务状态或机器规则变化时，同步长期文档和 `docs/changes/`。

## AI 写测试

默认不写新测试。

仅在以下情况补测（且尽量少）：

- 改了公开契约 / fixture
- 修了真实 bug（只加能复现该 bug 的回归）
- 用户或审查明确要求

不要主动扩测。测试栈与布局见 `docs/codebase/TESTING.md`。

## 提交和推送

先看范围，再按改动补跑，不要默认全跑：

```powershell
git status --short
git diff --check
```

- 改了前端：`npm run lint`
- 改了 Rust：`cargo check --manifest-path src-tauri/Cargo.toml`
- 改了公开契约、harness 配置或长期文档：`npm run harness:check`

不要默认运行 `npm run harness:test`、`npm run build`、`cargo test` 或 `cargo fmt`。提交信息使用 `<类型>: <结果>`，例如 `fix: restore asset folder expansion`。提交必须是可回退的完整单元。

默认在 `master` 提交并推送：

```powershell
git push origin master
```

并行任务才推功能分支。需要审查时再开 PR，模板见 `.github/pull_request_template.md`。合并后删除功能分支和对应 worktree。
