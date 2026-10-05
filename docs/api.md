# API 与工具契约

本文只描述当前 checkout 的注册契约。历史见 [changes/](changes/README.md)，产品链路见 [architecture.md](architecture.md)。本次以源码静态核实；真实媒体、网关、编辑器效果均**未核实**。

选镜 / 策划评测运行器是 `footage-eval` feature 下的独立 CLI 二进制，不新增也不修改任何 Tauri IPC 命令或 Agent 工具；用法见 [docs/evaluation/README.md](evaluation/README.md)。

### 预留素材策划内部接口（任务 4，尚未接入生成）

`storyboard/inventory.rs::build_inventory(access, request, &[SegmentEvidence])` 分批覆盖完整分析集合，返回 `Inventory`：逐片 `InventoryItem.reference`（assetId / segmentId / evidenceId / range / supports）及 `analysisSnapshotId`、自由表达 `statements`（expression / evidenceQuote / direct）、可讲内容、缺口与带原文锚点的因果关系。模型只回传理解、批内片段序号与原文锚点序号，代码绑定完整源窗、身份和原分析逐字引文；漏报 direct 不当直证。缺片、重复引用或不存在的锚点均失败，不建立素材场景/功能类目。全库汇总与独立需求覆盖审核并发，代码用后者的必需画面缺项控制 `requestFulfillable`，避免汇总把剪辑参数当缺素材；原始两份模型判断都保留轨迹，因果链仍单独核对。

`storyboard/genre.rs::decide_genre(access, selection, request, inventory, previous)` 使用 `GenreSelection=auto/narrative/promotion/bts`；手选固定，自动只发一次判断，信息不足默认宣传；自动叙事缺因果改花絮，手选叙事保留体裁且受限。`GenreDecision.snapshotId` 绑定需求、手选和完整盘点；同输入复用 `previous` 不请求模型，输入改变拒绝旧快照。

`build_recipe(decision, durationMs?, eligibleEvidence, inventory)` 返回版本 `genre-recipe-2026-10-05-v1` 的 `GenreRecipe`。默认叙事/宣传 30 秒、花絮 20 秒；用户毫秒优先。正常结构叙事 4 段 20/35/25/20%，宣传 4–6 段 15/70/15%（卖点按直证数量分预算），花絮 3–5 段 10/80/10%。每段 `budgetMs/minReadableMs/maxShots` 由代码算；过短宣传/花絮合并可选过渡，叙事短于四段可读时间失败。合格窗不足收缩预算，不回放淘汰画面。

`storyboard/planning.rs::plan_with_evidence(access, request, inventory, decision, recipe, eligibleEvidence, userFacts)` **只消费调用方已过底线的证据列表**，不依赖 eligibility 模块，也不自己判断风险合格。返回 `PlanningResult.status=accepted/limited/gap_only/rejected`、有引用的段/具体表达/备选、真实代码预算、缺口和拒绝原因。`rejectedAttempts` 保留每次被拒提案的原因；最多基于失败原因提出一次新策划，新提案重新经过全部校验，最终仍不合格即返回 rejected。Provider 失败不走这条内容校正。模型选择的原文锚点由代码回填为 `evidenceQuote`；引用身份、允许窗、原分析引文、配方顺序、可读镜数、重复源窗和数字来源由代码检查。宣传卖点禁用气氛支持，模型另并发逐段审核主选及备选直证，再审核标题/必需画面/因果链。审核失败不提供可落地策划；模型审核是可追溯的判断，不是人工金标或视觉真值。

`UserFact{id,text,source="user_request"}` 必须逐字出自本次请求；数字表达必须逐字等于对应事实且携带 `userFactId`，不能从机器画面推产能。缺必需画面/手选叙事缺因果返回 `gap_only`，零合格片段明确错误。备选不足如实记缺口。`PlanningResult.metadata` 使用任务 2 的 `StoryboardEvidenceMetadata`，提供 pipelineVersion / genre / recipeVersion / 完整合格证据快照 / 主选与备选引用，任务 6 在新版本事务中持久化；本接口不写历史产物。

合格集合仍须提供有效、已封印的 `SegmentEvidence`；补核验或收窄合格窗改变内容时由上游重新封印，不沿用旧 evidenceId。盘点按 assetId / segmentId / analysisSnapshotId 和源窗包含关系绑定同一分析，策划输入的引用重新绑定为合格集合当前 ID/窗；旧盘点 ID 不作为放行 ID。当前接口每个基础片段最多一个合格窗，不表示支持多个离散窗。

模型策划仅需回传 evidenceId 和原文锚点序号，代码回填身份、完整合格窗及 supports=expression；若模型仍主动提供身份/源窗，照常验证，伪造或越窗仍拒绝。最佳源窗由任务 5 精修。段审核与全局审核互不依赖并发执行，Schema 限定审核数组长度和索引，代码仍检查缺项/重复；短于最短可读时长的源窗不进入提案候选，但仍保留在事实快照。2D/3D 技术名称须有直证但不当作产能数字，性能/产能/百分比等数量仍须逐字用户事实。盘点遗漏或锚点无效时只补读缺项一轮，仍不完整失败；媒体开关、时钟与风险放行不作为本阶段的缺画面理由。

本阶段请求携带严格 JSON Schema（必需字段、数组元素、批次数量、片段序号和允许证据 ID）；使用统一 Provider，Custom/Gateway 走 Chat 嵌套 json_schema，OAuth 走 Responses 格式。不支持该格式的 Provider 返回真实错误，不自动改走宽松 JSON 或另一模型。

## 桌面命令边界

命令清单以 `src-tauri/src/lib.rs::generate_handler!` 为准，共 **101** 个；包括仍注册的兼容命令。前端 invoke 只在 `src/lib/local-store.ts`，并非每个注册命令都有 wrapper。参数用 camelCase；表内类型为 Rust 声明，`AppHandle` 由 Tauri 注入、不属于输入，`Option<T>` 为可省略/空值，`Result<T, String>` 成功返回 T、失败拒绝 Promise。DTO 的序列化字段以 `models.rs` 及各命令模块的 serde 声明、bridge 类型为准。

### 账号、模型与外部服务

