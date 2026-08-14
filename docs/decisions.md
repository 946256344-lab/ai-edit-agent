# 技术决策记录

## ADR-053：Agent 完成事实由持久化状态交付，事件仅作通知

- 状态：已实现，待桌面事件丢失场景手工验证
- 决策：Agent run 终态由后端在发出 `agent-edit-completed` 前，以 `agent-task-result-{agentTaskId}` 为确定性消息 ID，把最终回复幂等写入原 conversation；不存在更新请求或更新活动任务时才把会话恢复为 `ready`。前端保留早到事件缓存，同时在发送中或当前任务活跃时轮询 `agent_tasks`；事件或轮询任一方先发现终态，都按原项目、task、conversation 重载持久化消息与领域产物。执行卡为 `finish` 使用“整理并回答”文案。项目事实问答已有成功观察且结果包含所问事实时，模型提示要求直接 `finish`，不得调用语义重叠观察工具只为确认。
- 原因：Tauri 事件是瞬时通知，监听重建、竞态或前端状态更新失败都可能让任务卡永久保留旧 `running`，并使只存在事件 payload 中的最终回答丢失；步骤轮询本身不能更新父级任务终态。项目事实问答中重叠观察还会增加无必要模型往返。
- 后果：SQLite 的 `agent_tasks`、`messages` 和领域版本成为恢复的完整事实来源；事件丢失只影响低延迟展示，不影响最终回复或任务停止。无需新增表或公开命令，重复完成通知不会重复消息。前端轮询只在发送或活动任务期间运行；模型仍负责选择首个项目观察工具，后端只强化观察后的收敛提示，不新增关键词业务直通分支。对话工作区已从 `App.tsx` 拆出到独立 `ConversationWorkspace` 组件，路由状态被显式显示在 composer 周边，以减少“已归属/澄清/创建新任务”的歧义。另一个实现细节是：当任务路由/对话路由本身失败时，系统不再直接吐出“无法可靠读取真实状态”的固定拒答，而是允许继续进入 Agent 路由/问答流程，从而避免把基础问答误拦成工具失败。

## ADR-048：用户素材元数据与分析证据分层持久化

- 状态：已实现，待桌面视觉验收
- 决策：schema v12 使用独立关系表保存收藏、评分、备注、禁止使用、用户标签和素材集合；不得混入会被重新分析替换的 `assets.metadata_json`。批量写命令限制为同项目 1–200 条并记录不含用户正文的操作审计。“禁止使用”从后续 storyboard 候选中硬排除，但不改写历史产物。
- 原因：用户整理结果属于长期人工判断，生命周期不同于技术、OCR 或 Provider 视觉证据。
- 后果：搜索和分页支持用户元数据；集合只保存引用，不移动源文件。删除或重命名集合尚未开放。

## ADR-048：以脱敏事实驱动 Agent 失败说明

- 状态：已实现
- 决策：Agent 工具执行失败时，不把原始日志或底层错误原文交给模型，也不把所有用户文案写死在前端。后端在内存中把真实失败转换为结构化诊断，限定为操作、阶段、安全码、计数事实、可重试性和恢复建议，再回读同一有界循环；模型可选择继续使用合法工具，或以 `finish.answer` 自然解释失败。无真实产物时终态仍强制为 `failed`，用户消息带确定性的“任务未完成”前缀；明显声称已生成、已创建或成功完成的矛盾说明会被拒绝并回退固定文案。模型不可用或未给出解释时继续使用固定诚实降级。
- 原因：单一 `missing_or_invalid_prerequisite` 会掩盖“已有视觉证据但源文件均不可访问”等可恢复事实；直接发送日志则可能泄露本机路径、媒体内容、用户请求或 Provider 信息。
- 后果：步骤审计仍只持久化安全码，不新增含 payload 的表或公开字段。storyboard 源筛选会产生不含路径的候选数与可访问数；模型负责语言表达，Rust 继续负责事实、作用域、状态和产物完成门。

## ADR-047：以安全步骤和真实产物呈现 Agent 执行过程

- 状态：已实现，待桌面视觉验收
- 决策：对话区显示当前作用域最近一次 Agent run 的可折叠执行卡。运行中默认展开并轮询既有 `list_agent_run_steps` 作用域查询；终态保留简洁摘要。UI 只用固定白名单把工具名映射为用户动作，并从步骤终态和 `artifact_type` 展示完成数、耗时与真实产物，不展示模型 `reason`、prompt、参数、错误原文、本机路径或媒体证据。
- 原因：发送按钮的“处理中”无法说明当前动作和已完成内容；直接直播模型输出又会泄露不稳定内部信息，并可能把模型声称完成误当成真实产物。
- 后果：执行卡不引入新的持久化或公开命令契约，继续以 `agent_tasks`、payload-free `agent_run_steps` 和后端产物完成门为事实来源。远端模型阶段使用不确定动画和步骤计数，不伪造百分比；后台素材分析继续使用独立右下角提示。暂停、恢复和人工审阅后的明确续跑入口仍为 TODO。

## ADR-046：任务归属先于任务内会话路由

