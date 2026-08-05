# 架构

## 状态

仓库已实现 React/Tauri 桌面基础、SQLite 本地持久化、媒体分析、证据绑定 storyboard、内部时间线、preview 和实验性 Jianying draft 创建。本文件同时描述当前实现与仍待完成的生产能力；标记为 `TODO` 的项目尚未实现或尚未验证。

## 当前组成

```text
React 19 + TypeScript + Vite
|
|- src/App.tsx            工作区组合与展示状态
|- src/lib/local-store.ts Tauri 命令 TypeScript 桥接
|- src/lib/agent-tools.ts Agent 工具的目标契约
`- src-tauri/             Rust 命令、SQLite、媒体工具与 OAuth 边界
```

`App.tsx` 在 Tauri 环境中通过 `local-store.ts` 加载项目、剪辑任务、会话、消息和素材。剪辑任务是项目内的创作目标；会话、storyboard、时间线和 preview 均被限制在该任务内，素材保持项目级复用。首次消息或导入会在需要时创建项目、任务和会话。

Tauri 2 后端提供 SQLite、本地文件/文件夹导入、媒体分析、storyboard、内部时间线、FFmpeg preview 和实验性 Jianying Pro 8.0 仅视频草稿创建。`tauri.conf.json` 使用受限 CSP，仅允许作用域内的本地派生媒体协议。

## 系统边界

```text
Windows 桌面应用（Tauri + React）
|
|- 展示层（已实现）
|  |- Agent 会话、素材库、媒体分析任务提示、证据、storyboard、版本与 preview
|  `- 项目与 Provider 状态
|
|- 本地 Agent 控制器（部分实现）
|  |- 受限工具选择与后端校验
|  |- 会话/任务上下文
|  `- 通用调用持久化、恢复与完整审计 TODO
|
|- 本地工具服务（部分实现）
|  |- 导入、FFprobe/FFmpeg/Tesseract 分析、时间线、preview
|  |- Jianying 仅视频适配器
|  `- 音频、字幕、生产运行时供应、voice Provider TODO
|
|- 模型 Provider（部分实现）
|  |- 实验性 OpenCode 兼容 OAuth/PKCE
|  `- 自定义托管 API、官方 OAuth 验证、本地模型 TODO
|
`- 本地存储（已实现基础）
   |- SQLite：项目、任务、会话、素材、版本、分析任务、操作日志
   `- Windows Credential Manager：实验性 OAuth 凭据
```

## 数据流

```text
导入本地文件或文件夹
  -> SQLite 保存源文件引用
  -> 后台 FFprobe 提取时长、尺寸、帧率和音频轨信息
  -> FFmpeg 生成缩略图、关键帧和启发式场景片段
  -> Tesseract 提取图片/关键帧英文 OCR
  -> 实验性 Provider 最多接收三张派生关键帧或一张缩略图
  -> 保存带源时间的视觉建议
  -> Provider 仅基于持久化证据生成 storyboard，后端验证素材与时间范围
  -> 创建源时间绑定的内部时间线版本
  -> FFmpeg 渲染 540 x 960 本地 preview 并执行质量检查
  -> 可选地创建新的 Jianying Pro 8.0 仅视频草稿
```

视觉建议是 AI 建议，不是经验证的媒体事实。语义相似度、质量评分和多帧重复检测仍为 `TODO`。

## 数据所有权与安全

- 源文件默认仅被引用，列出或使用时检测可用性。缺失素材会保留记录，但不能进入新的 storyboard 或 preview。
- 应用绝不修改源媒体。
- 内部时间线是事实来源；Jianying draft 是单向交付物，不回读用户在 Jianying 中的编辑。
- OAuth 凭据只保存在 Windows Credential Manager，绝不进入 SQLite、浏览器存储、项目文件或日志。
- 模型仅接收获批的精简提示、证据文本和低分辨率派生帧，绝不接收原始媒体或本机路径。

## 当前实现细节

`store.rs` 管理 SQLite 和迁移。当前 schema version 为 3，包含 `projects`、`editing_tasks`、`conversations`、`messages`、`assets`、`storyboard_versions`、`timeline_versions`、`agent_tasks` 与 `operation_logs`。迁移只为确有未作用域化旧数据的项目创建遗留任务；不会删除旧记录。

仓库还包含开发期文档同步 harness。`.harness/doc-sync-policy.json` 将高影响的桌面命令、持久化、Provider/凭据安全和运行时配置路径映射到必须同步的长期 Markdown 文档。`check-doc-sync.mjs` 对 Git 变更集执行硬检查，`.githooks/pre-commit` 检查暂存区；`docs/changes/` 保存可审计的架构变更记录。对于触发规则的工作，独立上下文 Agent 会审查代码 diff、变更记录和文档语义，并在最多三轮修复后给出结果。详见 `docs/harness.md`。

导入后，每个素材会创建 `analyze_asset` 持久化任务。启动时会恢复未完成分析并取消同一素材的重复任务。前端轮询活动项目素材状态，并在右下角显示最多三个正在分析的显示名及任务总数；不展示源路径。生成的缩略图与关键帧位于应用数据目录，通过作用域 Tauri asset 协议展示；UI 不接收或展示原始源路径。Windows 上所有由 `store.rs` 创建的外部命令均使用无控制台窗口标志执行，避免媒体分析或 Jianying 适配器闪现命令行。

实验性 OAuth 使用系统浏览器 loopback PKCE 流程，回调校验 state，并通过原生 Windows `keyring` 后端保存凭据。该流程只用于个人测试，不是官方通用 OpenAI 第三方 OAuth。前端通过 Tauri 事件接收状态，并以轮询作为恢复路径。

自然语言编辑控制器只允许选择 `generate_storyboard`、`create_timeline_draft`、`replace_timeline_clip`、`render_preview`、`create_jianying_draft` 或 `no_action`。后端验证任务、storyboard、时间线、素材和时间范围的关联。`replace_timeline_clip` 只能替换既有 `shotIndex`，并产生新时间线版本和操作日志。中文中未限定的“创建草稿”指 Jianying draft；“内部时间线”或“时间线”才只创建本地时间线。控制器不能删除素材或执行最终视频导出。

Jianying 适配器在 Rust 中预校验所有源引用，将版本化 JSON 输入写到应用数据目录后交给 Python 适配器，并在执行后删除输入文件。适配器只支持源时间绑定的视频片段，创建唯一目录，跨进程串行化注册表写入，并在 Jianying Pro 运行或注册表快照变化时中止。

## 技术约束

- FFmpeg/FFprobe、Tesseract（英文 `eng` 数据）、Python 与 `pyJianYingDraft` 是当前开发机依赖，尚未随生产安装包分发。
- Jianying Pro 8.0 仅视频草稿已人工验证能在首页出现并以完整视频片段打开；图片、文本和音频轨道尚不支持。
- `App.tsx` 仍较大；在新增可复用领域功能时应继续将类型、组件和服务拆出。