| 命令 | 输入（类型） | 返回（Rust） | 行为与边界 |
|---|---|---|---|
| `get_experimental_openai_oauth_status` | `无` | `OAuthStatus` | 仅从 Windows Credential Manager 读取连接状态。 |
| `start_experimental_openai_oauth` | `无` | `Result<OAuthStart, String>` | 启动五分钟 loopback PKCE 回调并返回浏览器授权 URL；仅个人测试。 |
| `clear_experimental_openai_oauth` | `无` | `OAuthStatus` | 删除 Windows Credential Manager 中的实验性凭据并重置连接状态。 |
| `get_custom_api_status` | `无` | `CustomApiStatus` | 仅返回自定义 API 的 Base URL、主 Model、可选粗视觉 Model；不返回 API Key。 |
| `save_custom_api` | `baseUrl: String, model: String, coarseVisualModel: Option<String>, apiKey: String` | `CustomApiStatus` | 保存于 Windows Credential Manager；粗视觉 Model 为空时沿用主 Model。 |
| `clear_custom_api` | `无` | `CustomApiStatus` | 删除 Windows Credential Manager 中的自定义 API 凭据并重置状态。 |
| `sign_in_fellowcut` | `email: String, password: String` | `Result<FellowCutAccountStatus, String>` | 邮箱/密码换取 Firebase 会话，仅系统凭据库保存登录；返回账号与只读资格，不返回 token。 |
| `get_fellowcut_account_status` | `无` | `Result<FellowCutAccountStatus, String>` | 读取/刷新本机登录与网关资格投影；不进行模型调用，不返回 token。 |
| `sign_out_fellowcut` | `无` | `Result<FellowCutAccountStatus, String>` | 清除本机登录凭据，不删除云端账号或项目。 |
| `get_voice_availability` | `无` | `VoiceAvailability` | 配音开关是否可用：内置网关时探测网关配音，只有网关明确没有配音能力时 `available=false`；不返回任何密钥。 |
| `get_jamendo_status` | `无` | `JamendoStatus` | 只检查 Windows Credential Manager 中是否存在可读取的 Jamendo client ID，返回 `connected` 或 `disconnected`。 |
| `save_jamendo_client_id` | `clientId: String` | `JamendoStatus` | 将非空 Jamendo client ID 写入 Windows Credential Manager；失败时只返回 `failed`，不回传凭据。 |
| `get_elevenlabs_status` | `无` | `ElevenLabsStatus` | 返回密钥是否已存、音色列表是否可读、可空的 TTS 授权探测和安全错误码；不返回 API Key。 |
| `save_elevenlabs_api_key` | `apiKey: String` | `ElevenLabsStatus` | 将非空 ElevenLabs API Key 写入 Windows Credential Manager，并只 `GET /v1/voices` 探活。 |
| `clear_elevenlabs_api_key` | `无` | `ElevenLabsStatus` | 删除 Windows Credential Manager 中的 ElevenLabs 密钥。 |
| `import_elevenlabs_api_key_from_environment` | `无` | `ElevenLabsStatus` | 当凭据库未配置时，从本机 `ELEVENLABS_API_KEY` 导入一次；不在每次 HTTP 时偷读环境变量。 |
| `get_fish_audio_status` | `无` | `FishAudioStatus` | 返回 Fish Audio 密钥是否已存及音色列表是否可读；不返回 API Key。 |
| `save_fish_audio_api_key` | `apiKey: String` | `FishAudioStatus` | 将 Fish Audio API Key 写入 Windows Credential Manager，并用音色列表接口探活。配置后配音优先使用 Fish Audio；传输/超时/5xx/429 且 ElevenLabs 已配置时可回退（401/密钥错误不回退）。 |
| `clear_fish_audio_api_key` | `无` | `FishAudioStatus` | 删除 Windows Credential Manager 中的 Fish Audio 密钥。 |
| `import_fish_audio_api_key_from_environment` | `无` | `FishAudioStatus` | 从本机 `FISH_API_KEY` 导入一次。 |

### 项目、共享库与会话

| 命令 | 输入（类型） | 返回（Rust） | 行为与边界 |
|---|---|---|---|
| `initialize_local_store` | `无` | `Result<StoreStatus, String>` | 创建数据目录、SQLite WAL 与只追加迁移；恢复中断分析与会话，不自动重放未知 Agent 副作用；缺权重时启动后台下载。 |
| `create_project` | `name: String, libraryIds: Option<Vec<String>>` | `Result<Project, String>` | 省略库列表默认全选，空数组不选库。 |
| `rename_project` | `projectId: String, name: String` | `Result<Project, String>` | 修改项目名称并更新时间，拒绝空名称。 |
| `delete_project` | `projectId: String, confirmed: bool` | `Result<(), String>` | 删除项目及其素材索引、分析派生文件、会话和本地预览；保留原始媒体与外部剪映草稿。必须 `confirmed=true`。 |
| `list_shared_libraries` | `无` | `Result<Vec<SharedLibrary>, String>` | 全局子素材库名称、素材数量与默认子库标记 `unfiled`。 |
| `list_projects` | `无` | `Result<Vec<Project>, String>` | 按最后更新时间倒序。 |
| `get_candidate_score_first_slots` | `projectId: String` | `Result<usize, String>` | 读取项目每拍 9 条候选中按综合分优先选入的名额，默认 5。 |
| `set_candidate_score_first_slots` | `projectId: String, scoreFirstSlots: usize` | `Result<usize, String>` | 保存项目候选名额；设置界面提供 3～9 条。其余名额轮流从 CLIP 画面、语义、关键词分项高分候选中选入，仍遵守同素材和相似画面限制；下次生成生效。 |
| `create_editing_session` | `projectId: String, title: String` | `Result<EditingSession, String>` | 兼容入口；在同一事务内创建 editing task 与首个 conversation，拒绝空标题。 |
| `list_editing_sessions` | `projectId: String` | `Result<Vec<EditingSession>, String>` | 返回项目内 task 与最近 conversation 的兼容聚合投影。 |
| `rename_editing_session` | `projectId: String, editingTaskId: String, title: String` | `Result<(), String>` | 同步修改 editing task 及其 conversation 标题，拒绝空名称。 |
| `delete_editing_session` | `projectId: String, editingTaskId: String, confirmed: bool` | `Result<(), String>` | 删除剪辑会话（editing task）及其对话消息、Agent 记录、storyboard/timeline 与本地 preview 目录；项目级素材保留。必须 `confirmed=true`；进行中的 Agent 任务先标为 `cancelled`。不删除用户 Jianying 草稿目录中的外部草稿。 |
| `create_editing_task` | `projectId: String, title: String` | `Result<EditingTask, String>` | 在既有项目内创建作用域化创作目标。 |
| `list_editing_tasks` | `projectId: String` | `Result<Vec<EditingTask>, String>` | 按最后更新时间倒序。 |
| `update_editing_task_brief` | `editingTaskId: String, brief: String` | `Result<(), String>` | 保存非空 brief；首次请求会为未命名任务定名。 |
| `create_conversation` | `projectId: String, editingTaskId: String, title: String` | `Result<Conversation, String>` | 任务必须属于指定项目，拒绝空标题。 |
| `list_conversations` | `projectId: String, editingTaskId: Option<String>` | `Result<Vec<Conversation>, String>` | 按最后更新时间倒序；可按任务过滤。 |
| `create_message` | `conversationId: String, role: String, content: String, routeReceipt: Option<String>` | `Result<Message, String>` | 保存消息并更新时间；`role` 可为 `user`、`assistant`、`agent`、`tool` 或 `system`。`role=user` 必须提供与目标 conversation 和完整 content 匹配、仍未消费的 route receipt，其他角色不需要。`role=agent` 且会话最后一条正是内容相同的 agent 消息时不再插入，直接返回那条消息（如重复导入提示）。 |
| `set_conversation_status` | `conversationId: String, status: String` | `Result<(), String>` | 状态为 `ready`、`working` 或 `review`。 |
| `list_messages` | `conversationId: String` | `Result<Vec<Message>, String>` | 按时间正序。 |
| `resolve_conversation_task` | `projectId: String, activeEditingTaskId: Option<String>, request: String` | `Result<TaskRouteResult, String>` | 在消息持久化前解析当前激活任务的归属；候选仅为仍属于该项目的显式活动任务，不把兄弟任务的 title/brief/`active_subgoal` 交给路由模型。返回继续当前任务、原子创建新任务或澄清（继续或新建，不列举其他任务）。没有激活任务时直接创建新任务。确定目标时签发一次性 route receipt；只选择任务，不选择 Agent 工具。 |

### 导入、分析与素材库