- 状态：已实现，待真实桌面 Provider 验证
- 决策：在 `submit_conversation_turn` 前增加项目级 `resolve_conversation_task`。它只基于最近 12 个 `task_state_snapshots` 选择继续当前任务、切换已有任务、原子创建新任务/会话或澄清；任务快照保存受限目标、当前子目标、真实产物阶段/标识、完成项与安全状态，不使用 `conversations.summary` 或完整历史。任何模型自动归属都要求至少 0.85 置信度；不确定请求保存在 `pending_task_routes`。确定目标后签发绑定项目、确切 task、完整请求与可选 pending 记录的一次性 route receipt，公开提交入口必须在后端消费凭证，pending 也只在消费时 resolved。
- 原因：原 Conversation Router 接收前端已经选定的 `projectId/editingTaskId/conversationId`，只能判断一句话在当前任务内应直接回答、澄清或执行，无法处理“切回刚才的任务”或防止无关请求污染当前任务历史。
- 后果：Task Resolver 不规划工具，任务确定后仍由 Conversation Router 和十步 Agent loop 负责意图、技能与结果门。schema 升至 v10；`pending_task_routes`、严格绑定 task/conversation/唯一 user message 的 `task_route_receipts`、任务内 `pending_clarifications` 分层管理。`create_message(role=user)` 同样要求并原子占用未消费凭证，`pendingAction=keep` 不再替换旧请求；同一 pending 的并发凭证只有一个能成功消费。当前子目标由已归属的真实用户请求更新，产物事实每次从领域表重建；真实 Provider 下的跨任务语言理解仍需桌面验收。

## ADR-044：合并首次目标决策与交互优先 Provider 调度

- 状态：已实现，待真实桌面 Provider 验证
- 决策：确定性快路径仍锁定明确目标；模糊请求不再单独调用意图分类模型，而由主 Agent 首次响应在同一 JSON 中声明 `goal`/`isQuestion` 并选择第一个技能或直接回答。最近一次同作用域调用若为 `needs_clarification`，首次决策会收到待澄清标记并结合完整历史理解用户补充。交互模型调用优先于尚未开始的粗视觉调用；粗视觉连续三次失败后熔断 60 秒，只允许单一半开探测。Agent 模型决策总预算为 90 秒，每步使用剩余预算；已开始的副作用不强制中止。
- 原因：独立分类加多轮工具决策使简单请求产生重复模型往返；后台视觉失败风暴会与用户交互争抢 Provider；标题式疑问句的长文案也可能被分类器误当成问答。
- 后果：问答可在首轮直接 `finish`，创作文案可在首轮直接选择 storyboard 技能；后端真实产物完成门、作用域校验、工具白名单和诚实降级保持不变。熔断期间视觉批次保持 `queued` 并在冷却后恢复，不把 Provider 暂时故障扩散为批量终态失败。诊断只保存固定数字耗时，不保存模型原文或用户内容。
- 补充：精确的“剪好了吗/完成了吗”属于真正无需模型判断的只读单命令，经 `get_edit_status` 查询上一条同作用域 Agent 任务和真实产物标识；后台分析任务和重复状态查询会被排除。

## ADR-045：Conversation Router 分离即时轮次与 Agent run

- 状态：已实现，待真实桌面 Provider 验证
- 决策：新增 `submit_conversation_turn` 作为对话入口。它在后端校验作用域后，使用一次首轮路由响应选择 `respond`、`clarify` 或 `run`；即时响应不创建 `agent_tasks`，执行型请求才创建异步 Agent run。首轮执行工具通过初始化技能注入既有循环作为 step 1，保留工具白名单、作用域校验、版本审计与真实产物完成门。
- 原因：原 `execute_agent_edit` 把所有消息都包装成 Agent run，简单状态问题也进入多步循环；循环首轮选出的工具还会再次经过模型决策，造成额外延迟和路由漂移。
- 后果：前端以判别式结果处理即时回复或异步 run；异步完成事件只属于 run，不会和即时回复重复。旧命令保留用于兼容，不作为新的前端对话入口。schema v7 增加内部 `pending_clarifications`：问题按三重作用域持久化，路由同轮明确 `keep/resolve`，新问题 supersede 旧问题；任务创建/解决旧问题与 `needs_clarification` 终态/保存新问题分别原子提交。完整独立 `ConversationRouterSnapshot` 类型仍可后续提取，但不再影响澄清可靠性。

## ADR-038：工具优先的 Agent 操作空间

- 状态：已接受
- 决策：自然语言 Agent 向模型提供受限工具、当前状态快照、真实副作用反馈与有界步骤预算；不得以关键词把普通业务意图改造成越来越多的硬编码直通分支或少数固定操作。意图分类只用于产物完成门，模型在循环中选择实际观察、编辑或交付工具。
- 原因：用户表达常有上下文和多步意图；把“分析素材”等请求硬编码为单一操作会错误缩小模型操作空间，并绕开模型对当前素材状态和后续动作的判断。
- 后果：显式单命令、确认门、作用域/范围校验与安全降级仍可确定性执行；普通请求必须通过模型工具循环，工具失败和真实结果回读模型而不是由关键词分流代替决策。

## ADR-043：目标优先视觉批次、连接复用与粗视觉模型

- 状态：已实现，待真实 Provider 桌面验证
- 决策：storyboard brief 只在本地与显示名、文件夹组织 hint 和 OCR 做词汇重合排序，并只把纯数字优先级写入 queued 视觉批次；最高相关批次最多等待 65 秒。Provider 请求共享进程级 HTTP Agent。自定义 API 可配置独立粗视觉 Model，空值沿用主 Model；OAuth 不使用未经验证的替代模型。
- 原因：按导入顺序分析会先消耗低相关素材；重复连接建立增加批次延迟；粗分类无需强制使用主模型。但文件名和文件夹不能冒充媒体事实。
- 后果：文件名、文件夹和路径不进入 Provider；OCR 不进入粗视觉 payload，但仍是 storyboard 可用的本地提取文字证据，不能冒充画面语义。无命中与同分按创建时间和任务 ID 稳定排序。API Key 与两个模型配置继续只保存于 Windows Credential Manager，状态只返回模型名。

## ADR-042：低帧率低分辨率场景检测

