# Voycut

面向 Windows 的本地优先 AI 视频剪辑 Agent 原型。用户通过自然语言协作，Agent 将媒体分析、storyboard、内部时间线、低清 preview 和 Jianying draft 创建作为受控本地工具执行。Voycut 是对外展示名称；现有应用标识、数据库文件名和凭据服务名保持不变，以读取本机原有数据。

## 当前实现

- Tauri 2 桌面应用，使用 SQLite 持久化本地项目、剪辑任务、会话、消息、素材、storyboard 和时间线版本。
- 原生文件和文件夹导入；保存源媒体引用，不复制或修改原文件。
- 基于 FFprobe、FFmpeg 和 Tesseract 的本地技术分析、缩略图、关键帧拼接网格、OCR 证据。关键帧提取使用固定时间采样（第 1 秒、1/3、2/3、最后 1 秒），覆盖整个视频，拼接为 2×2 网格图供多模态选镜使用。
- 关键帧本地计算清晰度质量分；`bge-small-zh-v1.5` 权重安装后由应用下载（或开发机自备），以视觉证据文本做离线语义召回，每个 beat 向模型提供最多 12 个候选并由模型选择 1 个。向量不可用时回退词面排序。
- 实验性 Provider 最小帧视觉分析、证据校验后的 storyboard 生成，以及受限的自然语言编辑工具选择。
- 源时间绑定的内部时间线、540 x 960 本地 FFmpeg preview 和质量检查。明确旁白文案可通过 ElevenLabs 合成配音；密钥只进 Windows Credential Manager。
- 实验性的 OpenCode 兼容 OAuth PKCE 登录；凭据仅存储于 Windows Credential Manager。
- 已人工验证的 Jianying Pro 8.0 仅视频草稿创建、注册和打开。
- 非显式自然语言请求由模型在受控工具中逐步决策；模型可请求分析项目内已导入但未分析的素材，但不能直接执行文件、SQLite 或 FFmpeg 操作。storyboard 的镜头数和时长由模型提案，应用只保留本地处理安全上限。
- 对话请求统一按 SQLite 时间顺序发送真实 user/assistant 会话消息进入 NativeToolLoop；只读请求使用观察工具，非只读请求按 RequestToolPolicy 暴露获授权的原生工具，Legacy JSON decision/Router 路径已移除。
- NativeToolLoop 不声明固定单一目标：有原生工具调用就执行并继续，没有调用且有自然语言就结束；任务完成状态仍只来自真实工具收据和持久化产物。
- 每轮 NativeToolLoop 在系统提示后注入不含路径、ID、证据正文或凭据值的本地权威状态快照；写工具成功后刷新，长上下文裁剪始终保护该块。
- 工具结果进入下一次模型总结请求后，瞬时 Provider 传输失败会在原单步/总预算内重试；重试只重发模型请求，不会重复执行已经完成的本地工具。
- 开发构建可用显式 `NATIVE_PROVIDER_FULL_TRACE=1` 把 NativeToolLoop 每次 HTTP 的完整请求 JSON 和原始响应写入 `src-tauri/target/native-provider-full-trace.jsonl`；不进前端，release 构建不可用。

这不是生产就绪的 Agent 编排系统。自定义模型适配器、多轨音频/字幕、最终视频导出和从 Jianying 反向同步尚未实现。

## 运行

```powershell
npm install
npm run tauri:dev
```

`npm run tauri:dev` 会在 debug 进程中开启 Native Provider JSONL 转储。完整请求/响应写入 `src-tauri/target/native-provider-full-trace.jsonl`，不进入界面；release 构建即使设置同名变量也保持关闭。

`npm run dev` 仅用于浏览器 UI 检查，不能访问本地项目、媒体工具或模型凭据，不能作为剪辑模式使用。

Tauri 脚本会在进程 `PATH` 中加入当前用户的 Rust 安装目录，无需将 Cargo 写入系统全局 `PATH`。

## 桌面环境依赖

