# API 与工具契约

## 状态

桌面后端已实现本地持久化、素材导入与证据、实验性 OAuth、媒体分析、源时间绑定 storyboard、内部时间线、批量片段替换、改时长、排序、澄清反问、preview 和实验性 Jianying Pro 8.0 仅视频草稿创建。`src/lib/agent-tools.ts` 是面向未来通用 Agent 工具层的 TypeScript 目标契约；当前 Tauri 命令直接返回领域结果。

## 已实现的 Tauri 命令

| 命令 | 输入 | 结果 | 说明 |
| --- | --- | --- | --- |
| `initialize_local_store` | 无 | `StoreStatus` | 创建应用数据目录、打开 SQLite（WAL + busy_timeout）、执行迁移；中断且存在未完成通用 Agent 调用的会话恢复为 `review`，其余 `working` 会话恢复为 `ready`，并每进程恢复一次未完成分析任务（只立即处理前 4 条，其余保持 `queued`），且对状态为 `queued`/`analyzing` 但没有对应 `analyze_asset` 任务的孤立素材补建并排队分析。 |
| `create_project` | `{ name }` | `StoredProject` | 拒绝空名称。 |
| `list_projects` | 无 | `StoredProject[]` | 按最后更新时间倒序。 |
| `create_editing_task` | `{ projectId, title }` | `StoredEditingTask` | 在既有项目内创建作用域化创作目标。 |
| `list_editing_tasks` | `{ projectId }` | `StoredEditingTask[]` | 按最后更新时间倒序。 |
| `update_editing_task_brief` | `{ editingTaskId, brief }` | `void` | 保存非空 brief；首次请求会为未命名任务定名。 |
| `create_conversation` | `{ projectId, editingTaskId, title }` | `StoredConversation` | 任务必须属于指定项目，拒绝空标题。 |
| `list_conversations` | `{ projectId, editingTaskId? }` | `StoredConversation[]` | 按最后更新时间倒序；可按任务过滤。 |
| `create_message` | `{ conversationId, role, content, routeReceipt? }` | `StoredMessage` | 保存消息并更新时间；`role=user` 必须提供与目标 conversation 和完整 content 匹配、仍未消费的 route receipt，其他角色不需要。 |
| `set_conversation_status` | `{ conversationId, status }` | `void` | 状态为 `ready`、`working` 或 `review`。 |
| `list_messages` | `{ conversationId }` | `StoredMessage[]` | 按时间正序。 |
| `resolve_conversation_task` | `{ projectId, activeEditingTaskId?, request }` | `TaskRouteResult` | 在消息持久化前解析项目内任务归属；候选为最近 12 个任务并始终包含仍属于该项目的显式活动任务。返回继续当前任务、切换已有任务、原子创建新任务或澄清。确定目标时签发一次性 route receipt；只选择任务，不选择 Agent 工具。 |
| `import_assets` | `{ projectId, sourceReferences }` | `StoredAsset[]` | 校验本地文件、保存引用并排队分析。 |
| `import_asset_folder` | `{ projectId, sourceDirectory }` | `StoredAsset[]` | 递归登记支持的媒体，并记录文件夹层级根。 |
| `preview_asset_relink` | `{ projectId, sourceDirectory }` | `{ matches, unmatchedCount }` | 扫描用户选定的新根目录，仅按唯一的原相对路径与媒体类型给出可确认匹配；不修改项目。 |
| `confirm_asset_relink` | `{ projectId, sourceDirectory, assetIds, preserveAnalysis }` | `{ relinkedCount }` | 重新计算已预览的唯一匹配后，才更新所选素材源引用。`preserveAnalysis=true` 仅更新路径并保留已有分析证据；`false` 时清除旧分析证据、取消旧 active 分析任务并按有界批次重排分析。 |
| `start_asset_health_scan` | `{ projectId }` | `{ taskId }` | 显式启动可取消的后台源文件元数据检查；已有活动扫描时返回同一任务。 |
| `cancel_asset_health_scan` | `{ projectId, taskId }` | `void` | 取消当前项目仍在排队或运行的健康扫描。 |
| `get_asset_health_scan_summary` | `{ projectId }` | `AssetHealthScanSummary` | 读取持久化健康计数与活动任务进度，不访问源文件。 |
| `preview_collect_project_media` | `{ projectId }` | `{ collectableCount, unavailableCount, totalBytes }` | 用户发起收集前逐项复核源文件并估算复制量；不写文件。 |
| `collect_project_media` | `{ projectId, destinationDirectory }` | `{ copiedCount, unavailableCount, outputDirectory }` | 在用户选择目录下创建 UUID 命名的新包，复制当前可读源文件并写无原路径 manifest；不覆盖已有文件、不改写项目引用，操作日志只记录计数。 |
| `list_assets` | `{ projectId }` | `StoredAsset[]` | 返回文件夹名、相对路径、技术分析状态、独立视觉分析状态和安全证据计数，不向 UI 暴露源引用或即时源文件探测结果；若该项目仍有 `queued` 分析任务且没有正在运行的分析 worker，顺带排空至多 4 条。实际分析和交付工具使用素材前会验证源文件。 |
| `list_asset_page` | `{ projectId, search?, kind?, analysisStatus?, visualStatus?, folderName?, offset, limit }` | `AssetPage` | 面向素材库 UI 的有界分页查询，`limit` 强制为 1–200；搜索、类型、技术状态、视觉状态、storyboard 可用性和文件夹条件在 SQLite 执行。返回当前页摘要、匹配总数、项目级技术状态计数和安全文件夹名，不返回源引用。 |
| `get_asset_task_center` | `{ projectId }` | `AssetTaskCenter` | 返回项目级技术/视觉任务的排队、运行、失败、跳过计数，以及最多 50 条只含安全原因码的最近失败；不返回后台错误原文、路径或媒体证据。 |
| `retry_asset_analysis_batch` | `{ projectId, assetIds }` | `BatchAssetActionResult` | 用户批量重试技术分析，每次最多 200 条；只处理当前项目、源文件仍可用且未 ready/active 的素材，活动任务不重复创建，并写入用户操作审计。 |
| `skip_asset_visual_analysis_batch` | `{ projectId, assetIds }` | `BatchAssetActionResult` | 用户明确确认后批量跳过视觉分析，每次最多 200 条；仅修改当前项目技术 `ready` 的图片/视频，保留技术证据、清除视觉标签并写入用户操作审计。在途视觉批次不得覆盖显式用户跳过。 |
| `update_asset_user_metadata_batch` | `{ projectId, assetIds, favorite?, rating?, note?, excluded? }` | `BatchAssetActionResult` | 批量设置收藏、0–5 评分、最多 2000 字符备注和禁止使用；用户字段与分析证据分表保存，审计不保存正文。 |
| `add_asset_tag_batch` / `remove_asset_tag_batch` | `{ projectId, assetIds, tag }` | `BatchAssetActionResult` | 增删项目内不区分大小写的 1–64 字符用户标签。 |
| `create_asset_collection` / `list_asset_collections` / `add_assets_to_collection` | 项目、集合及素材标识 | `AssetCollection` / `AssetCollection[]` / `BatchAssetActionResult` | 创建并查询项目内集合、将最多 200 条当前项目素材加入集合；集合不移动源媒体。 |
| `get_asset_evidence` | `{ assetId }` | `AssetEvidence` | 返回派生关键帧、OCR、视觉证据、`durationMs` 和独立 `visualAnalysisStatus`；视觉分析失败或跳过时返回 `visualAnalysisNote` 说明原因。 |
| `generate_storyboard` | `{ projectId, editingTaskId, brief }` | `StoryboardVersion` | 仅以证据作为实验性模型输入，在本地校验后创建任务内版本。 |
| `get_latest_storyboard` | `{ projectId, editingTaskId }` | `StoryboardVersion \| null` | 加载所选任务的最新 storyboard。 |
| `create_timeline_draft` | `{ projectId, storyboardVersionId }` | `TimelineVersion` | 从经验证的 storyboard 创建源时间绑定内部时间线。 |
| `get_latest_timeline` | `{ projectId, storyboardVersionId }` | `LatestTimeline \| null` | 仅加载该 storyboard 的最新时间线及其 preview。 |
| `list_timeline_versions` | `{ projectId, editingTaskId, storyboardVersionId }` | `TimelineVersion[]` | 返回同一项目、剪辑任务与 storyboard 内的时间线版本，按版本号倒序。 |
| `list_agent_tasks` | `{ projectId, editingTaskId, conversationId? }` | `AgentTask[]` | 返回作用域内的持久化 Agent 调用，按更新时间倒序。 |
| `list_agent_run_steps` | `{ projectId, editingTaskId, agentTaskId }` | `AgentRunStep[]` | 仅在项目、剪辑任务和调用三重作用域匹配时返回步骤；不包含参数、模型原文、对话或媒体证据。 |
| `list_operation_logs` | `{ projectId, editingTaskId, agentTaskId? }` | `OperationLog[]` | 返回作用域内的副作用审计记录，按创建时间倒序。 |
| `render_preview` | `{ timelineVersionId }` | `PreviewResult` | 用 FFmpeg 本地渲染 540 x 960 MP4。 |
| `execute_agent_edit` | `{ projectId, editingTaskId, conversationId, storyboardVersionId, timelineVersionId, request, routeReceipt }` | `String`（任务 ID） | 兼容入口；必须消费与项目、task、conversation、请求完全匹配的一次性 route receipt，随后才可启动异步 Agent run。 |
| `submit_conversation_turn` | `{ projectId, editingTaskId, conversationId, storyboardVersionId, timelineVersionId, request, routeReceipt }` | `{ kind: 'immediate', status, message }` 或 `{ kind: 'run', agentTaskId }` | Conversation Router 入口。后端先消费一次性 route receipt，防止绕过 Task Resolver；普通回复和只读状态不创建 `agent_tasks`，执行型首轮决策创建异步 Agent run。异步 run 终态先幂等写入原 conversation，再发出 `agent-edit-completed`。对一般问答，`route=respond` 可直接返回自然语言答案；若路由判定或模型选择失败，后端现在会回退到 Agent run，而不是用“无法判断真实状态”的固定拒答卡住基础问答。前端会在对话工作区显示当前路由状态文本、细节和语气，用于解释当前请求是已归属、需澄清还是将创建新任务。 |
| `get_experimental_openai_oauth_status` | 无 | `ExperimentalOAuthStatus` | 仅从 Windows Credential Manager 读取连接状态。 |
| `start_experimental_openai_oauth` | 无 | `ExperimentalOAuthStart` | 启动五分钟 loopback PKCE 回调并返回浏览器授权 URL；仅个人测试。 |
| `clear_experimental_openai_oauth` | 无 | `ExperimentalOAuthStatus` | 删除 Windows Credential Manager 中的实验性凭据并重置连接状态。 |
| `get_custom_api_status` | 无 | `CustomApiStatus` | 仅返回自定义 API 的 Base URL、主 Model、可选粗视觉 Model；不返回 API Key。 |
| `save_custom_api` | `{ baseUrl, model, coarseVisualModel?, apiKey }` | `CustomApiStatus` | 保存于 Windows Credential Manager；粗视觉 Model 为空时沿用主 Model。 |
| `clear_custom_api` | 无 | `CustomApiStatus` | 删除 Windows Credential Manager 中的自定义 API 凭据并重置状态。 |
| `create_jianying_draft` | `{ timelineVersionId }` | `JianyingDraftResult` | 在当前用户配置的 Jianying Pro 8.0 草稿库创建并注册唯一的仅视频草稿。 |