- 状态：已实现
- 决策：首次场景检测在前 30 秒内先限制为 4 fps，再以 fast bilinear 缩放到 320 像素宽，最后运行 FFmpeg `scene` 比较与关键帧选择；源时间继续使用 `showinfo` 的 `pts_time`。
- 原因：原链路先在原始帧率和分辨率上比较画面，再缩放输出，4K/60fps 素材会浪费大量计算。场景候选只需要粗粒度变化，不需要逐帧全分辨率比较。
- 后果：可能漏掉持续不足 250ms 的极短切换，但首次分析吞吐明显提高；90 秒 1080p 测试视频的相同 30 秒扫描窗口平均从 3317ms 降至 2280ms，约快 31.3%。

## ADR-041：有界的本地媒体分析并发与超时

- 状态：已实现
- 决策：本地技术分析最多运行两个 worker；FFprobe、缩略图、场景扫描、回退抽帧和 Tesseract 均使用有界等待。Windows 超时时通过隐藏的 `taskkill /T /F` 请求终止进程树，并只在短清理窗口内回收直接子进程；若系统拒绝终止请求，调用不会无限等待。启动时中断的本地 `running` 任务重新排队。
- 原因：实测一个 FFmpeg 分析进程可使 767 条排队任务停滞超过 19 小时。单 worker 避免资源打满但无法绕过卡死素材；无限并发又会重现 CPU、磁盘和 SQLite 竞争。
- 后果：正常吞吐最多提高到两个并行素材；超时或无法启动的素材会失败，队列继续推进。阶段级安全耗时指标仍为 TODO，不能记录路径、媒体内容或模型原文。

## ADR-040：本地优先与批量视觉素材识别

- 状态：已实现，待真实 Provider 批量响应桌面验证
- 决策：`analyze_asset` 只生成本地技术证据并使素材技术就绪；最多 6 条技术就绪素材由单一后台 worker 组成 `analyze_asset_visual_batch`，每条只发送一张中间代表帧及受控素材 ID/源时间标签。响应只能回填同批次的 ID 与时间。视觉状态独立保存为 `queued`、`running`、`ready`、`failed` 或 `skipped`；storyboard 仅使用 `ready` 且有视觉证据的素材。
- 原因：每个素材串行请求视觉模型会把导入延迟放大为网络等待，并迫使 storyboard 在视觉证据尚未到达时选择素材。批量粗识别减少请求轮次，同时保持素材到画面的可验证映射。
- 后果：视觉任务失败不影响本地浏览或技术 `ready`，也不自动无限重试；任务 payload 与安全结果不得包含路径、OCR 正文、图像或模型原文。更细的候选多帧精检仍为 TODO。

## ADR-039：有界的首次素材证据采样

- 状态：已实现
- 决策：首次视频分析只扫描前 30 秒、最多生成 4 张场景关键帧，并对其中前 2 张执行 OCR。视觉建议调度由 ADR-040 取代；技术分析完成后即标记为 `ready`。
- 原因：原先前 90 秒、最多 8 帧和 3 次远端视觉请求会让串行单 worker 长时间占用，令刚导入的素材迟迟不可用。缩小首次采样可显著减少解码、子进程和网络等待，同时保留可核查的时间绑定证据。
- 后果：首次 storyboard 的画面覆盖面较窄；更深度的按需采样仍为 TODO。不得把 `ready` 提前赋予尚在生成证据的素材。

## ADR-001：Agent-first 交互

- 状态：已接受
- 决策：以自然语言 Agent 会话作为主交互，storyboard、内部时间线、preview 与交付物均是可检查的 Agent 工具产物。
- 原因：产品目标是持续协作的剪辑 Agent，不是附加聊天栏的传统剪辑器。

## ADR-002：先规划，后组接媒体

- 状态：已接受
- 决策：Agent 必须基于真实媒体片段的分析证据生成故事和 storyboard，再创建时间线。
- 原因：避免随机拼接，并让用户可理解和修订创作方向。

## ADR-003：自主创建草稿和 preview

- 状态：已接受
- 决策：Agent 可不经逐项确认创建内部时间线、低清 preview 和新的 Jianying draft。
- 后果：产物必须版本化、可恢复；最终导出、覆盖和删除仍需明确确认。

## ADR-004：本地优先的媒体与项目存储

- 状态：已接受
- 决策：原始媒体、项目数据、preview、内部时间线和 Jianying draft 默认保留本地；初始导入保存引用。
- 后果：必须检测缺失文件，未来提供收集项目媒体能力。

## ADR-005：可替换的模型 Provider

- 状态：已接受
- 决策：OpenAI OAuth 只是可选入口之一；Agent 和工具不依赖厂商专有格式。
- 后果：自定义托管 API 和本地模型适配器仍为 `TODO`。

## ADR-006：安全的桌面 OAuth

- 状态：目标已接受；实验性实现已完成，官方方案待验证
- 决策：使用系统浏览器 PKCE，并将刷新凭据保存至 Windows Credential Manager。
- 后果：浏览器 localStorage 与项目数据不能存放凭据；官方 scope 和 Provider 支持必须验证。

## ADR-007：通过 pyJianYingDraft 创建 Jianying draft

- 状态：已实现，仅视频映射已人工验证
- 决策：用 `pyJianYingDraft` 从内部时间线创建新的 Jianying Pro 草稿目录。
- 后果：内部时间线为事实来源；不得覆盖、不得反向同步。图片、文本和音频映射仍为 `TODO`。

## ADR-008：首个产品格式

- 状态：已接受
- 决策：MVP 面向 15 至 30 秒、9:16、英文字幕的短促销视频。
- 后果：数字人、AI 生成画面、复杂特效和商用音乐不在首个阶段。

## ADR-009：Tauri 桌面壳

- 状态：已接受并实现
- 决策：以 Tauri 2 封装 React 前端，并由 Rust 作为本地副作用和安全边界。