| 命令 | 输入（类型） | 返回（Rust） | 行为与边界 |
|---|---|---|---|
| `import_assets` | `projectId: String, sourceReferences: Vec<String>` | `Result<Vec<Asset>, String>` | 校验本地文件、保存引用并排队分析。 |
| `import_asset_folder` | `projectId: String, sourceDirectory: String` | `Result<Vec<Asset>, String>` | 递归登记支持的媒体，并记录文件夹层级根。 |
| `preview_asset_relink` | `projectId: String, sourceDirectory: String` | `Result<AssetRelinkPreview, String>` | 扫描用户选定的新根目录，仅按唯一的原相对路径与媒体类型给出可确认匹配；不修改项目。 |
| `confirm_asset_relink` | `projectId: String, sourceDirectory: String, assetIds: Vec<String>, preserveAnalysis: bool` | `Result<AssetRelinkResult, String>` | 重新计算已预览的唯一匹配后，才更新所选素材源引用。`preserveAnalysis=true` 仅更新路径并保留已有分析证据；`false` 时清除旧分析证据、取消旧 active 分析任务并按有界批次重排分析。 |
| `preview_collect_project_media` | `projectId: String` | `Result<CollectProjectMediaPreview, String>` | 用户发起收集前逐项复核源文件并估算复制量；不写文件。 |
| `collect_project_media` | `projectId: String, destinationDirectory: String` | `Result<CollectProjectMediaResult, String>` | 在用户选择目录下创建 UUID 命名的新包，复制当前可读源文件并写无原路径 manifest；不覆盖已有文件、不改写项目引用，操作日志只记录计数。 |
| `list_assets` | `projectId: String` | `Result<Vec<Asset>, String>` | 安全素材投影，含技术/视觉状态与证据计数；源引用不进入列表。遗留入口可顺带推进排队分析，Agent 同名观察工具只读。 |
| `list_asset_page` | `projectId: String, search: Option<String>, kind: Option<String>, analysisStatus: Option<String>, analysisState: Option<String>, visualStatus: Option<String>, directoryKey: Option<String>, userFilter: Option<String>, collectionId: Option<String>, offset: usize, limit: usize` | `Result<AssetPage, String>` | SQLite 有界分页（limit 1–200）；支持 search/kind/analysisStatus/visualStatus/analysisState/directoryKey；返回 items、counts、完整目录树、unfiledCount、全库 progress。 |
| `get_asset_task_center` | `projectId: String` | `Result<AssetTaskCenter, String>` | 返回项目级技术/视觉任务的排队、运行、失败、跳过计数，以及最多 50 条只含安全原因码的最近失败；不返回后台错误原文、路径或媒体证据。 |
| `get_asset_analysis_progress` | `projectId: String, assetIds: Option<Vec<String>>` | `Result<AssetAnalysisProgress, String>` | AssetAnalysisProgress |
| `cancel_asset_analysis` | `projectId: String, assetIds: Option<Vec<String>>` | `Result<usize, String>` | 取消数量 |
| `resume_asset_analysis` | `projectId: String, assetIds: Option<Vec<String>>` | `Result<usize, String>` | 继续数量 |
| `rename_library_asset` | `projectId: String, assetId: String, name: String` | `Result<(), String>` | void |
| `remove_library_assets` | `projectId: String, assetIds: Vec<String>` | `Result<BatchAssetActionResult, String>` | BatchAssetActionResult |
| `start_asset_health_scan` | `projectId: String` | `Result<AssetHealthScanStart, String>` | 显式启动可取消的后台源文件元数据检查；已有活动扫描时返回同一任务。 |
| `cancel_asset_health_scan` | `projectId: String, taskId: String` | `Result<(), String>` | 取消当前项目仍在排队或运行的健康扫描。 |
| `get_asset_health_scan_summary` | `projectId: String` | `Result<AssetHealthScanSummary, String>` | 读取持久化健康计数与活动任务进度，不访问源文件。 |
| `retry_asset_analysis_batch` | `projectId: String, assetIds: Vec<String>` | `Result<BatchAssetActionResult, String>` | 用户批量重试技术分析，每次最多 200 条；只处理当前项目、源文件仍可用且未 ready/active 的素材，活动任务不重复创建，并写入用户操作审计。 |
| `skip_asset_visual_analysis_batch` | `projectId: String, assetIds: Vec<String>` | `Result<BatchAssetActionResult, String>` | 用户明确确认后批量跳过视觉分析，每次最多 200 条；仅修改当前项目技术 `ready` 的图片/视频，保留技术证据、清除视觉标签并写入用户操作审计。在途视觉批次不得覆盖显式用户跳过。 |
| `update_asset_user_metadata_batch` | `projectId: String, assetIds: Vec<String>, favorite: Option<bool>, rating: Option<i64>, note: Option<String>, excluded: Option<bool>` | `Result<BatchAssetActionResult, String>` | 批量设置收藏、0–5 评分、最多 2000 字符备注和禁止使用；用户字段与分析证据分表保存，审计不保存正文。 |
| `add_asset_tag_batch` | `projectId: String, assetIds: Vec<String>, tag: String` | `Result<BatchAssetActionResult, String>` | 增删项目内不区分大小写的 1–64 字符用户标签。 |
| `remove_asset_tag_batch` | `projectId: String, assetIds: Vec<String>, tag: String` | `Result<BatchAssetActionResult, String>` | 增删项目内不区分大小写的 1–64 字符用户标签。 |
| `create_asset_collection` | `projectId: String, name: String` | `Result<AssetCollection, String>` | 创建并查询项目内集合、将最多 200 条当前项目素材加入集合；集合不移动源媒体。 |
| `list_asset_collections` | `projectId: String` | `Result<Vec<AssetCollection>, String>` | 创建并查询项目内集合、将最多 200 条当前项目素材加入集合；集合不移动源媒体。 |
| `add_assets_to_collection` | `projectId: String, collectionId: String, assetIds: Vec<String>` | `Result<BatchAssetActionResult, String>` | 创建并查询项目内集合、将最多 200 条当前项目素材加入集合；集合不移动源媒体。 |
| `get_asset_evidence` | `assetId: String` | `Result<AssetEvidence, String>` | 返回派生关键帧、OCR、视觉证据、`durationMs`、`analysisVersion`、独立 `visualAnalysisStatus`，以及 `segments[]`（真实场景片段的帧、可选视觉标签，以及可选 `usableStartMs`/`usableEndMs`/`motionTailSettled`/`motionUncertain`/`motionEnergy[]`）；视觉分析失败或跳过时返回 `visualAnalysisNote` 说明原因。另追加 `segmentEvidence[]` 结构化证据（schema v1，见下文），不移除原字段。 |

### 故事版、时间线与镜头编辑

| 命令 | 输入（类型） | 返回（Rust） | 行为与边界 |
|---|---|---|---|
| `generate_storyboard` | `projectId: String, editingTaskId: String, brief: String, voiceId: Option<String>` | `Result<StoryboardVersion, String>` | 公开命令以 brief 创建经过 P1–P5 校验的任务内故事版；配音/媒体快照与自动 preview/交付编排属于 Agent 同名工具，见下表。 |
| `get_latest_storyboard` | `projectId: String, editingTaskId: String` | `Result<Option<StoryboardVersion>, String>` | 加载所选任务的最新 storyboard。 |
| `list_storyboard_versions` | `projectId: String, editingTaskId: String` | `Result<Vec<StoryboardVersion>, String>` | 返回该剪辑任务内全部故事版，按创建先后倒序。`versionNumber` 是会话内版本号，每个剪辑任务从 1 开始，局部编辑派生版本同样在任务内累加；schema v20 之前的旧版本保留原项目内编号。 |
| `get_storyboard_version` | `projectId: String, editingTaskId: String, storyboardVersionId: String` | `Result<StoryboardVersion, String>` | 读取指定故事版；必须属于当前项目和剪辑任务。 |
| `create_timeline_draft` | `projectId: String, storyboardVersionId: String` | `Result<TimelineVersion, String>` | 从经验证的 storyboard 创建源时间绑定内部时间线。 |
| `get_latest_timeline` | `projectId: String, storyboardVersionId: String` | `Result<Option<LatestTimeline>, String>` | 仅加载该 storyboard 的最新时间线及其 preview。 |
| `list_timeline_versions` | `projectId: String, editingTaskId: String, storyboardVersionId: String` | `Result<Vec<TimelineVersion>, String>` | 返回同一项目、剪辑任务与 storyboard 内的时间线版本，按创建先后倒序。`versionNumber` 按 storyboard 所属剪辑任务编号，每个任务从 1 开始，规则同故事版。 |
| `commit_studio_edits` | `payload: StudioCommitPayload` | `Result<StudioCommitResult, String>` | 校验 StudioCommitPayload 后以 user 身份原子创建时间线新版本与审计；支持源片替换/构图、主轨与叠加轨编辑、文本/音乐/旁白轨。preview 另行渲染。 |
| `list_shot_recommendations` | `projectId: String, editingTaskId: String, timelineVersionId: String, shotIndex: i64` | `Result<ShotRecommendations, String>` | 只读已保存候选，返回 `saved, beatPurpose, candidates`。候选含唯一 `candidateId`（素材＋片段）、素材 ID、源起止、名称、片段缩略图、时长、当前/已使用标记及不可用原因；最多 12 个，不补假数据。 |
| `generate_shot_recommendations` | `projectId: String, editingTaskId: String, timelineVersionId: String, shotIndex: i64` | `Result<ShotRecommendations, String>` | 老版本用户主动生成；复用已分析证据的本地排序并保存候选，不改粗剪、不重新识别素材。 |
| `prepare_shot_replacement` | `projectId: String, editingTaskId: String, timelineVersionId: String, shotIndex: i64, candidateId: String` | `Result<PreparedShotReplacement, String>` | 为一个候选复用 Phase 4 精修并渲染静音画面，返回原版本 ID、镜头索引、素材 ID、源起止、`cropFocus`、`previewPath`；不写时间线。依赖已配置模型，失败直接返回。 |