`agent-edit-completed` 事件包含持久化的 `agentTaskId`、`status`（`completed`、`partially_completed`、`failed` 或 `needs_clarification`）和 `result`；其中 `AgentEditResult` 包含同一 `agentTaskId`、固定诚实消息及可空的 `storyboard`、`timeline`、`preview` 与 `jianyingDraft`。`execute_agent_edit` 立即返回任务 ID：后端插入 `queued` 调用后在后台线程执行完整流水线。终态回复先以 `agent-task-result-{agentTaskId}` 作为消息 ID幂等写入原 conversation，并将仍无更新请求的会话恢复为 `ready`，随后才发事件。前端把事件作为低延迟通知，同时轮询 `list_agent_tasks`；事件丢失时从持久化消息和领域表恢复任务卡、回复及产物，不会重复插入 Agent 回复。未限定的中文“创建草稿”进入 Conversation Router，由模型结合状态选择工具；只有明确“创建剪映草稿”才直通 `create_jianying_draft`，明确“内部时间线”或“时间线”才直通 `create_timeline_draft`。显式“生成预览”或创建 Jianying draft 在请求未携带 storyboard 版本时，仍接受同一项目与剪辑任务内的指定时间线；这类上下文不完整的请求不直接执行，后端把已验证时间线及缺失上下文作为事实提供给模型，由模型在受控工具集中决定下一步；指定时间线不属于当前任务时会被拒绝。后端会归一化常见精确命令，并仅在动作成功后附加经验证的结果消息。`needs_clarification` 不创建产物，只返回一个可执行的中文澄清问题；`partially_completed` 保留并列出真实中间产物，但不声称最终目标完成。