## ADR-010：Rust 管理 SQLite

- 状态：已接受并实现基础能力
- 决策：通过 Rust `rusqlite` 管理 SQLite；前端只能经命名 Tauri 命令访问。
- 后果：迁移归 `db.rs` 管理；通用 Agent 调用审计和查询契约仍待补齐。

## ADR-011：原生文件选择与素材引用

- 状态：已接受并实现
- 决策：使用 Tauri dialog 选择本地媒体并持久化源文件引用，不复制或修改源文件。

## ADR-012：FFprobe 作为首个后台媒体任务

- 状态：已接受并实现
- 决策：导入素材时创建持久化 `analyze_asset` 任务，以 FFprobe 保存时长、尺寸、帧率和音频轨事实。
- 后果：未完成任务可恢复；生产运行时供应仍为 `TODO`。

## ADR-013：通过 Tauri asset 协议缓存缩略图

- 状态：已接受并实现
- 决策：在应用数据目录生成 320 像素 JPEG 缩略图，并仅通过作用域 `asset:` 协议展示。

## ADR-014：阈值场景候选和回退关键帧

- 状态：已接受；采样上限由 ADR-039 更新
- 决策：以 FFmpeg `scene` 过滤器的 0.30 阈值生成最多四张关键帧；无明显切换时提取首、中、尾帧。
- 后果：结果是启发式候选，不能表述为语义理解。

## ADR-015：本地 Tesseract OCR 证据

- 状态：已接受并实现
- 决策：对原图和视频关键帧执行英文 OCR，并保存对应源时间。
- 后果：OCR 失败不使技术分析失败；中文 OCR、文字框仍为 `TODO`。

## ADR-016：实验性 OpenCode 兼容 ChatGPT OAuth

- 状态：仅个人测试的实验性实现
- 决策：使用 OpenCode 兼容的 loopback PKCE 流程，明确不将其称为官方 OpenAI 第三方集成。
- 后果：凭据仅由 Windows 原生 `keyring` 存储；令牌刷新与模型访问仍需人工验证，可能随上游变化失效。

## ADR-017：最小帧实验性视觉分析

- 状态：已实现，待真实模型访问验证；采样上限由 ADR-039 更新
- 决策：每段视频最多发送两张生成的关键帧，每个图片最多发送一张缩略图；保存带源时间的视觉建议。
- 后果：原始媒体和全分辨率图片不外发；模型输出是建议而非事实。

## ADR-018：证据校验的 storyboard 版本

- 状态：已实现，待真实 Provider 响应验证
- 决策：只在本地验证素材 ID、媒体类型、源时间范围、镜头顺序和总时长后持久化 storyboard。
- 后果：无效模型 JSON 或超范围引用不会创建版本；用户必须提供非空 brief。

## ADR-019：受限的模型编辑工具

- 状态：已实现并冒烟测试
- 决策：模型只能选择 storyboard、时间线、局部片段替换、preview、Jianying draft 或无操作；Rust 验证并执行副作用。
- 后果：模型无文件系统权限；替换只能修改既有镜头且必须创建版本和审计记录。通用持久化调用状态仍为 `TODO`。

## ADR-020：本地低分辨率视觉相似候选

- 状态：已实现
- 决策：比较每段渲染片段中点处的 `24 x 24` 灰度帧，平均像素差小于 12 时提示候选。
- 后果：这是审阅提示，不是语义重复结论，不替代未来多帧或 embedding 检测。

## ADR-021：Jianying 8.0 仅视频适配器

- 状态：已实现并人工验证
- 决策：用安装的 `pyJianYingDraft` 在探测到的 Jianying Pro 8.0 草稿根目录创建唯一草稿，禁止覆盖。
- 后果：Jianying 必须关闭；注册表写入跨进程串行化；图片和其他编辑轨仍为 `TODO`。受限文本与实验性本地音乐轨分别按已验证的适配器能力生成新草稿，绝不覆盖或反向同步。若唯一目录创建后的轨道构建、保存或注册失败，适配器只回滚本次未成功交付的新目录，既有 draft 不受影响。

### ADR-041：Jianying 音乐采用本地受控素材与实验性交付

- 决策：音乐只从当前 local project 的 `ready` 音频素材读取；适配器以 `AudioMaterial`/`AudioSegment` 写入独立 Jianying 音频轨，按明确源区间拆分循环、映射音量和仅在首尾片段写淡入淡出。
- 原因：远程 URL 不能提供可复现的 FFmpeg preview 或 Jianying 草稿引用；现有适配库提供音频构造 API，但 Jianying UI 播放效果尚未逐项验收。
- 后果：每次交付仍创建唯一的新 draft，并在生成后要求用户在 Jianying 中复核；不得把结构验证描述为已完成播放兼容性。线上音乐 Provider 在取得其 API 与同步授权条款后才可按需下载为项目受控素材。

## ADR-022：版本化 Jianying 适配器交接

- 状态：已接受并实现
- 决策：Rust 验证时间线源后，将版本化 JSON 输入文件交给 Python 适配器，而不是通过标准输入传递含媒体信息的 JSON。
- 后果：输入可检查且避免路径序列化不一致；桌面更新必须递增应用版本以可靠替换适配器资源。

## ADR-023：文档同步 Harness

- 状态：已实现
- 决策：将高影响架构改动与同一 Git 变更集中的长期 Markdown 更新和架构变更记录绑定，并以提交前检查强制执行。
- 原因：产品规则、工具契约、持久化和安全边界不能只依赖 Agent 记忆或人工审阅；同时，纯文件存在检查不足以验证文档语义。
- 后果：`.harness/doc-sync-policy.json` 是机器可读的触发规则来源，`docs/changes/` 是每次架构变更的审计记录。Agent 必须在硬检查通过后使用独立上下文进行文档一致性审查并修复发现；规则只覆盖高影响区域，避免每次普通 UI 改动都产生无意义文档噪声。Provider 安全规则覆盖 OAuth、自定义模型 API、在线音乐 Provider、统一 Provider 决策和 Agent 控制器，避免 Windows Credential Manager 读写模块绕过文档门。

