# Assembly Video Agent

面向 Windows 的本地优先 AI 视频剪辑 Agent 原型。用户通过自然语言协作，Agent 将媒体分析、storyboard、内部时间线、低清 preview 和 Jianying draft 创建作为受控本地工具执行。

## 当前实现

- Tauri 2 桌面应用，使用 SQLite 持久化本地项目、剪辑任务、会话、消息、素材、storyboard 和时间线版本。
- 原生文件和文件夹导入；保存源媒体引用，不复制或修改原文件。
- 基于 FFprobe、FFmpeg 和 Tesseract 的本地技术分析、缩略图、关键帧、场景候选和英文 OCR 证据。
- 实验性 Provider 最小帧视觉分析、证据校验后的 storyboard 生成，以及受限的自然语言编辑工具选择。
- 源时间绑定的内部时间线、540 x 960 本地 FFmpeg preview 和质量检查。
- 实验性的 OpenCode 兼容 OAuth PKCE 登录；凭据仅存储于 Windows Credential Manager。
- 已人工验证的 Jianying Pro 8.0 仅视频草稿创建、注册和打开。

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

- `docs/architecture.md`：现有架构、数据流和技术约束。
- `docs/decisions.md`：架构决策记录（ADR）。
- `docs/api.md`：已实现的 Tauri 命令和 Agent 工具契约。
- `docs/roadmap.md`：里程碑和未实现能力。
- `TASKS.md`：当前可执行任务与待决问题。
- `docs/harness.md`：架构改动与文档同步的检查规则和 Agent 审查 loop。

## 文档同步 Harness

首次建立 Git 基线后，运行以下命令启用并验证提交前的文档同步检查：

```powershell
npm run harness:install
npm run harness:check
```

高影响的架构改动必须在同一 Git 变更集更新对应文档和 `docs/changes/` 记录。详细规则见 `docs/harness.md`。