Agent 工具失败后，循环可把不含路径和原始错误的结构化诊断临时回读模型，由模型生成自然失败说明；持久化步骤仍只保存安全码。即使模型给出说明，`status` 仍保持后端判定的 `failed` 或 `partially_completed`，消息不能替代真实产物。

Tauri 命令以适合展示的字符串错误返回，但不得在错误中暴露凭据或完整媒体路径。

“剪好了吗”“完成了吗”等精确状态问题走只读 `get_edit_status` 确定性路径，不调用模型。它只查询同一项目、剪辑任务和会话的上一条 Agent 任务及安全产物标识，区分处理中、待澄清、失败、部分完成、storyboard、内部时间线与 local preview；不会把后台视觉分析任务当作剪辑完成状态。

## 会话生命周期

运行时覆盖：未限定的中文“创建草稿”进入 Conversation Router，由模型结合状态选择工具；只有明确“创建剪映草稿”才直通 `create_jianying_draft`，明确“内部时间线”或“时间线”才直通 `create_timeline_draft`。本规则覆盖上文保留的历史表述。

schema v11 的 `task_state_snapshots` 是 Task Resolver 的受限输入，只保存目标、当前子目标、真实 storyboard/时间线/preview 阶段与标识、完成项和任务状态；每次任务收到已路由请求时更新当前子目标，事实字段在解析前从领域表重建。v11 会为缺少 `active_subgoal` 的早期任务快照表补齐该列。`conversations.summary` 仍只是侧栏预览，不能作为任务记忆。`pending_task_routes` 在项目级保存未归属请求、候选任务 ID、问题及 `pending/resolved/superseded` 生命周期；`task_route_receipts` 保存绑定项目、目标 task、目标 conversation、完整请求、唯一 user message 与可选 pending 记录的一次性授权。任何模型自动归属低于 0.85 都必须澄清，模型不能通过自报请求只读来降低门槛。任务内 `pending_clarifications` 继续按项目、剪辑任务和会话保存 `router`/`agent_run` 澄清。这些表都不保存模型响应原文、媒体证据、凭据或路径。