## ADR-024：无窗口媒体子进程与本地任务提示

- 状态：已实现，待桌面手工验证
- 决策：所有 Rust 后端发起的 Windows 外部命令统一通过 `process.rs` 的 `hidden_command` 带无控制台窗口标志执行；前端使用现有素材分析状态在右下角提示活跃任务。
- 后果：FFmpeg、FFprobe、Tesseract、Python 适配器和系统进程查询不会从 GUI 应用弹出命令行窗口；提示仅显示显示名和数量，不暴露原始路径，也不新增持久化或 Tauri 命令。

## ADR-025：持久化 Agent 调用与待审阅恢复

- 状态：已实现
- 决策：每次受限 `execute_agent_edit` 在模型调用前创建作用域化的 `agent_tasks` 记录，并将副作用操作日志关联到调用、剪辑任务和会话。应用启动时不自动重试未完成的通用调用，而是将其标记为 `needs_review`。
- 原因：远端模型请求、preview 渲染和 Jianying draft 创建在进程中断后不一定可证明幂等；自动重放可能重复创建用户可见产物。
- 后果：UI 可查询当前剪辑会话的调用、操作日志与时间线版本历史。通过作用域校验的工具失败会以安全结构化结果回传模型生成后续回复；模型不可用时使用固定降级回复。用户可重新发起请求；分析任务和 Jianying 注册仍保留各自的专用恢复机制。

## ADR-026：视觉分析超时、失败原因与 OAuth 退出

- 状态：已实现
- 决策：`analyze_asset` 的内部视觉分析请求设置 30 秒超时；请求失败时不阻塞技术分析，而是把失败原因聚合为 `visualAnalysisNote` 随素材证据返回。同时新增 `clear_experimental_openai_oauth` 命令用于清除 Windows Credential Manager 中的凭据。
- 原因：无超时的同步 `ureq` 请求会在远端接口不响应时永久阻塞分析线程，导致素材状态卡在 `analyzing` 且 UI 无法获得原因；凭据没有 UI 出口，用户无法断开模型连接。
- 后果：视觉分析超时或失败时素材仍以 `ready` 完成技术分析，证据面板展示失败原因；未连接 OAuth 时证据面板提示已跳过。前端模型弹窗在已连接状态下提供退出登录按钮。

## ADR-027：Rust 后端按职责拆分与 Agent 决策层解耦

- 状态：已实现
- 决策：将原先 4190 行的 `store.rs` 单体模块拆分为 `db`、`models`、`process`、`provider`、`audit`、`projects`、`assets`、`storyboard`、`timeline`、`preview`、`jianying`、`agent` 模块，保持全部 Tauri 命令名与参数契约不变。自然语言编辑控制器集中在 `agent.rs`，并通过 `ToolDecisionProvider` trait 将模型决策层与副作用执行层解耦：决策 provider 只负责把请求、brief、storyboard 状态、时间线候选与媒体证据交给模型并解析工具决策，副作用执行、作用域校验与审计保持在控制器内。
- 原因：单体模块同时承载迁移、领域命令、外部进程与 Agent 决策，难以在不影响既有契约的前提下演进；Provider 决策逻辑也应可替换而不绑定控制器内部实现。
- 后果：命令模块映射变化（例如 `store::initialize_local_store` 迁移到 `projects::initialize_local_store`），但 `src/lib/local-store.ts` 依赖的命令名与入参出参不变，前端无需改动；`execute_agent_edit` 保持单一入口。`request_agent_edit_decision` 保留为测试辅助的自由函数，生产路径经 trait 对象调用。schema version 不升（保持 4），迁移 SQL 按原字节重建。

## ADR-028：Agent 编辑异步派发

- 状态：已实现，待桌面手工验证
- 决策：`execute_agent_edit` 改为异步派发：命令先同步校验、插入 `queued` 调用并立即返回任务 ID，随后在后台线程执行完整流水线（模型决策、作用域校验、工具副作用与审计），终态经 `agent-edit-completed` 事件携 `AgentEditResult` 回传前端。命令返回类型由 `AgentEditResult` 改为 `String`（任务 ID），前端据此应用产出、追加回复并轮询任务状态。
- 原因：原有的同步命令在远端模型请求、preview 渲染等长耗时期间占据单次调用等待，前端只能等待完整结果后才更新；异步派发让一次调用不阻塞桌面 UI，并保留在白名单、作用域校验、副作用审计与 `needs_review` 恢复策略不变的前提下。
- 后果：命令契约变化（返回 `String`），`src/lib/local-store.ts` 的 `executeAgentEdit` 同步更新为返回任务 ID；前端新增 `agent-edit-completed` 事件监听并依据事件结果应用状态。状态经持久化 `agent_tasks.status` 区分 `completed` 与 `failed`。本次仅完成异步化；可恢复本地运行时（队列、暂停/恢复）留待后续迭代。

## ADR-029：严格 per-tool 决策 schema 与时间线编辑工具集

