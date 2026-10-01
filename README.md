# Voycut

面向 Windows 的本地优先 AI 视频剪辑 Agent 原型。用户通过自然语言协作，Agent 将媒体分析、storyboard、内部时间线、低清 preview 和编辑器交付作为受控本地工具执行。Voycut 是对外展示名称；现有应用标识、数据库文件名和凭据服务名保持不变，以读取本机原有数据。

## 当前实现

- Tauri 2 / React 19 桌面壳，SQLite 保存本地项目、独立剪辑会话及故事版/时间线版本；共享子素材库复用媒体引用与分析结果。
- 文件/文件夹导入后，以 FFprobe/FFmpeg/Tesseract 做技术、硬切段、运动可用窗与 OCR 分析；画面识别按真实片段多帧送审，本地 BGE/CLIP 辅助召回。
- 自然语言进入统一 NativeToolLoop；当前按 brief 策划拆拍、本地召回、看图选镜、锁窗精修和校验。局部换镜/改区间生成新版本；素材先行策划与体裁分流仍待重做。
- 画幅支持 9:16、16:9、1:1；本地低清 preview 可含字幕、旁白、BGM、品牌图层和转场，实际落地以工具收据为准。
- 编辑器输出可选剪映、CapCut、FCPXML、OTIO，只创建新交付物。内置网关构建用 Voycut 登录与模型/配音服务；无网关开发构建保留自定义兼容 API、实验性 OAuth 和本机配音配置。

实现细节见 [架构](docs/architecture.md) 和 [命令与工具契约](docs/api.md)。当前仍是原型：真实 ASR 与最终视频导出未实现，编辑器导入/播放及桌面主路径待实测，见 [首发验收清单](docs/release-checklist.md)。

## 运行

```powershell
npm install
npm run tauri:dev
```

`npm run tauri:dev` 会在 debug 进程中开启 Native Provider JSONL 转储。完整请求/响应写入 `src-tauri/target/native-provider-full-trace.jsonl`，不进入界面；release 构建即使设置同名变量也保持关闭。

`npm run dev` 仅用于浏览器 UI 检查，不能访问本地项目、媒体工具或模型凭据，不能作为剪辑模式使用。

Tauri 脚本会在进程 `PATH` 中加入当前用户的 Rust 安装目录，无需将 Cargo 写入系统全局 `PATH`。

## 桌面环境依赖

开发环境需要 Node.js、Rust/Cargo、Visual Studio 2022 C++ Build Tools。正式安装包构建前必须设置环境变量 `FELLOWCUT_GATEWAY_BASE_URL=https://<站点>/api/model`（编译进二进制，缺失或格式不对时 `npm run tauri:build` 直接失败；`--debug` 构建不检查）。正式安装包构建会自动拉取并捆绑 FFmpeg/FFprobe、embeddable Python 3.12 与 `pyJianYingDraft`/`pycapcut`，以及 Tesseract 5.4.0 与英文 `eng` 数据；开发机也可分别运行 `npm run ffmpeg:fetch`、`npm run python:fetch`、`npm run tesseract:fetch`。安装包捆绑 ONNX Runtime、DirectML 与模型小配置，**默认不捆绑** BGE/CLIP 的 `model.onnx` 大文件：首次启动后由应用后台下载（官方 + 国内镜像、断点续传）到本机数据目录并校验。需要离线开箱可用时，先 `npm run models:fetch`，再 `npm run tauri:build:full` 打完整包。验证正式运行时：先 `npm run tauri:build -- -b nsis`，再运行 `npm run ffmpeg:verify`、`npm run python:verify`、`npm run tesseract:verify`；Release 应用 IPC 验证使用 `node scripts/verify-release-python-app.mjs`。

剪映适配器优先调用随包 `python.exe`，不要把 `py -3` 传给 embeddable 解释器。更新 Jianying 的首页草稿注册表时，Jianying Pro 必须保持关闭。

## 数据与安全边界

- OAuth 凭据仅保存到 Windows Credential Manager，绝不进入浏览器存储、SQLite、项目数据或日志。
- 原始媒体、项目数据、preview 和编辑器交付物默认留在本机。
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
