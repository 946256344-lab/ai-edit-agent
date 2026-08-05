# API 与工具契约

## 状态

桌面后端已实现本地持久化、素材导入与证据、实验性 OAuth、媒体分析、源时间绑定 storyboard、内部时间线、局部片段替换、preview 和实验性 Jianying Pro 8.0 仅视频草稿创建。`src/lib/agent-tools.ts` 是面向未来通用 Agent 工具层的 TypeScript 目标契约；当前 Tauri 命令直接返回领域结果。

## 已实现的 Tauri 命令

| 命令 | 输入 | 结果 | 说明 |
| --- | --- | --- | --- |
| `initialize_local_store` | 无 | `StoreStatus` | 创建应用数据目录、打开 SQLite、执行迁移、将中断时处于 `working` 的会话恢复为 `ready`，并每进程恢复一次未完成分析任务。 |
| `create_project` | `{ name }` | `StoredProject` | 拒绝空名称。 |
| `list_projects` | 无 | `StoredProject[]` | 按最后更新时间倒序。 |
| `create_editing_task` | `{ projectId, title }` | `StoredEditingTask` | 在既有项目内创建作用域化创作目标。 |
| `list_editing_tasks` | `{ projectId }` | `StoredEditingTask[]` | 按最后更新时间倒序。 |
| `update_editing_task_brief` | `{ editingTaskId, brief }` | `void` | 保存非空 brief；首次请求会为未命名任务定名。 |
| `create_conversation` | `{ projectId, editingTaskId, title }` | `StoredConversation` | 任务必须属于指定项目，拒绝空标题。 |
| `list_conversations` | `{ projectId, editingTaskId? }` | `StoredConversation[]` | 按最后更新时间倒序；可按任务过滤。 |
| `create_message` | `{ conversationId, role, content }` | `StoredMessage` | 保存 user、agent、tool 或 system 消息并更新时间。 |
| `set_conversation_status` | `{ conversationId, status }` | `void` | 状态为 `ready`、`working` 或 `review`。 |
| `list_messages` | `{ conversationId }` | `StoredMessage[]` | 按时间正序。 |
| `import_assets` | `{ projectId, sourceReferences }` | `StoredAsset[]` | 校验本地文件、保存引用并排队分析。 |
| `import_asset_folder` | `{ projectId, sourceDirectory }` | `StoredAsset[]` | 递归登记支持的媒体，并记录文件夹层级根。 |
| `list_assets` | `{ projectId }` | `StoredAsset[]` | 返回文件夹名和相对路径，不向 UI 暴露源引用。 |
| `get_asset_evidence` | `{ assetId }` | `AssetEvidence` | 返回派生关键帧、OCR 和视觉证据。 |
| `generate_storyboard` | `{ projectId, editingTaskId, brief }` | `StoryboardVersion` | 仅以证据作为实验性模型输入，在本地校验后创建任务内版本。 |
| `get_latest_storyboard` | `{ projectId, editingTaskId }` | `StoryboardVersion \| null` | 加载所选任务的最新 storyboard。 |
| `create_timeline_draft` | `{ projectId, storyboardVersionId }` | `TimelineVersion` | 从经验证的 storyboard 创建源时间绑定内部时间线。 |
| `get_latest_timeline` | `{ projectId, storyboardVersionId }` | `LatestTimeline \| null` | 仅加载该 storyboard 的最新时间线及其 preview。 |
| `render_preview` | `{ timelineVersionId }` | `PreviewResult` | 用 FFmpeg 本地渲染 540 x 960 MP4。 |
| `execute_agent_edit` | `{ projectId, editingTaskId, storyboardVersionId, timelineVersionId, request }` | `AgentEditResult` | 实验性模型选择、后端强制作用域的 storyboard、时间线、局部替换、preview 和 Jianying draft 操作。 |
| `get_experimental_openai_oauth_status` | 无 | `ExperimentalOAuthStatus` | 仅从 Windows Credential Manager 读取连接状态。 |
| `start_experimental_openai_oauth` | 无 | `ExperimentalOAuthStart` | 启动五分钟 loopback PKCE 回调并返回浏览器授权 URL；仅个人测试。 |
| `create_jianying_draft` | `{ timelineVersionId }` | `JianyingDraftResult` | 在当前用户配置的 Jianying Pro 8.0 草稿库创建并注册唯一的仅视频草稿。 |

`AgentEditResult` 包含可空的 `storyboard`、`timeline`、`preview` 与 `jianyingDraft`。未限定的中文“创建草稿”会调用 `create_jianying_draft`；用户必须说“内部时间线”或“时间线”才只创建 `create_timeline_draft`。后端会归一化常见精确命令，并仅在动作成功后附加经验证的结果消息。

Tauri 命令以适合展示的字符串错误返回，但不得在错误中暴露凭据或完整媒体路径。

## 会话生命周期

桌面应用直接进入 Agent 会话。首次消息或导入会按需创建项目、剪辑任务和会话。第一条用户消息保存任务 brief 并为未命名任务命名；每条消息更新会话摘要。Agent 请求执行期间 UI 保存 `working`，成功或失败后恢复 `ready`。素材是项目级；storyboard 及其派生产物只在同一剪辑任务中可见。

## 实验性 OAuth

OAuth 命令兼容当前 OpenCode 实现，但不是官方 OpenAI 第三方集成。浏览器回调会在交换令牌前校验 PKCE state。访问和刷新凭据仅保存于 Windows Credential Manager，绝不返回前端、不写入 SQLite、项目文件、日志或工具结果。

