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

## 主会话与执行会话

多任务并行时，由一个主会话管方向和验收，代码交给执行会话。主会话按证据验收，不读代码；上下文里只放任务、证据和决定。长期状态存在 `TASKS.md` 和 `docs/decisions.md`，主会话变长时直接开新的，读这两份文件接上。

| 角色 | 做什么 | 不做什么 |
|---|---|---|
| 主会话 | 定方向；写任务书；按证据验收；维护 `TASKS.md` 和 `docs/decisions.md`；整理待桌面确认清单 | 写代码；读分支代码 |
| 执行会话 | 一个会话一个任务，在自己的 worktree 和分支上实现、自查、按格式回报；验收通过后合并回 `master` | 改产品方向；扩大范围 |
| 审查会话（按需） | 读分支 diff，只交回问题清单 | 改代码 |

- 主会话写任务书前需要了解现状时，只看 `master`，或派只读子 Agent 带回总结。
- 同时最多 2–3 个执行会话，任务之间不碰同一批文件；热点文件同一时间只交给一个会话。
- 执行会话的回报不算事实，主会话要按任务书里的验收标准逐条核对证据。桌面效果以用户实测为准，未实测的在 `TASKS.md` 标「待桌面确认」。
- 验收通过后，由执行会话自己同步 `master`、处理冲突、推送，然后删除分支和 worktree。打回时，主会话写明哪条验收没过。

任务书模板：

```markdown
## 任务：<一句话结果>
- 为什么：<解决什么问题；是否让策划或选镜更好>
- 验收标准：<写成可观察的行为，不写改哪个函数>
- 范围：<可能涉及的目录或模块>
- 不做：<明确排除的内容>
- 分支：<类型>/<简短主题>
```

回报模板：

```markdown
## 回报：<任务名>
- 结果：完成 / 部分完成 / 受阻（原因）
- 改动：<用行为描述，另附提交号>
- 验证：<跑了什么命令、实际结果；手动验证了什么>
- 未验证：<哪些没验证、为什么，需用户桌面确认的点>
- 文档：<同步了哪些文档，或「无」>
- 新发现：<范围外的问题，已记入 TASKS.md 或建议记入>
```

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