桌面应用直接进入 Agent 会话。自然语言消息先调用 `resolve_conversation_task`；新任务与 conversation 由路由事务原子创建，已有任务则由后端校验项目作用域。前端激活返回的目标、保存用户消息后，将同一请求和 `routeReceipt` 交给 `submit_conversation_turn`；保存 user message 会在同一事务中唯一占用凭证，后端只有消费成功才进入 Conversation Router。若多个凭证引用同一 pending，胜出者消费时会原子删除其余未消费 sibling，且消息占用也要求 pending 仍有效。若保存或提交前中断，项目级 pending 请求仍保持可恢复；若提交失败发生在消费之后，用户消息已经存在于目标 conversation。

`TaskRouteResult` 包含 `action`、可空 `taskId`/`conversationId`、`confidence`、可空 `question`/`suggestedTitle`、安全 `reasonCode`、可空 `deferredRequest` 与可空 `routeReceipt`。只有 `clarify` 不返回凭证；其余结果均返回已绑定确切 task 的一次性凭证。`deferredRequest` 只在用户解决项目级任务归属问题时返回，前端按固定格式把原始请求和归属补充组合后提交，该完整请求也受凭证约束。

## 模型 Provider 决策

模型请求统一经 `ModelAccess::resolve()` 选择活动 Provider：若已配置自定义 API 则优先生效，否则回退到实验性 OpenAI OAuth。自定义 API 走 OpenAI 兼容的 `{baseUrl}/chat/completions` 端点（凭 `Authorization: Bearer <apiKey>`），在 Rust 侧把 Responses 风格的 `input`/`text.format` 载荷转换为 chat/completions 的 `messages`/`response_format`，并替换为配置的 Model 名；OAuth 仍走实验性 Responses 端点。前端透过 `CustomApiStatus` 展示连接状态与配置的 Model，API Key 从不回传前端。`agent.rs`、`storyboard.rs`、`assets.rs` 的模型请求均改用该决策层。

模型传输复用一个进程级 `ureq::Agent`，同时保留每次请求自身的超时和凭据边界。交互 Agent 模型请求优先于尚未开始的粗视觉请求；粗视觉连续三次失败后熔断 60 秒，期间对应任务保持 `queued`，冷却后只允许一个半开探测。自定义 API 的批量视觉请求可使用可选 `coarseVisualModel`；storyboard 与 Agent 仍使用主 Model。OAuth 没有经验证的替代模型，继续使用既有请求模型。

生成 storyboard 前，brief 仅在本地与素材显示名、文件夹组织 hint 和 OCR 做词汇重合排序；只为 queued 视觉批次持久化纯数字 priority，相同分数按创建时间和任务 ID 稳定排序。最高相关的 queued 或 running 批次最多等待 65 秒后继续使用已落地视觉证据，不等待全部素材。文件名、文件夹和路径不进入 Provider；OCR 不进入粗视觉 Provider payload，但仍作为明确标注的本地提取文字证据提供给 storyboard，不能冒充画面语义。

## 实验性 OAuth

OAuth 命令兼容当前 OpenCode 实现，但不是官方 OpenAI 第三方集成。浏览器回调会在交换令牌前校验 PKCE state。访问和刷新凭据仅保存于 Windows Credential Manager，绝不返回前端、不写入 SQLite、项目文件、日志或工具结果。

实验性 Responses 请求设置 `store: false` 与 `stream: true`。后端累计 SSE 文本后再解析结构化工具或分析 JSON。回调成功、失败或超时后，后端发出 `experimental-openai-oauth-status` 事件；前端同时轮询状态作为恢复路径。

