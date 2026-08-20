# Assembly Video Agent

面向 Windows 的本地优先 AI 视频剪辑 Agent 原型。用户通过自然语言协作，Agent 将媒体分析、storyboard、内部时间线、低清 preview 和 Jianying draft 创建作为受控本地工具执行。

## 当前实现

- Tauri 2 桌面应用，使用 SQLite 持久化本地项目、剪辑任务、会话、消息、素材、storyboard 和时间线版本。
- 原生文件和文件夹导入；保存源媒体引用，不复制或修改原文件。
- 基于 FFprobe、FFmpeg 和 Tesseract 的本地技术分析、缩略图、关键帧拼接网格、OCR 证据。关键帧提取使用固定时间采样（第 1 秒、1/3、2/3、最后 1 秒），覆盖整个视频，拼接为 2×2 网格图供多模态选镜使用。
- 实验性 Provider 最小帧视觉分析、证据校验后的 storyboard 生成，以及受限的自然语言编辑工具选择。
- 源时间绑定的内部时间线、540 x 960 本地 FFmpeg preview 和质量检查。
- 实验性的 OpenCode 兼容 OAuth PKCE 登录；凭据仅存储于 Windows Credential Manager。
- 已人工验证的 Jianying Pro 8.0 仅视频草稿创建、注册和打开。
- 非显式自然语言请求由模型在受控工具中逐步决策；模型可请求分析项目内已导入但未分析的素材，但不能直接执行文件、SQLite 或 FFmpeg 操作。storyboard 的镜头数和时长由模型提案，应用只保留本地处理安全上限。
- 对话请求统一按 SQLite 时间顺序发送真实 user/assistant 会话消息进入 NativeToolLoop；只读请求使用观察工具，非只读请求按 RequestToolPolicy 暴露获授权的原生工具，Legacy JSON decision/Router 路径已移除。
- NativeToolLoop 不声明固定单一目标：有原生工具调用就执行并继续，没有调用且有自然语言就结束；任务完成状态仍只来自真实工具收据和持久化产物。

这不是生产就绪的 Agent 编排系统。自定义模型适配器、生产安装包中的媒体运行时、多轨音频/字幕、最终视频导出和从 Jianying 反向同步尚未实现。

## 运行

```powershell
npm install
npm run tauri:dev
```

`npm run dev` 仅用于浏览器 UI 检查，不能访问本地项目、媒体工具或模型凭据，不能作为剪辑模式使用。

Tauri 脚本会在进程 `PATH` 中加入当前用户的 Rust 安装目录，无需将 Cargo 写入系统全局 `PATH`。

## 桌面环境依赖

开发环境需要 Node.js、Rust/Cargo、Visual Studio 2022 C++ Build Tools、FFmpeg/FFprobe、Tesseract（含英文 `eng` 语言数据）、Python 和 `pyJianYingDraft`。当前安装包不会捆绑 FFmpeg、Tesseract、Python 或 Jianying 适配器依赖；生产安装、发现与报错策略仍待实现。

`pyJianYingDraft` 适配器要求通过本地 `py` Python launcher 可调用。更新 Jianying 的首页草稿注册表时，Jianying Pro 必须保持关闭。

## 数据与安全边界

- OAuth 凭据仅保存到 Windows Credential Manager，绝不进入浏览器存储、SQLite、项目数据或日志。
- 原始媒体、项目数据、preview 和 Jianying draft 默认留在本机。
- `create_jianying_draft` 只创建唯一的新草稿目录，绝不覆盖已有 Jianying 项目。
- Jianying draft 是单向交付物；内部时间线才是本产品的事实来源。
- 最终视频导出、覆盖既有导出和删除项目、素材或版本必须先获得明确确认；最终视频导出目前尚未实现。

## 文档

- `CONTRIBUTING.md`：所有人类与编码 Agent 共用的分支、验证、提交和 PR 流程。
- `docs/architecture.md`：现有架构、数据流和技术约束。
- `docs/decisions.md`：架构决策记录（ADR）。
- `docs/api.md`：已实现的 Tauri 命令和 Agent 工具契约。
- `docs/roadmap.md`：里程碑和未实现能力。
- `TASKS.md`：当前可执行任务与待决问题。
- `docs/harness.md`：架构改动与文档同步的检查规则和 Agent 审查 loop。
- `docs/audits/`：只读媒体事实审计报告（timeline、素材范围和 preview 渲染校验）。
- `docs/codebase/`：面向 IDE 阅读和新成员上手的七份源码地图；建议从 `STRUCTURE.md` 和 `ARCHITECTURE.md` 开始。