开发环境需要 Node.js、Rust/Cargo、Visual Studio 2022 C++ Build Tools。正式安装包构建前必须设置环境变量 `FELLOWCUT_GATEWAY_BASE_URL=https://<站点>/api/model`（编译进二进制，缺失或格式不对时 `npm run tauri:build` 直接失败；`--debug` 构建不检查）。正式安装包构建会自动拉取并捆绑 FFmpeg/FFprobe、embeddable Python 3.12 与 `pyJianYingDraft`/`pycapcut`，以及 Tesseract 5.4.0 与英文 `eng` 数据；开发机也可分别运行 `npm run ffmpeg:fetch`、`npm run python:fetch`、`npm run tesseract:fetch`。安装包捆绑 ONNX Runtime 与模型小配置，**默认不捆绑** BGE/CLIP 的 `model.onnx` 大文件：首次启动后由应用后台下载（官方 + 国内镜像、断点续传）到本机数据目录并校验。需要离线开箱可用时，先 `npm run models:fetch`，再 `npm run tauri:build:full` 打完整包。验证正式运行时：先 `npm run tauri:build -- -b nsis`，再运行 `npm run ffmpeg:verify`、`npm run python:verify`、`npm run tesseract:verify`；Release 应用 IPC 验证使用 `node scripts/verify-release-python-app.mjs`。

剪映适配器优先调用随包 `python.exe`，不要把 `py -3` 传给 embeddable 解释器。更新 Jianying 的首页草稿注册表时，Jianying Pro 必须保持关闭。

## 数据与安全边界

- OAuth 凭据仅保存到 Windows Credential Manager，绝不进入浏览器存储、SQLite、项目数据或日志。
- 原始媒体、项目数据、preview 和 Jianying draft 默认留在本机。
- `create_jianying_draft` 只创建唯一的新草稿目录，绝不覆盖已有 Jianying 项目。
- Jianying draft 是单向交付物；内部时间线才是本产品的事实来源。
- 最终视频导出、覆盖既有导出和删除项目、素材或版本必须先获得明确确认；最终视频导出目前尚未实现。

## 文档

每份文档只回答一个问题：

| 问题 | 文档 |
|---|---|
| 规则怎么守 | `AGENTS.md`（入口与产品底线）、`CONTRIBUTING.md`（协作、验证、提交） |
| 现在做什么 | `TASKS.md`：只放未完成的事 |
| 首发还差什么、哪些改动待桌面确认 | `docs/release-checklist.md` |
| 为什么这么定 | `docs/decisions.md`：只放仍然有效的决定 |
| 系统现在长什么样 | `docs/architecture.md`（产品链路）、`docs/api.md`（Tauri 命令与工具契约）、`docs/codebase/`（代码地图，从 `STRUCTURE.md` 开始） |
| 过去发生了什么 | `docs/changes/`（按日期的变更记录）、`docs/audits/`（媒体事实审计）、git 记录 |
| 检查怎么跑 | `docs/harness.md` |

编码 Agent 从根 `AGENTS.md` 进入；Cursor、Claude Code 和 OpenCode 分别通过 `.cursor/rules/project-workflow.mdc`、`CLAUDE.md` 和 `opencode.json` 加载同一组权威文件，这些入口不保存第二份流程。

## 文档同步 Harness

首次建立 Git 基线后，运行以下命令启用并验证提交前的文档同步检查：

```powershell
npm run harness:install
npm run harness:check
```

`architecture:check` 会阻止 `App.tsx`、工作区组件、领域 controller、命令桥接和当前 Rust 热点超过只降不升的结构预算；`agent:check` 会核对分层指令、当前任务窗口、源码顶部中文导航、IPC/命令/API、Rust 外部边界和 Agent 工具目录；`harness:check` 汇总这些硬门，并检查高影响架构改动是否在同一 Git 变更集更新对应文档和 `docs/changes/` 记录。详细规则见 `docs/harness.md`。

所有手写 Rust、TypeScript/React、Node、Python、HTML、Shell 与 CSS 源码在文件顶部都有中文职责导航；权限、事务、恢复、外部进程和非直观算法再补就地中文解释。注释用于帮助在 IDE 中沿真实调用链学习，不逐行翻译明显语法，也不能代替类型、测试和后端校验。

真实桌面前端回归可在以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` 启动 Tauri dev 后运行 `npm run tauri:verify`。脚本只切换工作区、开合目录和打开/关闭 Provider 弹窗，不发送 Agent 请求，不导入、生成或交付产物。