内部 `analyze_asset` 只执行本地技术分析，最多有两个 worker 并行运行：首次视频分析最多扫描前 30 秒、生成 4 张关键帧，并只对前两张关键帧进行 OCR；完成后素材成为技术 `ready`。FFprobe、缩略图、场景扫描、回退抽帧和 OCR 分别设有 20、30、45、20、20 秒硬超时；任一阶段超时都会将该素材标记失败而不阻塞队列，OCR 正常完成但无文字结果仍不失败。Windows 超时以无窗口 `taskkill /T /F` 请求终止子进程树，并在短时退出窗口内回收直接子进程；若终止请求或确认失败，调用会安全返回，不保证进程树已经退出。启动会将中断的本地 `running` 任务重新排队。后台 `analyze_asset_visual_batch` 以最多 6 条技术就绪素材为一批，发送每条素材一张低分辨率中间代表帧及素材 ID/源时间标签；模型响应只能回填同一批次内的 ID 和精确时间。该任务的持久化 payload 不含路径或媒体内容，结果只记录数量、安全错误码和从任务创建到终态的安全 `durationMs`。每批视觉分析请求带 30 秒超时；Provider 不可用、帧不可读或响应无效不影响技术 `ready`。连续 Provider 失败会熔断并令尚未开始的批次保持 `queued`。启动时有效的中断批次会恢复为 `queued`，无效 payload 则封闭为失败。storyboard 只使用 `ready` 且实际具有视觉证据的素材，并已按 brief 对 queued 视觉批次设置本地优先级。前端模型弹窗在已连接状态下提供退出登录按钮，调用 `clear_experimental_openai_oauth` 删除凭据并重置状态。

首次场景检测的滤镜顺序为 `fps=4 -> scale=320:-2:flags=fast_bilinear -> select(scene) -> showinfo`；它先降低比较成本，再以 `pts_time` 保存源时间。前 30 秒和最多 4 张关键帧仍是本地安全上限。

## 素材证据与 storyboard

Agent 的内部工具集中包含 `request_asset_analysis`：模型先通过 `list_assets` 观察项目素材，只能对该项目中已经导入且状态为 `queued` 或 `failed` 的素材请求本地分析。该工具不向模型暴露路径，也不授予它文件、SQLite、FFmpeg、FFprobe 或 Tesseract 的直接访问权。storyboard 响应还包含模型提出的 `targetDurationMs` 与 `scriptMode`（`full_script` 或 `key_message`）；30 个镜头/信息点和 120 秒是本地处理安全边界，不是成片创作规格。

`get_asset_evidence` 只返回派生证据：关键帧缓存路径、可选 `timeMs` 的 OCR 文本和视觉建议。它绝不返回 `source_reference` 或 `folder_reference`；UI 将派生图片路径转换为受限的 Tauri asset URL。

生成 storyboard 必须提交非空的用户 brief。模型输入仅有紧凑的持久化证据：素材 ID、媒体类型、已验证时长、场景片段、OCR 与视觉标签。生成镜头必须含素材 ID 和源范围；视频范围必须在已验证时长内，图片的源范围必须为零。校验失败不会保存版本。

## Agent 目标工具契约