### 预览、品牌与运行时

| 命令 | 输入（类型） | 返回（Rust） | 行为与边界 |
|---|---|---|---|
| `get_third_party_notices` | `无` | `Result<String, String>` | 固定 UTF-8 声明全文；release 只读 resources/third-party/ALL.txt，debug 读当前 worktree 固定文件，不接受路径参数；blocking worker 失败返回 notices_unavailable，不读项目或调用外部应用。 |
| `render_preview` | `timelineVersionId: String` | `Result<PreviewResult, String>` | 从持久化时间线生成三画幅 H.264 本地 preview，混入文本、音乐、旁白；返回质量检查与产物引用。 |
| `get_brand_kit` | `projectId: String` | `Result<BrandKitView, String>` | 读取项目品牌套件与默认转场。`logoPreview` 是 data URL，`logoFile`/`fontFile` 只是应用数据目录内的文件名，不含本机路径。 |
| `set_brand_kit` | `projectId: String, input: BrandKitInput` | `Result<BrandKitView, String>` | 校验长度与颜色（`#RRGGBB` 或空），把 logo（png/jpg/webp/svg，≤5 MB）和字体（ttf/otf/woff/woff2，≤20 MB）按内容哈希复制进 `app_data/brand/<projectId>/`，写 `settings_json.brandKit` 与 `defaultTransition`（`none \| crossfade \| dip_to_black`，200–1000 ms）。下次生成生效，不改已有时间线。 |
| `get_preview_cache_status` | `projectId: String` | `Result<PreviewCacheStatus, String>` | 读取当前项目预览中间缓存占用；不访问源媒体。 |
| `clear_preview_cache` | `projectId: String, confirmed: bool` | `Result<PreviewCacheStatus, String>` | 删除 `previews/cache/<projectId>`。必须 `confirmed=true`；不删除 timeline 最终 preview 目录、素材或 SQLite 记录。 |
| `get_release_readiness` | `无` | `Result<ReleaseReadinessReport, String>` | 启动/发行就绪检查。`overall`=`ready\|degraded\|blocked`；每项 `id/title/status/message/messageKey/messageParams`（`status`=`ok\|warn\|fail`；`messageKey` 如 `diskSpace.low` 为稳定文案键，前端按界面语言翻译，未知键回落中文 `message`）。FFmpeg/FFprobe 与 Tesseract/英文数据优先探测安装包资源。不探测源媒体内容，不写库。 |
| `get_runtime_model_status` | `无` | `Result<RuntimeModelStatus, String>` | 查询 BGE/CLIP ONNX 是否已在 `app_data` 或安装包就绪，以及下载进度。`messageParams.model` 为产物 id，前端按界面语言显示模型名。 |
| `start_runtime_model_download` | `无` | `Result<RuntimeModelStatus, String>` | 后台下载缺失的 ONNX 并校验 SHA-256；官方/国内镜像轮换、断点续传与自动重试；幂等；不挡 UI。 |

### Agent 生命周期、审计与配音

| 命令 | 输入（类型） | 返回（Rust） | 行为与边界 |
|---|---|---|---|
| `list_agent_tasks` | `projectId: String, editingTaskId: String, conversationId: Option<String>` | `Result<Vec<AgentTask>, String>` | 返回作用域内的持久化 Agent 调用，按更新时间倒序。 |
| `list_agent_run_steps` | `projectId: String, editingTaskId: String, agentTaskId: String` | `Result<Vec<AgentRunStep>, String>` | 仅在项目、剪辑任务和调用三重作用域匹配时返回步骤；不包含参数、模型原文、对话或媒体证据。 |
| `list_agent_diagnostics` | `projectId: String, editingTaskId: String, agentTaskId: String` | `Result<Vec<AgentDiagnostic>, String>` | 返回同一作用域的本地安全诊断标记；不包含模型原文、会话、路径、凭据或媒体证据。 |
| `list_operation_logs` | `projectId: String, editingTaskId: String, agentTaskId: Option<String>` | `Result<Vec<OperationLog>, String>` | 返回作用域内的副作用审计记录，按创建时间倒序。 |
| `submit_conversation_turn` | `projectId: String, editingTaskId: String, conversationId: String, storyboardVersionId: Option<String>, timelineVersionId: Option<String>, request: String, routeReceipt: String, mediaOptions: Option<crate::media_options::MediaOptions>, uiLocale: Option<String>` | `Result<ConversationTurnResult, String>` | `uiLocale`=`zh-CN\|en`（其余或缺省按 `zh-CN`）写入 `agent_tasks.input_json.uiLocale`，只决定 Agent 回复与系统兜底文案语言，不决定旁白/字幕语言。后端先消费一次性 route receipt，随后普通聊天、澄清、项目事实和工具执行统一创建 Agent task 并进入 NativeToolLoop；不调用对话分类模型、不返回 route/goal decision，也不预选首个工具。异步终态先幂等写入原 conversation，再发出 `agent-edit-completed`。 |
| `execute_agent_edit` | `projectId: String, editingTaskId: String, conversationId: String, storyboardVersionId: Option<String>, timelineVersionId: Option<String>, request: String, routeReceipt: String` | `Result<String, String>` | 兼容入口；必须消费与项目、task、conversation、请求完全匹配的一次性 route receipt，随后才可启动异步 Agent run。 |
| `cancel_agent_edit` | `projectId: String, editingTaskId: String, conversationId: String, agentTaskId: String` | `Result<(), String>` | 将作用域内仍为 `queued`/`running` 的 Agent 任务标为 `cancelled`；NativeToolLoop 在下一步检查点停止并写入取消终态与回复。已是 `cancelled` 视为成功；其他终态不可取消。 |
| `confirm_storyboard_and_preview` | `projectId: String, editingTaskId: String, conversationId: String, storyboardVersionId: String` | `Result<String, String>` | **兼容保留**：历史上在用户确认 storyboard 后异步执行 `create_timeline_draft` + `render_preview` 并返回后台任务 ID。主路径已改为 Agent `generate_storyboard` 成功后自动串联 timeline、preview 与所选输出端口；`src/lib/local-store.ts` 不再封装此命令。 |
| `synthesize_storyboard_voiceover` | `projectId: String, editingTaskId: String, conversationId: String, timelineVersionId: String` | `Result<VoiceoverApplyResult, String>` | storyboard 完成后自动合成整段配音：内置网关时只经 Voycut 网关（服务商由网关决定，默认 ElevenLabs），否则优先 Fish Audio 时间戳流，传输类失败可回退 ElevenLabs；旁白轨必写，alignment 字幕尽力。返回 `voiceoverApplied` / `subtitleApplied` / `provider`。 |

### 编辑器输出端口