- 状态：已实现，待真实 Provider 响应验证
- 决策：将 `AgentEditDecision` 从扁平字段结构改为内部以 `tool` 加标签关闭枚举，每个工具携带独立、`deny_unknown_fields` 的 `params` 对象；同时把单一 `replace_timeline_clip` 拆为 `replace_clips`（批量替换，保持每个镜头时间线时长）、`change_clip_duration`（在已验证源范围内重定时长与起止点）、`reorder_clips`（`order` 为全部既有 `shot_index` 的完整排列），并新增不产生产物的 `request_clarification`。
- 原因：扁平结构让模型可输出未知工具名或错字段而无法在解析期被识别，曾经导致“Agent 本次没有形成可执行的剪辑决定”这类失败掩盖真实决策错误并固定降级；工具缺失也限制了自然的“改这个镜头时长/换这几个镜头/调整顺序”表达。
- 后果：`timeline.rs` 新增 `replace_clips`/`change_clip_duration`/`reorder_clips` 作用域函数与各自单元测试；`agent.rs` 决策解析改为匹配枚举变体，每条变更都在 Rust 侧校验作用域与验证范围、生成新时间线版本并写操作审计；`request_clarification`/`no_action` 不写副作用。装好的 exe 仍是旧 app（4 工具 prompt），需重新构建安装后新工具才生效。

## ADR-030：可配置的自定义 OpenAI 兼容模型 API

- 状态：已实现，端到端真实托管响应待人工验证
- 决策：为模型 Provider 增加第二个可控入口，用户在模型弹窗填写 Base URL、Model 名与 API Key，`save_custom_api` 一并保存到 Windows Credential Manager，`clear_custom_api` 可清除；`ModelAccess::resolve()` 在自定义 API 已配置时优先使用，否则回退到实验性 OpenAI OAuth。模型请求统一经 `post_model_payload`/`model_response_json_text` 分派：OAuth 走实验性 Responses 端点，自定义 API 走 `{baseUrl}/chat/completions` 并把 Responses 风格的 `input`/`text.format` 载荷转换为 `messages`/`response_format`。
- 原因：ADR-005 要求 Provider 可替换且应用逻辑不绑定厂商格式；仅靠实验性 OAuth 让用户无法接入自托管或第三方 OpenAI 兼容模型。
- 后果：API Key、Base URL 与 Model 只存于 Windows Credential Manager，绝不进入 SQLite、浏览器存储、日志或工具结果；传入输入仅含最小必要提示与派生证据。`agent.rs`、`storyboard.rs`、`assets.rs` 均改经 `ModelAccess` 决策，不再直接引用 `AuthorizedOAuth`。视觉分析与 agent 决策在自定义 API 下的真实模型响应仍需桌面手工验证。

## ADR-031：平铺字段宽容 schema与有界技能循环

- 状态：已实现，被 ADR-032 取代
- 决策：将 `AgentEditDecision` 从 ADR-029 的“独立 `params` 嵌套对象”改为顶层平铺的宽容 schema：每个工具的参数直接放在最顶层（无嵌套 `params` 包装），多余键被 `#[serde(flatten)]` 吸收，`#[serde(other)]` 的 `Unknown` 变体保证任何未识别工具都不会解析失败。`agent.rs` 的快速路径只处理已确定的工具；遇到未识别工具（`Unknown`）时升级到新模块 `agentloop.rs` 的有界技能循环：默认循环按步提交当前 storyboard/时间线候选与逐步 transcript 给模型，模型选择单一技能，技能执行真实领域函数并回读结果，直到 `finish`/`ask_user`/步数上限（6）。
- 原因：真实响应曾把 `brief` 放到顶层而 ADR-029 的嵌套 `params` 无法解析，导致 `missing field 'params'` 解析失败而“死机”；平铺宽容 schema 让模型“换个说法就能被识别”。同时让固定路径无法处理的请求交给观察驱动的技能循环，而不是只在预设枚举里打转。
- 后果：删除各工具 `_*Params` 结构体，仅保留 `AgentEditCommon` 与 `ClipReplacementParams`/`ClipAdjustmentParams`；`agent.rs` 在决策外层增加 `escalated` 判定，`Unknown` 时以 `agent_loop` 为工具名调用 `run_agent_loop`；`agentloop.rs` 暴露观察技能 `list_assets`/`get_storyboard`/`get_timeline` 与编辑/交付技能 `generate_storyboard`/`create_timeline_draft`/`replace_clips`/`change_clip_duration`/`reorder_clips`/`render_preview`/`create_jianying_draft`，全部复用既有作用域与范围校验、写操作审计，失败只回读错误，绝不自动无限重试。该设计随后被 ADR-032 的“彻底统一”取代：不再有开放枚举决策、不再区分“已识别/升级”，所有非显式请求直接进入目标驱动循环。

## ADR-032：彻底统一为单一目标驱动技能循环