### AI Agent 接手顺序

1. 读根 `AGENTS.md`、`CONTRIBUTING.md` 与 `TASKS.md` 的 `ACTIVE_TASKS` 当前窗口。
2. 修改前端时读 `src/AGENTS.md`；修改 Rust 时读 `src-tauri/src/AGENTS.md`。
3. 从 `docs/codebase/STRUCTURE.md` 定位代码，再按根指令的路由只加载相关长期文档。
4. 修改前运行 `npm run agent:check` 确认基线，修改后运行范围测试和 `npm run harness:check`。

Cursor、Claude Code 和 OpenCode 分别通过 `.cursor/rules/project-workflow.mdc`、`CLAUDE.md` 和 `opencode.json` 加载同一组权威文件；这些入口不保存第二份流程。并行任务采用“一任务一分支一 worktree”，禁止直接在 `master`/`main` 提交。完整命令见 `CONTRIBUTING.md`。

这能让另一个编码 Agent 高可靠接手已提交、任务窗口明确的工作，但不是仅靠文档保证的“无缝记忆迁移”。交接时还必须保留干净或有说明的 Git 状态、准确的当前目标、未决问题、变更记录和可复现测试结果。

## 文档同步 Harness

首次建立 Git 基线后，运行以下命令启用并验证提交前的文档同步检查：

```powershell
npm run harness:install
npm run branch:check
npm run architecture:check
npm run agent:check
npm run harness:check
```

`architecture:check` 会阻止 `App.tsx`、工作区组件、领域 controller、命令桥接和当前 Rust 热点超过只降不升的结构预算；`agent:check` 会核对分层指令、当前任务窗口、源码顶部中文导航、IPC/命令/API、Rust 外部边界和 Agent 工具目录；`harness:check` 汇总这些硬门，并检查高影响架构改动是否在同一 Git 变更集更新对应文档和 `docs/changes/` 记录。详细规则见 `docs/harness.md`。

所有手写 Rust、TypeScript/React、Node、Python、HTML、Shell 与 CSS 源码在文件顶部都有中文职责导航；权限、事务、恢复、外部进程和非直观算法再补就地中文解释。注释用于帮助在 IDE 中沿真实调用链学习，不逐行翻译明显语法，也不能代替类型、测试和后端校验。

真实桌面前端回归可在以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` 启动 Tauri dev 后运行 `npm run tauri:verify`。脚本只切换工作区、开合目录和打开/关闭 Provider 弹窗，不发送 Agent 请求，不导入、生成或交付产物。

维护记录（2026-08-15）：agentloop 与 taskrouter 路由验证新增 validate-then-correct 重试；fast_goal 降级为提示；Agent run 超时从 90 s 提升至 300 s。
维护记录（2026-08-16）：移除后端 Rust 所有静默 fallback，错误路径改为输出真实原因；见 ADR-065 与 docs/changes/2026-08-16-remove-silent-fallbacks.md。
维护记录（2026-08-17）：agentloop.rs 分层重构完成；路由/执行/提示/纯类型分入 agentloop/{runtime,skills,prompt,schema}.rs；check-agent-contracts.mjs 扩展扫描 runtime.rs。
维护记录（2026-08-19）：Provider 原生工具调用统一为 ModelTurn/ModelOutputItem/FunctionCall，并由 NativeToolLoop 消费；非 Native 的 storyboard/视觉请求仍保留旧 JSON 提取接口。
维护记录（2026-08-19）：NativeToolLoop 已移除前置对话 Router，统一处理普通聊天、澄清、项目事实和工具执行；原生 loop 提供观察、主链、文本、音乐与 Jianying 工具，RequestToolPolicy、确认门、作用域、超时和审计边界保持不变。
维护记录（2026-08-19）：NativeToolLoop 已移除固定 LoopGoal 与 finish/done/no_action 控制动作；复合请求可跨多个授权工具，超时和步骤上限按真实 RunReceipt 保留部分产物。
维护记录（2026-08-20）：修复会话隔离 bug（agentloop/prompt.rs 查询新增 editing_task_id 过滤），防止跨会话数据泄漏；见 docs/changes/2026-08-20-fix-session-isolation-message-history.md。