| 命令 | 输入（类型） | 返回（Rust） | 行为与边界 |
|---|---|---|---|
| `create_jianying_draft` | `timelineVersionId: String` | `Result<JianyingDraftResult, String>` | 强制新建剪映草稿；支持视频、叠加、受限原生文字、音乐、旁白、品牌图片与转场。主镜头图片在能力表为 Unsupported，不能把品牌 PNG 支持等同于静态素材镜头交付。 |
| `get_jianying_registration_status` | `timelineVersionId: String` | `Result<Option<JianyingRegistrationStatus>, String>` | 读取该时间线最近一次延迟注册任务的 `pending`、`registered` 或 `failed` 投影。 |
| `list_editor_linkers` | `projectId: String` | `Result<EditorLinkerCatalog, String>` | 列出输出端口（剪映 / CapCut / FCPXML / OTIO）及当前项目选择。 |
| `set_output_editor` | `projectId: String, editorId: String` | `Result<EditorLinkerCatalog, String>` | 记住项目输出编辑器；未实现的选择会被拒绝。 |
| `deliver_to_editor` | `timelineVersionId: String, editorId: Option<String>` | `Result<EditorDeliveryResult, String>` | 按选择交付：剪映 / CapCut 新建草稿，FCPXML/OTIO 写出导入文件。`editorId` 为空时用项目已选端口。品牌卡先本地渲染成 PNG 并复制进草稿或导出文件旁；`notes` 如实列出图片交付、未渲染的卡、未带入或待确认的转场、文字降级（code：`brand_cards_as_images`、`brand_cards_failed`、`transitions_unverified`、`transition_not_delivered`、`text_basic_titles`、`text_as_markers`）。 |

## 操作确认入口

前端的会话删除、项目删除与预览缓存清理统一调用 `src/lib/local-store.ts` 的 `await confirmUserAction(message): Promise<boolean>`。入口要求桌面运行时，使用 `@tauri-apps/plugin-dialog` 的 `confirm()`，实际 IPC 为 `plugin:dialog|message`，只将明确的 `true` 视为授权；取消返回 false，插件失败抛出并由调用方显示已有的安全失败提示，两者均不执行后续副作用。正文以及确认框标题、确认/取消按钮取自 `src/lib/i18n` 中英词典。

`src-tauri/capabilities/default.json` 对 `main` 窗口显式声明 `dialog:allow-message`，开发版与正式版共用该配置；保留既有 `dialog:default` 的文件选择等权限。插件已在 Rust 注册，不新增后端命令，不修改删除或缓存清理命令的签名和逻辑。禁止用 `window.confirm`：插件 2.7.2 的注入函数返回 Promise，并调用未注册的旧 `plugin:dialog|confirm` 命令。见 `docs/changes/2026-10-01-confirm-dialog.md`。

## Agent Function Tool 契约

共 **33** 个工具，`tools.rs` 的完整目录与 `policy.rs` 观察/编辑数组、`native.rs::NATIVE_TOOL_NAMES`、`src/lib/agent-tools.ts` 逐一一致。每次 Provider 请求直接携带完整 strict Schema。模型不传 project/task/conversation、路径或 FFmpeg 参数；Rust 从 LoopState 注入并校验作用域。

以下是模型参数 Schema：顶层属性均必须出现，空值写 `null`；嵌套对象只按其 required 声明要求字段（mediaOptions.aspectRatio 可省略）。所有对象 `additionalProperties=false`；`integer` 与 `number` 区分保留，方括号是 Schema 声明的数值/字符/数组项约束。解析入口做协议形状检查，领域执行继续校验真实范围、槽位、样式与授权；Schema 描述与真实执行不同处以下表行为栏为准。

### 观察与查询

| 工具 | 参数 Schema | 结果与执行边界 |
|---|---|---|
| `get_edit_status` | `{  }` | 已实现：读取当前 task 的最新真实 storyboard、timeline 和磁盘 preview，不用最近 Agent task 替代产物事实。 |
| `get_asset_health_summary` | `{  }` | 已实现的只读 Agent 观察工具：返回当前项目持久化的健康计数、活动扫描状态、最近检查时间、脱敏原因码计数以及已解释/未解释失败数量；不访问源文件，不返回路径或原始系统错误。只有全部失败均有原因码时 `reasonEvidenceAvailable=true`。 |
| `list_assets` | `{  }` | 安全素材投影，含技术/视觉状态与证据计数；源引用不进入列表。遗留入口可顺带推进排队分析，Agent 同名观察工具只读。 |
| `get_library_visual_overview` | `{  }` | 已实现的只读 Agent 观察工具：聚合当前项目全部就绪素材的持久化视觉证据，返回频繁出现的主体、动作、场景、示例字幕及叙事角色；适合在写文案或规划分镜前了解实际画面内容。不访问源文件，不返回路径。 |
| `get_asset_visual_detail` | `{ assetId: string [字符:1..200] }` | 已实现的只读 Agent 观察工具：返回当前项目单条已就绪素材的完整片段级视觉证据，包括场景、主体、动作、字幕、叙事角色、镜头类型、摄像机运动及可用时间范围；`assetId` 须为当前项目素材。不返回路径或原始错误。 |
| `search_assets` | `{ query: string \| null [字符:1..200], kind: "video" \| "image" \| "audio" \| "other" \| null, minDurationMs: integer \| null [值:0..*], maxDurationMs: integer \| null [值:0..*], minRating: integer \| null [值:0..5], favoriteOnly: boolean, tag: string \| null [字符:1..200], collectionId: string \| null [字符:1..200], offset: integer [值:0..10000], limit: integer [值:1..20] }` | 已实现的只读 Agent 观察工具：按当前项目检索素材，单页最多 20 条并返回 `nextOffset`；空字符串的 `query`/`kind`/`tag`/`collectionId` 视为 null。自动排除禁止使用素材，只返回安全摘要和固定命中原因码，不返回路径、备注/OCR 正文、媒体内容或完整分析证据。 |
| `search_asset_segments` | `{ query: string [字符:1..200], assetId: string \| null [字符:1..200], offset: integer [值:0..10000], limit: integer [值:1..20] }` | 已实现的片段级只读观察工具：在当前项目已分析的视频/图片中返回明确 `segmentId`、`sourceStartMs/sourceEndMs`、`shotType`、安全视觉标签、固定命中原因和游标；空字符串 `assetId` 视为 null。用第一次段卡检索，不触发模型加深；排除禁止使用及已知缺失、变化或不可读源，不返回路径或 OCR 正文。 |
| `list_voices` | `{  }` | 内置网关时由服务端决定配音服务；无网关时用本机 Fish Audio/ElevenLabs 配置。只列音色，不合成。 |
| `search_music` | `{ query: string [字符:1..200] }` | 查询已配置 Jamendo；只允许可下载 CC0/CC-BY 曲目，返回许可和署名；HTTP 200 的 catalog failed 仍按真实失败处理。 |
| `get_storyboard` | `{  }` | 读取当前打开的任务内故事版，不强制最新版本。 |
| `get_timeline` | `{ timelineVersionId: string \| null }` | 读取所选作用域时间线；null 选择当前版本，不跨任务。 |
| `get_text_capabilities` | `{  }` | 返回本地文字字体/动态/预设及后端兼容矩阵；代码标 verified 不等于本次桌面验证（默认描边/阴影判定差异见关注点）。 |
| `transcribe_asset` | `{ assetId: string [字符:1..200], language: string \| null [字符:1..20] }` | 只校验就绪素材与真实时长，然后返回 local_stub_v1 占位分句（最多 40 段）、固定 confidence=0.55 与 warnings；language=null 实际默认 zh。没有真实 ASR 或模型自动升级路径。 |
| `read_logs` | `{ startLine: integer \| null [值:1..1000000], endLine: integer \| null [值:1..1000000] }` | 固定应用当前日志；null/null 读末尾最多 100 行，正数/正数为闭区间且最多 100 行；单行 500 字符、总预算 3500，输出行号、分页与遮蔽结果；日志不是产物事实。 |

### 分析、创作与交付