```ts
type ModelProvider = 'openai-oauth' | 'custom-api' | 'local'
type ToolStatus = 'queued' | 'running' | 'completed' | 'partially_completed' | 'failed' | 'cancelled' | 'needs_clarification' | 'needs_review'

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

此 envelope 是通用 Agent 审计层的目标形态。通过作用域校验后，`execute_agent_edit` 会先创建持久化调用，再记录模型选择的允许工具、经脱敏的成功结果或安全失败结果；终态 `completed` 与 `failed` 可包含 `result`，`failed` 的结果仅含工具名、状态和失败代码，`error` 不得含凭据或不必要的本机路径。命令先同步插入 `queued` 调用并立即返回任务 ID，完整流水线在后台线程执行，终态经 `agent-edit-completed` 事件（携带 `AgentEditResult`）回传前端。工具或循环失败时不把技术校验错误直接交给 UI：技能循环内失败只回读安全失败代码供模型继续决策，不再请求独立后续回合，也不会自动重放失败工具；Provider 或模型不可用、或循环最终无法达成目标时，后端保存相同结构的安全失败结果并返回固定诚实降级回复。Agent 单步与 storyboard 模型请求保留 120 秒上限，Agent 循环还以 90 秒模型决策总预算收紧每步实际超时；不再存在独立意图分类请求。启动时仍为 `queued` 或 `running` 的通用调用会变为 `needs_review`，用户可重新发起请求，但系统不会自动重放未知副作用。

前端会缓存最多 20 个先于命令返回到达的 `agent-edit-completed` 事件，并在取得任务 ID 后立即对账；发送中或当前任务仍活跃时还会以 1.2 秒周期读取持久化任务终态。后端完成消息是权威来源，事件只是通知；任一通道先确认终态后，前端按项目、task、conversation 作用域重载消息和产物。仅当任务的 `projectId`/`editingTaskId` 仍等于当前活动作用域时才应用可见产物。模型超时、响应解析失败或循环耗尽且目标仍未满足时，无中间产物为 `failed`，已有真实中间产物为 `partially_completed`，两者结果代码均为 `agent_goal_not_reached`；中间版本保留并接受审计，但回复不得声称最终目标完成。

Agent 请求统一经 `agentloop.rs` 的封闭、有界目标驱动循环处理。显式单命令继续确定性直通；其余请求加载最近会话历史。`fast_goal` 锁定明确目标；模糊请求由首次主模型响应在同一个顶层 JSON 中声明 `goal`/`isQuestion` 并选择首个技能或直接回答，不再先执行一次独立模型分类。最近一次同作用域 `needs_clarification` 会作为结构化标记提供给首次决策，模型结合历史判断当前消息是否为补充 brief。确定性目标不能被模型覆盖；模糊目标一旦声明即锁定。模型最多运行 10 步，真实产物完成门、作用域校验、失败回读和诚实终端回复保持不变。交互模型决策总预算为 90 秒，每步按剩余预算收紧超时；这是协作式边界，不会强制中断已经开始的副作用。

每轮决策前，后端从当前作用域重建 `AgentStateSnapshot`，仅包含项目/剪辑任务/会话标识、素材可用与分析状态计数、当前真实产物状态、已执行步骤摘要、剩余步数、目标和未满足条件。完整 storyboard/时间线细节不再每轮直接注入；模型需要镜头细节时使用观察工具。确定性前置条件提示负责指出最短合法路径，但已有时间线时允许直接编辑、渲染 preview 或创建 Jianying draft，不强制重建 storyboard。每个循环技能和显式直通技能都写入 `agent_run_steps`：只保存工具名、步骤状态、安全产物类型/ID、安全错误码和时间戳。中断后运行仍进入 `needs_review`，未完成步骤封闭为 `failed/interrupted_requires_review`，绝不自动重放未知副作用。

| 工具 | 当前契约 | 实现状态 |
| --- | --- | --- |
| `request_asset_analysis` | `{ assetIds: string[] }` | 已实现：仅重新排队当前项目内已导入、源文件仍可用且尚未 ready/active 的素材分析。 |
| `get_asset_health_summary` | 无 | 已实现的只读 Agent 观察工具：返回当前项目持久化的健康计数、活动扫描状态、最近检查时间、脱敏原因码计数以及已解释/未解释失败数量；不访问源文件，不返回路径或原始系统错误。只有全部失败均有原因码时 `reasonEvidenceAvailable=true`。 |
| `search_assets` | `{ query?, kind?, minDurationMs?, maxDurationMs?, minRating?, favoriteOnly?, tag?, collectionId?, offset?, limit? }` | 已实现的只读 Agent 观察工具：按当前项目检索素材，单页最多 20 条并返回 `nextOffset`；自动排除禁止使用素材，只返回安全摘要和固定命中原因码，不返回路径、备注/OCR 正文、媒体内容或完整分析证据。 |
| `search_asset_segments` | `{ query, assetId?, offset?, limit? }` | 已实现的片段级只读观察工具：在当前项目已分析的视频/图片中返回明确 `sourceStartMs/sourceEndMs`、安全视觉标签、固定命中原因和游标；排除禁止使用及已知缺失、变化或不可读源，不返回路径或 OCR 正文。 |
| `get_text_capabilities` | 无 | 已实现：返回可用于 local preview 的字体/动态，以及已验证可交付 Jianying 的最小文本矩阵和文本预设。每个预设包含机器可读的 `selectionHint`，使模型按字幕、递进/揭示、反差/结果、结论/警示或 CTA 的语义选择配方。 |
| `search_media_segments` | 名称保留，输入/结果未定义 | TODO |
| `create_timeline_draft` | `{ projectId, storyboardVersionId }` | 已实现，支持经验证的图片/视频 storyboard 镜头。 |
| `render_preview` | `renderPreview(timelineVersionId)` | 已实现，本地 540 x 960 H.264 preview。 |
| `create_jianying_draft` | `{ timelineVersionId }` | 已实现，创建并注册唯一的 Jianying Pro 8.0 仅视频草稿。 |
| `replace_clips` | `{ timelineVersionId?, replacements: [{ shotIndex, assetId, sourceStartMs, sourceEndMs }] }` | 已实现，批量替换既有镜头并保持对应时间线时长。 |
| `change_clip_duration` | `{ timelineVersionId?, adjustments: [{ shotIndex, newDurationMs?, newSourceStartMs? }] }` | 已实现，在已验证源范围内重定时长与起止点。 |
| `reorder_clips` | `{ timelineVersionId?, order: number[] }` | 已实现，要求 `order` 为全部既有 `shotIndex` 的完整排列。 |
| `replace_text_tracks` | `{ timelineVersionId?, textTracks: TextTrack[] }` | 已实现：Agent 可替换当前作用域时间线的完整文本轨；cue 只需提供 ID、时间和文案，省略的样式/布局使用安全默认值。成功结果包含非阻断 `qualityWarnings`（阅读密度、超过两行、动画占比和相邻重复文案）。cue 可带可选 `templateId`，后端将其解析成完整且可审计的样式/布局/动态配方，并覆盖冲突字段。交付级 `subtitle_safe`、`headline_rise`、`headline_pop` 与 `headline_drop` 都包含已验证的淡出；后者使用向下滑入。后端校验 cue 时间、颜色、样式/布局、受限动画及唯一 ID，并拒绝跨文本轨的 headline 重叠，且不会接受模型自证 Jianying 兼容性。 |
| `request_clarification` | `{ question }` | 已实现，不产生任何产物，仅返回澄清问题。 |

“分析素材”“重新分析视频/图片/媒体文件”等请求由首次主模型决策声明为无产物门的观察目标，避免被误设为 storyboard；模型在同一响应中自主选择 `list_assets`、`request_asset_analysis`、澄清或其他合法工具，不存在独立分类器替代工具决策。

### `replace_music_tracks`

`{ timelineVersionId?, musicTracks: MusicTrack[] }`：Agent 只可使用当前 local project 内分析完成的音频素材；cue 带源/时间线范围、可选循环、0–2 音量和淡入淡出。每次替换创建内部时间线新版本及审计；FFmpeg preview 在本地混入音乐且不改写源媒体。`create_jianying_draft` 会为音乐轨创建新的实验性 Jianying draft：仅使用当前项目 ready audio asset，并映射裁剪、循环、音量与淡入淡出；不得覆盖既有 draft，且生成后必须在 Jianying 中复核播放效果。

### `search_music` / `download_music` / `use_online_music`

`search_music({ query })` 只检索已配置的 Jamendo Provider，返回 API 明示 `audiodownload_allowed` 且为 CC0 或 CC-BY 的曲目；CC-BY 曲目的归属信息会随 music cue 持久化。`download_music({ trackId })` 仅下载该单曲到当前 local project 的受控目录，再进入现有媒体分析队列；下载完成不等于可编辑，音乐轨仍只接受分析为 `ready` 的音频 asset。`use_online_music({ trackId, timelineVersionId? })` 则在同一受限工具调用内下载单曲、等待本地分析完成并创建新的音乐时间线版本，默认按整条 timeline 循环和安全背景音量写入；它不会最终导出或覆盖既有 Jianying draft。Provider 凭据不进入工具结果、SQLite 或日志。

## 当前 Agent runtime 覆盖说明

本节历史表述中任何“未限定的创建草稿”归为 Jianying 的规则已废止：只有明确包含“剪映”或 `Jianying draft` 的精确单命令才允许直通。未限定草稿、preview/Jianying draft 缺少时间线、以及其他普通自然语言请求均进入模型工具循环；模型必须显式选择 `create_timeline_draft`，交付工具不会隐式创建时间线。

以下规则覆盖本文中保留的历史“6 步”表述：当前循环最多 10 步，模型在最后一步可对真实产物或部分完成项作总结；成功产物仍由后端验证，`AgentEditResult.message` 中的完成事实始终使用工具返回的权威摘要，不能被模型文本覆盖。可用技能还包括 `request_asset_analysis`，用于对当前项目内已导入、`queued` 或 `failed` 的素材排队本地分析。只有声明 `informationScope=general` 的问答与澄清可直接展示模型自然语言回复；项目事实问答必须进入只读工具循环。已有观察结果包含所问事实时，模型应直接 `finish` 回答，不使用语义重叠的观察工具重复确认。

开发诊断阶段可通过 `list_agent_diagnostics({ projectId, editingTaskId, agentTaskId })` 读取本地诊断记录。它只包含同一作用域内的受控阶段标记、响应长度和安全错误码，用于定位模型请求、响应解析、工具或管线在哪一步失败；绝不保存模型原文、会话内容、媒体证据、凭据或本机路径。

## 导入、时间线与 preview 规则

文件夹导入会递归记录支持的视频、图片和音频引用，不复制或修改源文件。`StoredAsset` 仅暴露 `folderName` 与 `relativePath`，不暴露绝对源路径；同时返回 `analysisStatus` 与 `visualAnalysisStatus`，使 UI 能按真实技术/视觉状态筛选。分析会写入时长、尺寸、帧率、音频、缩略图、关键帧、场景、OCR 和视觉标签计数。资产响应将活动分析任务映射为 `queued` 或 `analyzing`；初始化会恢复未完成任务、取消同一素材的重复任务，并为状态为 `queued`/`analyzing` 但没有任何对应分析任务的孤立素材补建并排队分析。桌面 UI 轮询 `list_assets`，在右下角展示活动分析数量和最多三个显示名，任务完成后自动移除；该提示不增加新的 Tauri 命令。

`change_clip_duration` 对视频保存实际使用的源窗口：`sourceEndMs = sourceStartMs + timelineDurationMs`；图片仍使用零源范围。新起点不得早于变更前已验证窗口的 `sourceStartMs`，新结束点不得晚于其 `sourceEndMs` 或素材技术时长，因此缩短或移动镜头不会越出已验证范围。

`create_timeline_draft` 的成功结果是按 storyboard 镜头顺序映射的内部时间线版本。版本化 `TimelineContent` 已预留 `textTracks`，旧版本读取为 `[]`；模型可经 `replace_text_tracks` 提交完整文本轨，后端校验 cue 时间、颜色、布局/样式范围、受限动画及唯一 ID，并按后端的已验证矩阵写入兼容性，绝不接受模型自证兼容。多轨音频、字幕、变换和自动化仍为 `TODO`。时间线变更目前只可经 `execute_agent_edit` 调用，决策严格限制在关闭工具集内：`replace_clips` 可一次替换多个既有 `shot_index`（每个保持对应时间线时长，视频源范围须已验证且严格等于该时长，图片源范围为零）；`change_clip_duration` 在不超出已验证源范围的前提下重定时长与起止点；`reorder_clips` 的 `order` 必须是全部既有 `shot_index` 的完整排列。每次变更都会创建新 `TimelineVersion` 并记录前后变化；`request_clarification` 仅返回澄清问题，不创建任何产物。

`render_preview` 会把已启用的 `textTracks` 编译为 ASS，再通过 FFmpeg/libass 叠加；已验证的最小 Jianying 文本矩阵包含 Unicode 文案。适配器对每条文本素材的嵌套 `content` JSON 使用 Unicode 转义，已在当前剪映 11.2 实机验收中文正确显示。适配器也可写入描边、背景、阴影及五个剪映内置字体资源，但这些字段在实机视觉验收前仍不是可交付能力。

前端 `TimelineVersion` 投影包含完整 `textTracks`。它仅用于显示当前本地时间线的文本 cue 与兼容性状态，不暴露或读取其他 Jianying 草稿。

`StoryboardVersion` 的 `shots` 响应新增 `beatId` 与 `matchLevel`（`direct` 或 `contextual`）；同时新增 `beats`（`id`、`purpose`、`requiredVisual`）和 `uncoveredBeatIds`。这是加性契约：旧 storyboard 读取时返回空的 `beats`/`uncoveredBeatIds`，旧镜头的 `matchLevel` 回退为 `contextual`。`uncoveredBeatIds` 不会映射成时间线 clip；调用方不得将其表述为已经被素材画面覆盖。

preview 渲染使用归一化图片/视频片段和内部 concat 序列，生成本地 540 x 960 H.264 MP4。存在已启用 `textTracks` 时，后端会生成 ASS 并以 FFmpeg/libass 叠加文本；当前允许 `sans_bold`、`sans_clean`、`serif_editorial`、`mono_tech` 字体 key 及 `fade`、`slide_up`、`slide_down`、`pop`、`wipe` 基础动态。结果包含黑帧扫描、精确重复源范围、低分辨率视觉相似候选、节奏异常与文本安全区/可读时长检查。当前不混音、不做多帧语义重复检测，也不提供取消语义。

## Jianying draft 创建规则

`JianyingDraftResult` 会返回草稿目录和内容文件路径，仅供本地桌面流程使用，调用方不得将其记录到日志、浏览器存储或文档。

- 仅创建新目标目录，绝不覆盖已有 Jianying 项目。
- 成功前验证所有媒体引用。
- 更新首页注册表期间 Jianying Pro 必须关闭。
- Assembly Video Agent 跨进程串行化注册表写入；替换前若注册表变化则中止。
- 源文件或配置的草稿库不可用时返回结构化失败。
- 不自动执行最终视频导出。
- 时间线含有 `textTracks` 时，当前版本会拒绝创建 Jianying draft，避免将尚未经 Jianying Pro 8.0 视觉验证的文本静默丢失；用户仍可创建含文本的 local preview。
- 运行时覆盖说明：已在 Jianying Pro 8.0 中验收的最小文本矩阵为 `jianying_default` 字体下的静态文本、`fade` 入场或出场、`slide_up` 入场、`slide_down` 入场和 `pop` 入场。后端仅当 cue 不含描边、阴影、背景或循环动画，且仅使用上述出入场组合时，将它标记为 `verified` 并随新 Jianying draft 写入；其余 cue 保持 `local_preview_only`，创建草稿会明确拒绝。

## 外部契约待定

已提供非空文案并要求剪辑的调用，如果 storyboard 生成因非前置条件校验失败，模型会收到该事实并继续决定重试或自然语言解释，而不会退化成“请描述成片目标”。只有缺少已分析素材等真实前置条件时才能返回 `needs_clarification`。

- 官方 OpenAI OAuth 的授权 URL、scope、令牌刷新和支持的模型能力。
- 除 OpenAI 兼容 chat/completions 外，其他模型 Provider 适配器 schema。
- 用户提供的 voice API 鉴权、请求体、音色选择、响应和异步任务处理。

## 开发期文档同步 Harness

`npm run harness:check` 不属于桌面应用 API；它是仓库开发期的 Git 变更集检查。规则定义在 `.harness/doc-sync-policy.json`，检查高影响 Tauri 命令、持久化、OAuth/安全和运行时配置变动是否同步更新本文档及其他要求的 Markdown。触发规则时，变更集还必须包含一份 `docs/changes/` 记录。详细的执行与 Agent 审查 loop 见 `docs/harness.md`。