- 状态：已实现
- 决策：删除 `AgentEditDecision`/`AgentEditCommon` 开放式 schema 与 `ToolDecisionProvider`/`ModelToolDecisionProvider` 决策解耦层，`agent.rs` 不再询问模型选哪个工具，也不再有“已识别快速路径 + Unknown 升级”的分叉。现在所有非显式单命令的自然语言请求一律直接进入 `agentloop.rs::run_agent_loop`：循环先调用 `derive_loop_goal` 按关键词派生产物目标（`LoopGoal`：问答/storyboard/内部时间线/preview/剪映草稿），模型每步在观察技能（`list_assets`/`get_storyboard`/`get_timeline`）或编辑/交付技能（`generate_storyboard`/`create_timeline_draft`/`replace_clips`/`change_clip_duration`/`reorder_clips`/`render_preview`/`create_jianying_draft`）中选择单一技能执行并回读结果；步骤解析为宽容的 `AgentStep`，技能参数放在 JSON 顶层（`step_args` 剔除 `tool`/`reason`/`answer`/`question`/`taskBrief` 元字段，多余键被容忍）。`finish`/`no_action`/`done` 只有在 `goal.satisfied_by(&last_outcome)` 判定目标产物真实存在时才结束，否则模型收到 `corrective_message` 纠偏并继续，直到步数上限（6）。`ask_user` 只返回可执行的中文澄清问题。显式单命令（“创建剪映草稿”“创建内部时间线”“生成预览”等）由 `agent.rs::explicit_command_tool` 精确匹配后走 `run_explicit_command` 确定性直通路径，不依赖模型决策。所有技能复用既有作用域与范围校验、每次变更生成新版本并写操作审计；失败技能只回读错误，绝不自动无限重试。终端回复由真实执行的产物组装，产物缺失时用 `honest_no_change` 固定诚实文案，绝不采用模型捏造的“已完成”。
- 原因：升维抽象（封闭枚举 + Unknown 升级）让“换一种说法就容易掉进升级循环”；把任意自然语言请求统一进同一份有界技能循环，让产物目标和完成门成为唯一判定标准，更贴合“构建 Agent”的产品定位，也避免在固定枚举里打转。
- 后果：删除 `AgentEditDecision`/`AgentEditCommon`/`ToolDecisionProvider`/`ModelToolDecisionProvider`/`request_agent_edit_decision`/`retarget_decision_for_draft`/`verified_action_message`/`safe_follow_up_reply`；`AgentEditResult` 保持不变，仍是循环与显式命令的通用产物载体；`agent.rs` 只保留显式命令匹配与流水线落地/审计，`agentloop.rs` 承担全部技能执行与目标判定；新增 6 项 `agentloop.rs` 单测（`derive_goal_pins_the_deliverable`/`goal_satisfied_only_with_a_real_artifact`/`terminal_without_artifact_is_honest_for_deliverable_goals`/`step_args_removes_meta_keys`/`step_args_survives_non_object_decisions`/`finalize_result_keeps_the_last_concrete_outcome`），`cargo build --lib` 与 `cargo test --lib`（19 通过）均验证通过。

## ADR-033：多轮对话记忆与模型意图分类

- 状态：已实现，真实模型响应待桌面手工验证
- 决策：让 Agent 能像自然语言对话一样连续交流。`run_agent_loop` 先用 `load_message_history` 从 `messages` 表按 `conversation_id` 读取最近消息（最多 12 条、总字符预算 8000，排除当前请求本身，按时间正序）作为多轮记忆拼进循环提示与分类提示。目标派生从 ADR-032 的“纯关键词规则”改为两级：先用确定性快路径 `fast_goal`（强编辑/创建动词 `EDIT_VERBS`/`CREATE_VERBS` 与清晰疑问句式 `QUESTION_PHRASES`——明确产物命令、明确编辑、清晰提问各自直接判定；疑问且无动作词归问答，疑问且带动词留给模型），只有快速路径无法确定的请求才用一次轻量模型调用分类（`classify_goal_with_model`：携带对话历史输出 `goal` + `isQuestion`，`isQuestion` 为真时一律归为问答目标，避免把“告诉我选择每个镜头的逻辑”这类提问逼进产物门；分类失败默认问答，无产物不落地）。`EDIT_VERBS` 不再包含固有的“镜头”等名词，避免把真实问题误判为时间线编辑。
- 原因：纯关键词分类把“请告诉我选择每个镜头的逻辑”这类含“镜头”的提问误判为时间线编辑目标，产物门让循环最终返回固定诚实文案而非自然回答；模型是常驻基础能力，可作为主要分类手段，同时保留零成本的确定性快路径处理明确命令/编辑/提问。
- 后果：`derive_loop_goal` 签名从 `(request)` 改为 `(access, request, history)`；`LoopState` 增加 `history` 字段，`build_step_prompt`/`classify_goal_with_model` 均携带多轮记忆；新增 `fast_goal`/`classify_goal_with_model`/`parse_classified_goal`/`load_message_history`/`render_history`。多轮记忆加载与快路径由确定性单测覆盖，模型分类分支依赖真实 Provider（与既有集成测试一样跳过）。明确不带“模型不可用”的多层兜底：模型不可用仅回退到问答目标，并保留现有一条友好的降级文案。

## ADR-034：异步完成对账与 Provider 故障封闭

- 状态：已实现，待桌面竞态手工验证
- 决策：`agent-edit-completed` 允许先于 `execute_agent_edit` 的任务 ID 返回到达，前端以任务 ID 为键缓存最多 20 个早到事件并在命令返回后对账；事件只在项目和剪辑会话仍匹配时更新当前可见产物。`ModelAccess` 只有在自定义凭据明确不存在时才回退 OAuth，凭据读取错误直接阻止请求。模型响应解析日志只记录固定阶段与长度。模型失败或循环耗尽且目标未满足时，任务终态为 `failed`，终态读取错误也失败封闭。视频重定时同时校验已验证源窗口上下界，源结束点始终等于新起点加新时长。
- 原因：后台线程可以在 Tauri invoke 返回前完成；单一 pending ref 会丢弃早到事件或把旧会话产物写进新会话。吞掉凭据读取错误会把内容发送到用户未预期的 Provider，记录模型原文则违反本地内容日志边界。成功状态和源时间范围也必须与真实执行结果一致。
- 后果：快速显式命令不再令 composer 永久卡在处理中；跨会话完成仍持久化到原会话但不污染活动视图。Provider 故障采用封闭失败，日志不含模型原文；未满足目标的调用以安全代码 `agent_goal_not_reached` 审计。`change_clip_duration` 的持久化源窗口与 preview/Jianying 实际使用范围一致。

## ADR-035：Payload-free 步骤审计、权威状态快照与确定性前置条件