| 工具 | 参数 Schema | 结果与执行边界 |
|---|---|---|
| `render_preview` | `{ timelineVersionId: string \| null }` | 从持久化时间线生成三画幅 H.264 本地 preview，混入文本、音乐、旁白；返回质量检查与产物引用。 |
| `request_asset_analysis` | `{ assetIds: Array<string [字符:1..200]> [项:1..100] }` | 已实现：仅重新排队当前项目内已导入、源文件仍可用且尚未 ready/active 的素材分析。 |
| `retry_failed_asset_analysis` | `{ stage: "technical" \| "visual" \| "both" \| null, assetIds: Array<string [字符:1..200]> \| null [项:0..1000], limit: integer \| null [值:1..1000] }` | 已实现：重试当前项目内失败的技术和/或视觉分析。`assetIds=null` 或空数组时自动收集最多 `limit`（默认 1000）条失败素材；显式 `assetIds` 只重试其中仍处失败状态的项。视觉批次入队时自动按 worker 批次上限拆分。返回 `technicalQueued`、`visualQueued`、`skippedCount` 与最多 10 条 `sample`。 |
| `generate_storyboard` | `{ brief: string \| null [字符:1..4000], voiceId: string \| null [字符:1..200], mediaOptions: { voiceover: boolean, subtitles: boolean, bgm: boolean, aspectRatio?: "9:16" \| "16:9" \| "1:1" } \| null, requestedDurationMs: integer \| null [值:1..120000] }` | brief=null 复用任务 brief；只用已 ready 的真实视频证据。配音先于拆拍；合成失败返回 storyboard_voiceover_failed；口播与点名时长相差约 30% 返回 storyboard_needs_user_decision 并保存 brief/暂停工具，下一轮可续跑。P1→P5 后保存故事版/候选，自动时间线、配音/BGM/品牌卡、preview 和所选编辑器交付；子产物失败保留已有版本并如实返回。结果含 appliedMedia、mediaNotApplied、musicTiming、qualityWarnings；不得由开关推断实际轨道。已有旁白禁止改稿；素材不足/选片耗尽提供真实失败或待决定信息。 |
| `create_timeline_draft` | `{  }` | 已实现，支持经验证的图片/视频 storyboard 镜头。 |
| `replace_clips` | `{ timelineVersionId: string \| null, shots: Array<{ shotIndex: integer [值:0..*], assetId: string [字符:1..200], sourceStartMs: integer [值:0..*], sourceEndMs: integer [值:0..*] }> [项:1..100] }` | 已实现，批量替换既有镜头并保持对应时间线时长；素材证据与源范围仍由 Rust 复核。 |
| `insert_clips` | `{ timelineVersionId: string \| null, clips: Array<{ assetId: string [字符:1..200], sourceStartMs: integer [值:0..*], sourceEndMs: integer [值:0..*], durationMs: integer \| null [值:1..*], insertAfterShotIndex: integer \| null [值:0..*] }> [项:1..100] }` | 已实现，在既有时间线插入已验证素材以补足画面时长；`insertAfterShotIndex` 为 null 插到开头。禁止用冻结帧垫时长；配音长于画面时应先搜段再插入，然后重试 `synthesize_voiceover`。 |
| `change_clip_duration` | `{ timelineVersionId: string \| null, adjustments: Array<{ shotIndex: integer [值:0..*], newDurationMs: integer \| null [值:1..*], newSourceStartMs: integer \| null [值:0..*] }> [项:1..100] }` | 已实现，在已验证源范围内重定时长与起止点。 |
| `reselect_shots` | `{ timelineVersionId: string \| null, shotIndexes: Array<integer [值:1..*]> \| null [项:1..5], beatIds: Array<string [字符:1..200]> \| null [项:1..5], instruction: string \| null [字符:*..500], keepCurrent: boolean \| null }` | 已实现：`shotIndexes` 与 `beatIds` 二选一，最多 5 拍。只为这些拍重跑 P2 召回（默认排除当前素材、相邻镜头素材、与冻结镜头相似或重叠的片段、已达复用上限的素材）→ P3 看图选镜 → P4 锁窗精修 → P5 全片校验；新镜头必须正好填满原拍槽位，冻结镜头的素材/源范围/时长/构图须逐字不变，否则不写入。同一拍一轮只能重选一次。池耗尽返回 `storyboard_local_reselect_failed`，不退回整条重跑。结果含每拍 `changes[{ beatId, before, after, matchLevel, remainingAlternates }]`。 |
| `refine_shot_ranges` | `{ timelineVersionId: string \| null, shotIndexes: Array<integer [值:1..*]> [项:1..10], instruction: string \| null [字符:*..500] }` | 已实现：最多 10 个镜头，只跑 P4→P5，素材与片段锁死，时间线时长锁回原槽位，只改入出点与构图。手动插入的 clip 不能精修。 |
| `reorder_clips` | `{ timelineVersionId: string \| null, order: Array<integer [值:0..*]> [项:1..100] }` | 已实现，要求 `order` 为全部既有 `shotIndex` 的完整排列。 |
| `replace_text_tracks` | `{ timelineVersionId: string \| null [字符:1..200], textTracks: Array<text_track> [项:0..21] }` | 已实现：Agent 可替换当前作用域时间线的完整文本轨；cue 只需提供 ID、时间和文案，省略的样式/布局使用安全默认值。成功结果包含非阻断 `qualityWarnings`（阅读密度、超过两行、动画占比和相邻重复文案）。cue 可带可选 `templateId`，后端将其解析成完整且可审计的样式/布局/动态配方，并覆盖冲突字段。交付级 `subtitle_safe`、`headline_rise`、`headline_pop` 与 `headline_drop` 都包含已验证的淡出；后者使用向下滑入。后端校验 cue 时间、颜色、样式/布局、受限动画及唯一 ID，并拒绝跨文本轨的 headline 重叠，且不会接受模型自证 Jianying 兼容性。 |
| `download_music` | `{ trackId: string [字符:1..200] }` | 复核曲目资格后有界下载、保留署名/许可，登记当前项目并排队本地分析；不自动放进时间线。 |
| `use_online_music` | `{ trackId: string [字符:1..200], timelineVersionId: string \| null [字符:1..200] }` | 下载合格曲目、完成本地分析并创建带音乐的新时间线；随后需实际 render_preview 才能证明混音产物存在。 |
| `add_title_cards` | `{ timelineVersionId: string \| null [字符:1..200], cards: Array<title_card> [项:0..4], removeTemplateIds: Array<"opening_title" \| "end_card" \| "corner_logo" \| "info_card"> [项:0..4] }` | 仅在本轮明确请求标题/卡片/logo/片尾时允许；Rust 决定时间/样式和截断文案，结果含 cards、copyAdjustments、notPlaced、editableInEditor=false；写新版本。 |
| `set_transitions` | `{ timelineVersionId: string \| null [字符:1..200], kind: "none" \| "crossfade" \| "dip_to_black", durationMs: integer \| null [值:200..1000], afterShotIndices: Array<integer [值:0..*]> \| null [项:1..100] }` | 仅本轮明确请求转场时允许；null afterShotIndices 改默认并清逐刀覆盖，列表只改所列切点；返回 resolvedTransitions 并创建新版本。 |
| `replace_music_tracks` | `{ timelineVersionId: string \| null [字符:1..200], musicTracks: Array<music_track> [项:0..100] }` | 仅用当前项目 ready 音频，校验源窗/槽位、循环、音量与淡入淡出；替换完整音乐轨并创建新版本。 |
| `synthesize_voiceover` | `{ text: string \| null [字符:1..5000], voiceId: string \| null [字符:1..200], includeSubtitles: boolean \| null, timelineVersionId: string \| null [字符:1..200] }` | 已实现：内置网关时只经网关（默认 ElevenLabs）；否则优先 Fish Audio，传输类失败可回退 ElevenLabs。用户没给文案时由 storyboard 撰写 `narrationText`（`key_message` 通常无旁白，需显式提供 `text` 或 beat `narration`）。禁止朗读 `onScreenText`。真实音频时长写入 `voiceoverTracks`（旁白必成）；alignment 字幕尽力，失败不回滚旁白。结果含 `voiceoverApplied`/`subtitleApplied`/`providerUsed`。相同指纹复用缓存。 |
| `create_jianying_draft` | `{ timelineVersionId: string \| null [字符:1..200] }` | 强制新建剪映草稿；支持视频、叠加、受限原生文字、音乐、旁白、品牌图片与转场。主镜头图片在能力表为 Unsupported，不能把品牌 PNG 支持等同于静态素材镜头交付。 |