实验性 Responses 请求设置 `store: false` 与 `stream: true`。后端累计 SSE 文本后再解析结构化工具或分析 JSON。回调成功、失败或超时后，后端发出 `experimental-openai-oauth-status` 事件；前端同时轮询状态作为恢复路径。

凭据可用时，内部 `analyze_asset` 最多将三张缓存视频关键帧或一张图片缩略图发送到实验性 Responses 端点。结构化输出仅保存为带源时间的视觉证据；请求失败不会使技术分析失败。

## 素材证据与 storyboard

`get_asset_evidence` 只返回派生证据：关键帧缓存路径、可选 `timeMs` 的 OCR 文本和视觉建议。它绝不返回 `source_reference` 或 `folder_reference`；UI 将派生图片路径转换为受限的 Tauri asset URL。

生成 storyboard 必须提交非空的用户 brief。模型输入仅有紧凑的持久化证据：素材 ID、媒体类型、已验证时长、场景片段、OCR 与视觉标签。生成镜头必须含素材 ID 和源范围；视频范围必须在已验证时长内，图片的源范围必须为零。校验失败不会保存版本。

## Agent 目标工具契约

```ts
type ModelProvider = 'openai-oauth' | 'custom-api' | 'local'
type ToolStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled'

type ToolInvocation<TInput, TResult> = {
  id: string
  name: AgentToolName
  status: ToolStatus
  input: TInput
  result?: TResult
  error?: string
  createdAt: string
}
```

此 envelope 是未来通用 Agent 审计层的契约，不表示所有现有 Tauri 命令已经以该形态返回。所有新增的 Agent 副作用应创建可持久化、可审计的调用记录；`result` 只可在 `completed` 时存在，`error` 不得含凭据或不必要的本机路径。

| 工具 | 当前契约 | 实现状态 |
| --- | --- | --- |
| `analyze_assets` | `analyzeAssets(projectId, assetIds)` | 部分实现：导入会创建内部 `analyze_asset` 任务；公开的通用视觉分析工具契约待完成。 |
| `search_media_segments` | 名称保留，输入/结果未定义 | TODO |
| `create_timeline_draft` | `{ projectId, storyboardVersionId }` | 已实现，支持经验证的图片/视频 storyboard 镜头。 |
| `render_preview` | `renderPreview(timelineVersionId)` | 已实现，本地 540 x 960 H.264 preview。 |
| `create_jianying_draft` | `{ timelineVersionId }` | 已实现，创建并注册唯一的 Jianying Pro 8.0 仅视频草稿。 |

## 导入、时间线与 preview 规则

文件夹导入会递归记录支持的视频、图片和音频引用，不复制或修改源文件。`StoredAsset` 仅暴露 `folderName` 与 `relativePath`，不暴露绝对源路径。分析会写入时长、尺寸、帧率、音频、缩略图、关键帧、场景、OCR 和视觉标签计数。资产响应将活动分析任务映射为 `queued` 或 `analyzing`；初始化会恢复未完成任务并取消同一素材的重复任务。桌面 UI 轮询 `list_assets`，在右下角展示活动分析数量和最多三个显示名，任务完成后自动移除；该提示不增加新的 Tauri 命令。

`create_timeline_draft` 的成功结果是按 storyboard 镜头顺序映射的内部时间线版本。多轨音频、字幕、变换和自动化仍为 `TODO`。`replace_timeline_clip` 目前只可经 `execute_agent_edit` 调用：必须替换已有 `shotIndex`，使用同项目已就绪素材，视频源范围须已验证并与原时间线时长严格相同，图片源时间必须均为零。操作会创建新 `TimelineVersion` 并记录前后变化。

preview 渲染使用归一化图片/视频片段和内部 concat 序列，生成本地 540 x 960 H.264 MP4。结果包含黑帧扫描、精确重复源范围、低分辨率视觉相似候选、节奏异常和尚未作为字幕渲染的 storyboard 文本。当前不混音、不绘制字幕、不做多帧语义重复检测，也不提供取消语义。

## Jianying draft 创建规则

`JianyingDraftResult` 会返回草稿目录和内容文件路径，仅供本地桌面流程使用，调用方不得将其记录到日志、浏览器存储或文档。

- 仅创建新目标目录，绝不覆盖已有 Jianying 项目。
- 成功前验证所有媒体引用。
- 更新首页注册表期间 Jianying Pro 必须关闭。
- Assembly Video Agent 跨进程串行化注册表写入；替换前若注册表变化则中止。
- 源文件或配置的草稿库不可用时返回结构化失败。
- 不自动执行最终视频导出。

## 外部契约待定

- 官方 OpenAI OAuth 的授权 URL、scope、令牌刷新和支持的模型能力。
- 其他模型 Provider 适配器 schema。
- 用户提供的 voice API 鉴权、请求体、音色选择、响应和异步任务处理。

## 开发期文档同步 Harness

`npm run harness:check` 不属于桌面应用 API；它是仓库开发期的 Git 变更集检查。规则定义在 `.harness/doc-sync-policy.json`，检查高影响 Tauri 命令、持久化、OAuth/安全和运行时配置变动是否同步更新本文档及其他要求的 Markdown。触发规则时，变更集还必须包含一份 `docs/changes/` 记录。详细的执行与 Agent 审查 loop 见 `docs/harness.md`。