- 状态：第一阶段已实现
- 决策：schema v5 新增 `agent_run_steps`，对每个循环技能和显式直通技能持久化步骤号、工具名、状态、安全产物类型/ID、安全错误码与时间戳；查询必须同时匹配项目、剪辑任务和 Agent 调用。每轮模型决策前重建紧凑 `AgentStateSnapshot`，确定性代码提供真实依赖提示，模型继续负责创作选择。终态区分 `completed`、`partially_completed`、`failed`、`needs_clarification` 与启动恢复的 `needs_review`。
- 原因：只记录整个调用无法说明执行到哪一步；重复传完整历史容易让模型选择旧版本；把固定依赖交给模型会浪费有界步骤。步骤审计又不能成为模型原文、用户对话、本机路径或媒体证据的旁路存储。
- 后果：前端和审计工具可经 `list_agent_run_steps` 查询执行轨迹；中断步骤封闭为 `failed/interrupted_requires_review`，运行保持 `needs_review` 且不自动重放。模型需要镜头细节时调用观察技能，快照本身只含紧凑事实。工具契约与回归用例保存为版本化 fixture，并由测试核对白名单一致性。完整持久化队列、暂停/恢复、版本撤销和危险操作 approval token 仍为后续阶段。

## ADR-036：信息点覆盖优先的 storyboard 选镜

- 状态：已实现，待真实桌面素材验证
- 决策：storyboard 请求必须先返回文案信息点（beats），再为每个被覆盖的信息点选择真实素材。每个镜头保存 `beatId` 和 `matchLevel`；只允许 `direct` 或 `contextual`。缺少可用画面证据的信息点仅记录在 `uncoveredBeatIds`，不能生成 `insufficient` 镜头或进入内部时间线。
- 原因：一次性把完整文案和全部素材交给模型会造成相近 B-roll 的机械拼接，并把无法由画面证明的技术结论伪装为已表达。剪辑 Agent 必须诚实地区分直接表达、场景承载与素材缺口。
- 后果：后端增加信息点覆盖、匹配等级、重复缺口和最低英文阅读时长校验；`StoryboardVersion` 返回加性字段，旧版本以安全默认值读取。该变更不新增配音、字幕、音乐、图形或最终导出能力；`uncoveredBeatIds` 由 UI 提示用户补素材或接受语境剪辑。

## ADR-037：模型主导创作，受控执行副作用

- 状态：已实现，待真实桌面素材验证
- 决策：顶层 Agent 使用 10 步模型编排预算，storyboard 使用独立的 3 次内存修订预算。模型提出目标时长与表达模式，不以固定镜头数或固定成片时长规定创作；只保留本地处理的安全上限。模型可使用 `request_asset_analysis` 请求项目内已导入的未分析素材进入本地分析队列。
- 后果：模型只负责意图、选材和自然语言沟通；Rust 保留文件访问、SQLite 写入、FFprobe/FFmpeg/Tesseract 调用、作用域校验和审计权。模型可以解释工具回读，但真实产物完成事实始终由后端工具摘要呈现，避免模型捏造未执行编辑。模型不可用时才使用固定安全降级文案；其余失败和部分完成由模型依据工具回读决定重试、修订或解释。
# ADR-049：源文件健康检查显式、异步且只读元数据

- 状态：已采用（2026-08-13）
- 决策：素材列表禁止同步访问源路径；用户通过可取消后台任务检查文件大小和修改时间，结果独立持久化。
- 原因：大型项目和离线或网络路径可能令列表阻塞；快照让浏览延迟与文件系统健康解耦。
- 边界：扫描不打开或上传媒体内容，不自动覆盖既有基线；仅新导入和确认重链路建立基线。

# ADR-050：项目素材收集生成不可覆盖的独立副本

- 状态：已采用（2026-08-13）
- 决策：收集前必须展示文件数、不可用数和估算体积并获得确认；执行时在所选目录创建 UUID 新包，永不覆盖现有文件，也不改写项目源引用。
- 隐私：manifest 不包含源路径，审计日志只保存复制与跳过计数。
- 降级：执行时再次验证每个源文件；不可用或复制失败的条目跳过并诚实计数。
# ADR-051：素材管理是 Agent 后台能力，不是主工作区

- 状态：已采用（2026-08-13）
- 决策：主界面展示素材导入、整体分析状态、源文件健康摘要、异常恢复，以及只读的层级文件夹目录和派生证据信息；每级只显示直属子文件夹和直属素材，单文件导入统一进入“未归类素材”。目录键只含根名称和相对目录，不暴露绝对路径。搜索、状态筛选、收藏、标签、集合、批量操作、任务明细和片段检索不作为人工管理控件展示。
- 原因：产品是自动剪辑 Agent，用户应表达成片目标，由 Agent 使用受限工具管理素材与选择片段；传统素材管理器会增加认知负担并模糊产品定位。
- 后端：既有数据、检索、分页、批量与审计契约继续保留，供 Agent 和诊断使用，不因 UI 收敛而削弱安全边界。

# ADR-052：项目事实问答必须进入观察循环

- 状态：已采用（2026-08-13）
- 决策：保持 Conversation Router 的 `respond/clarify/run` 顶层结构不变，仅为 question 增加 `informationScope=general|project`。通用知识可 `respond`；依赖当前项目素材、任务、产物、数量、状态或原因的问答必须 `run`，具体首个观察工具仍由模型选择。
- 原因：把项目问答继续视为普通即时回复会允许模型绕过真实状态并生成通用猜测；新增关键词分支或第二分类器又会侵占模型的工具选择职责。
- 后果：后端只校验结构化组合；`project + respond` 在原 90 秒预算内最多纠正一次，项目问答只允许观察工具且至少一次观察成功后才能 finish，不按关键词决定具体工具。`get_asset_health_summary` 同时返回已解释/未解释失败数量；schema v14 保存脱敏文件读取原因码，路径和原始系统错误不进入模型上下文。