### 复用的嵌套 Schema

下列名称仅是本页缩写，与 `tools.rs` helper 一致。text cue 的 nullable 样式/布局/动态可交 null 使用默认；有模板时由 Rust 解析配方，不能让模型宣称编辑器兼容性。

- `title_card` = `{ templateId: "opening_title" | "end_card" | "corner_logo" | "info_card", shotIndex: integer | null [值:0..*], headline: string | null [字符:*..120], subline: string | null [字符:*..120], cta: string | null [字符:*..80] }`

- `text_track` = `{ id: string [字符:1..200], role: "subtitle" | "headline" | "callout" | "cta" | "label", layer: integer [值:0..20], enabled: boolean, cues: Array<text_cue> [项:0..100] }`

- `text_cue` = `{ id: string [字符:1..200], templateId: "subtitle_safe" | "headline_rise" | "headline_pop" | "headline_drop" | "callout_card" | "cta_card" | null, startMs: integer [值:0..*], endMs: integer [值:1..*], text: string [字符:1..280], style: nullable_text_style, layout: nullable_text_layout, entrance: nullable_text_animation, exit: nullable_text_animation, loopAnimation: nullable_text_animation }`

- `nullable_text_style` = `{ fontKey: string [字符:1..200], fontSize: number [值:0.01..0.3], bold: boolean, color: string [pattern=^#[0-9A-Fa-f]{6}$], strokeColor: string | null [pattern=^#[0-9A-Fa-f]{6}$], strokeWidth: number [值:0.0..10.0], shadow: boolean, backgroundColor: string | null [pattern=^#[0-9A-Fa-f]{6}$], alignment: "left" | "center" | "right", letterSpacing: integer [值:-100..100], lineSpacing: integer [值:-100..100] } | null`

- `nullable_text_layout` = `{ anchor: "top" | "center" | "bottom", x: number [值:0.0..1.0], y: number [值:0.0..1.0], maxWidth: number [值:0.2..1.0], safeArea: "title_safe" | "action_safe" } | null`

- `nullable_text_animation` = `{ templateId: "fade" | "slide_up" | "slide_down" | "pop" | "wipe", durationMs: integer [值:0..*], intensity: number [值:0.0..1.0] } | null`

- `music_track` = `{ id: string [字符:1..200], enabled: boolean, cues: Array<music_cue> [项:0..100] }`

- `music_cue` = `{ id: string [字符:1..200], assetId: string [字符:1..200], sourceStartMs: integer [值:0..*], sourceEndMs: integer [值:1..*], timelineStartMs: integer [值:0..*], timelineEndMs: integer [值:1..*], loopEnabled: boolean | null, volume: number [值:0.0..2.0], fadeInMs: integer | null [值:0..*], fadeOutMs: integer | null [值:0..*] }`

## 桌面复合参数与状态 DTO

- `FellowCutAccountStatus`：state、email、entitlement、trialStartedAt、accountPageUrl；后四项可空，accountPageUrl 由内置网关地址推出。token 不回前端；资格读取只读，模型授权由服务端请求校验，账号错误带 account_* 稳定码。
- `PreviewResult`：timelineVersionId、previewPath、qualityReport；`LatestTimeline`：timeline 和可空 preview。`JianyingDraftResult`：draftDirectory、draftContentPath、registrationStatus、notes；`JianyingRegistrationStatus`：timelineVersionId、draftName、status。路径仅供本地桌面读取，不记普通日志或浏览器存储，不证明编辑器已经打开。
- `StoryboardVersion`：id/projectId/editingTaskId/versionNumber/brief/title/summary/targetDurationMs/scriptMode/beats/uncoveredBeatIds/shots/createdAt，加性平铺派生字段见后文。`TimelineVersion`：id/projectId/storyboardVersionId/versionNumber/clips/textTracks/musicTracks/voiceoverTracks/overlayClips/qualityReport/createdAt；品牌 graphicOverlays/transitions 平铺，不另造产品模型。

- `BrandKitInput`：name、handle、cta、primaryColor、accentColor 必填字符串，logoSourcePath/fontSourcePath 可空，clearLogo/clearFont 默认 false；defaultTransition 为 `{kind,durationMs}`。BrandKitView 平铺 name/handle/cta/colors、logoFile/fontFile（应用内文件名）、logoPreview（data URL）、fontName 和 defaultTransition，不回传源路径。
- `AssetAnalysisProgress`/AssetPage.progress：total、ready、analyzing、queued、failed、readyVideo、cancelled；分析数量不受分页筛选影响。取消/继续返回处理数量，rename_library_asset 返回 void，remove_library_assets 返回 requested/updated/skipped 批量计数。
- `EditorLinkerCatalog`：selectedId、linkers（id/label/summary/implemented/available/deliveryKind）；注册表发现草稿库决定 dropInDraft 可用性，importFile 可用性来自实现能力。set_output_editor 保存选择，不修改当前时间线。
- `ReleaseReadinessReport`：overall 与 checks；每项 id/title/status/message/messageKey/messageParams 为展示投影。`RuntimeModelStatus`：overall/currentId/message/messageKey/messageParams/artifacts；runtime-model-progress 发同结构。`PreviewCacheStatus`：projectId/bytesUsed/limitBytes/fileCount。具体可空值与扩展字段以对应 Rust/bridge 类型为准。
- `AgentEditResult`：agentTaskId、message、storyboard、timeline、preview、jianyingDraft（产物可空）；完成事件另带 status。错误码、质量警告与实际轨道是事实，message 不单独证明成功。

## 会话、Provider 与产物返回规则

- Task Resolver 只处理任务归属，不选择工具；候选仅为活动任务，无活动任务则创建 task + conversation。`TaskRouteResult` 包含 action、taskId、conversationId、confidence、question、suggestedTitle、reasonCode、deferredRequest、routeReceipt。任务确定前不写消息；user 消息须占用绑定完整请求的 receipt，提交时只消费一次。

- `submit_conversation_turn` 返回 `{kind:"run",agentTaskId}`；类型保留 `{kind:"immediate",status:"response"|"clarification",message}` 兼容变体。后台原子保存终态、固定 ID 完成消息和会话状态后才发 `agent-edit-completed`。前端缓存最多 20 个早到事件，并以 1.2 秒轮询/重新读库恢复；切换作用域不应用旧产物。任务状态包括 queued/running/completed/partially_completed/failed/needs_clarification/cancelled，启动中断变为 needs_review，不重放未知副作用。

- NativeLoop 最多 10 步，单步上限 **180 秒**、整轮 **1800 秒**；storyboard 独立模型请求上限 **120 秒**。剩余整轮截止时间约束同步 HTTP/配音/媒体进程；ONNX 只能在阶段边界检查。模型 function_call 输出按 call_id 对齐，传输重试不重复已执行工具；产物完成由 RunReceipt 与持久化事实裁决。

- 每轮先注入当前作用域的安全状态快照，写工具后刷新；完整请求超过 40K token 压缩到 30K，硬上限 60K，保护快照、当前用户消息与最近调用/结果对。快照本身可提供成功观察，高层事实问答不必强制另调工具。

- 内置网关构建只走 Voycut 服务，失败不回退本机 Provider/配音 key。无网关构建由 ModelAccess 选择自定义 OpenAI 兼容 API（可另设粗视觉 model）或实验性 OAuth；凭据读取错误封闭失败。Chat/Responses 适配保留原生工具、tool_call_id、stream 与响应 output。单请求最多 4 张图；429 按 Retry-After 退避最多 4 次、单次最多 30 秒，耗尽返回真实失败。网关登录/资格/版本/413 等 `provider_gateway_*` 为终止错误。

- 无网关的配音仍优先 Fish Audio，仅传输/5xx/超时类失败可回退已配置 ElevenLabs，认证失败不回退；内置网关由服务端决定 ElevenLabs 或备选 Fish，不自动回退。`VoiceAvailability` 公开返回 `{available,viaGateway,reason}`，暂时性探测失败仍保持开关可用；`GatewayVoices` 的内部 provider/modelId/defaultVoiceId 用于合成解析和缓存指纹，不混同这两个契约。

- `StoryboardVersion` 的 beatId、matchLevel（direct/contextual）、beats、uncoveredBeatIds、derivedFromVersionId、changedBeatIds 为加性字段；旧版本使用空集合/空值与 contextual 回落。uncovered 不映射 clip，也不能宣称已被画面覆盖。当前快照保存 mediaOptions、音乐 musicPlan；实际落地以 appliedMedia/mediaNotApplied 为准。

- 版本号由 task_version_number 供 UI/Agent 展示，旧版本空值回退项目内 version_number；新 task 从 v1，旧任务继续自己的最大号。所有写工具保留原版本；局部选镜写派生故事版+时间线同事务，冻结其他镜头与音文轨。

- `StudioCommitPayload` 外层是 `{payload: ...}`；项目/task/timeline 与 clipReplacements 的显式 serde 名为 camelCase，text_tracks、deleted_shot_indices、overlay_inserted、overlay_deleted_shot_indices、overlay_adjustments、overlay_reorder、music_tracks、voiceover_tracks 按 Rust 字段名发送（bridge 负责映射）。子项字段以 studio.rs 声明为准；它不是任意 JSON/SQL 写入口。

- `AssetEvidence` 含 durationMs、analysisVersion、visualAnalysisStatus、visualAnalysisNote、关键帧/OCR/视觉证据与真实 segments；运动可用窗/曲线及 `VisualEvidence.detail` 是可选加性字段。仅显式素材详情预览入口把选中源媒体转换成受限 asset URL，库列表不暴露原始路径。

- 文本轨 cue 在 timeline 内保存角色/layer、时间、样式/布局/动态与兼容判定；ASS preview 与编辑器原生文字能力分别校验。品牌图片文字不可编辑。转场时长夹到相邻较短镜头 40%，默认硬切；FCPXML/OTIO 黑场过渡保留硬切并返回说明。

- `EditorDeliveryResult` 含 editorId、deliveryKind（dropInDraft/importFile）、status、displayName、message、outputPath、jianying、notes；剪映/CapCut 草稿注册可能 pending，文件输出到 editor-handoffs。四端口均有旁白音轨写入代码；剪映/CapCut 主轨静态图片在能力表 Unsupported，品牌图片为独立叠加。交付仅新建、不覆盖、不反向同步；本次编辑器打开效果未核实。

- `runtime-model-progress`、`assets-changed`（项目 ID）、`agent-edit-completed` 是通知；持久化查询仍是恢复事实。debug 的 Native Provider 完整转储只在显式开关/tauri:dev 下写 gitignored target，release 强制关闭，不能通过 read_logs 读取。

## 源码核对入口

`src-tauri/src/lib.rs`（101 项注册）→ 各命令函数参数/返回 → `src/lib/local-store.ts`（95 个静态 invoke 名称，注册的兼容命令可无 wrapper）；`agentloop/tools.rs`（33 项 Schema）→ `policy.rs` / `native.rs` 白名单 → `skills.rs` 分派与各领域校验。检查与协作规则见 [harness.md](harness.md) 和 `CONTRIBUTING.md`。官方 OAuth 的外部支持范围与刷新行为未核实；本页不把原型连接描述为官方稳定契约。


## 片段证据契约 v1（2026-10-05）

`get_asset_evidence` 的 `AssetEvidence` 追加 `segmentEvidence: SegmentEvidence[]`。不新增 IPC 或 Agent 工具，不改变当前生成/导入默认路径。Rust 类型由 `models.rs` 拥有，适配与读取在 `assets/evidence_contract.rs`，补核验在 `assets/evidence_verification.rs`。

| 类型 | 字段 / 语义 |
|---|---|
| `SegmentEvidence` | `schemaVersion=1`、`id`、`analysisSnapshotId`、`assetId`、`segmentId`、`range`、`source`、`risks[]`、`visualEvidence[]`、`relations`、`motionProfile`、`caption`、`narrativeRole` |
| `RiskEvidence` | 内容寻址 `id`、`risk`、`state=hit/not_hit/unknown`、`source`、半开源窗 `range={startMs,endMs}`、`confidence: number/null`、`value`（可见标签、严重程度、说明） |
| `EvidenceSource` | `analysisId`、`model: string/null`、`method`、`analysisVersion`；旧分析模型无法恢复时明确为 null，不用当前模型配置冒充旧来源 |
| 风险名称 | `brand_logo/exhibition/out_of_focus/shake/clutter/on_screen_text/crowd/motion_blur/staged/advertising/empty_shot`；风险事实不等于体裁过滤，不做人为素材分类 |
| 关系证据 | `relations` 原样保留 `ShotDetail` 的主体位置与左右边界、主体/相机方向、开头结尾是否干净、最佳源窗、高光时点、变化等；`visualEvidence[]` 也保留原始描述与 detail；无轴线/人物一致性证据时不声称可裁决跳轴/连续性 |

旧正向标签保留 `hit`，无时段按整段；缺字段、无卡、旧空数组/false/锐利等否定标签及 `None.` 占位都为 `unknown`。旧置信度不编造；新 `not_hit` 必须有置信度，只能证明自身覆盖的源窗；段内其他时段仍未知。补核验结果追加保存，旧命中不被新阴性覆盖；冲突按命中优先、未知不放行。`risk_state_for_window` 返回给定最终源窗的三态，不把其他时段的阳性挪到当前窗，也不把几个零散阴性拼成安全。

Rust 内部 `verify_candidates(app, projectId, &[EvidenceVerificationRequest])` 输入 `assetId/segmentId/analysisSnapshotId/risks[]` 与可选 `range`（默认整段，必须位于基础片段内），复核项目可访问性、ready、排除与健康状态，只向模型询问候选未知项。每段最多四张时间网格；请求通过统一 Provider 按需并发，传输层仅 429 按 Retry-After 退避，不自动切换模型。逐候选返回真实结果或错误，缺项保持未知，越窗/未请求字段拒绝；回写事务重新核对基础快照，分析已更新则拒绝。该能力供任务 3/4 接入，本任务不自动调用它。

## 新故事版预留事实元数据

`StoryboardEvidenceMetadata` 包含 `pipelineVersion: string|null`、`genre: narrative/promotion/bts|null`、`recipeVersion: string|null`、`evidenceSnapshot: SegmentEvidence[]`、`evidenceReferences: EvidenceReference[]`。引用包括 `evidenceId/assetId/segmentId/range/supports`。

schema v21 只追加 `storyboard_evidence_metadata(storyboard_version_id,metadata_json)` 与 `asset_evidence_verifications`，不重建表、不回填旧行。新故事版在创建事务中调用 `write_storyboard_metadata`，校验版本、快照内容 ID 和引用范围，已有记录不可覆盖；`read_storyboard_metadata` 对无附加行的历史版本返回三个 null 和两个空数组，历史版本不能因此获得新管线合格标签。现行 `StoryboardContent/StoryboardVersion` 及写库不变；预留 `StoryboardVersionWithEvidence` 与 `project_storyboard_version` 加性投影（既有版本字段与附加字段在 JSON 根平铺），任务 6 负责将这份冻结的附加契约接入新生成、读取与派生结果，任务 7 可消费类型，无需再改公共字段定义。
