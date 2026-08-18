# 架构

## 开发协作边界

`CONTRIBUTING.md` 是分支、worktree、验证、提交与 PR 的唯一流程；`AGENTS.md`、`CLAUDE.md`、Cursor rule 和 `opencode.json` 只是薄入口。`.harness/branch-policy.json` 与 pre-commit 禁止直接在 `master`/`main` 提交、拒绝未知分支前缀，并要求当前任务分支包含本地 `origin/master`。检查不执行网络操作，远端基线由开发者先 `git fetch origin` 更新；GitHub 分支保护仍是独立的远端 TODO。

## 状态

仓库已实现 React/Tauri 桌面基础、SQLite 本地持久化、媒体分析、证据绑定 storyboard、内部时间线、preview 和实验性 Jianying draft 创建。本文件同时描述当前实现与仍待完成的生产能力；标记为 `TODO` 的项目尚未实现或尚未验证。

2026-08-15 的素材前端重建保留后端目录投影、桌面命令、持久化 schema、Agent 工具、媒体处理和交付行为，只删除违背 Agent-first 边界的人工素材管理界面。

| 能力 | 状态 | 备注 |
|---|---|---|
| React/Tauri 桌面壳、SQLite、迁移 | ✅ 已实现 | |
| 媒体分析（FFprobe/FFmpeg/Tesseract/视觉批次）| ✅ 已实现 | |
| 素材目录树、分页、健康扫描、重链路 | ✅ 已实现 | |
| 证据绑定 storyboard | ✅ 已实现 | |
| 版本化内部时间线（视频/文本/音乐轨）| ✅ 已实现 | |
| FFmpeg preview（含文本轨 ASS / 音乐混音）| ✅ 已实现 | |
| Jianying 仅视频 + 受限文本草稿 | ✅ 已实现（实验性）| |
| Jamendo 在线音乐下载 | ✅ 已实现（实验性）| Jianying UI 试听待验收 |
| Task Resolver + Conversation Router + Agent loop | ✅ 已实现 | |
| 实验性 OAuth / 自定义 OpenAI 兼容 API | ✅ 已实现 | 官方 OAuth 机制待核实 |
| 多步 Agent fixture 可执行运行器 | ⚠️ 部分实现 | scripted runner 待补 |
| 视觉质量评分 / 语义重复检测 | ⚠️ 部分实现 | 仅单帧低分辨率候选 |
| 生产安装包运行时供应 | ❌ TODO | FFmpeg/Tesseract/Python 未随包分发 |
| Jianying 图片/完整字幕/logo 轨 | ❌ TODO | |
| Voice API | ❌ TODO | 契约待获取 |
| Windows CI / 远端分支保护 | ❌ TODO | |

## 当前组成

```text
React 19 + TypeScript + Vite
|
|- src/App.tsx            项目/会话/消息入口与顶层工作区组合
|- src/hooks/             Provider、素材、成果交付与 Agent 终态对账 controller
|- src/components/        互斥工作区和单一职责展示组件
|- src/lib/local-store.ts Tauri 命令 TypeScript 桥接
|- src/lib/agent-tools.ts Agent 内部技能名的 IDE 镜像
`- src-tauri/             Rust 命令、SQLite、媒体工具与 OAuth 边界
  |- src/agent.rs         自然语言编辑控制器
  |- src/taskrouter.rs    项目内任务归属解析与任务状态快照
  |- src/{projects,assets,storyboard,timeline,preview,jianying,audit}.rs  领域命令
  |- src/db.rs + models.rs SQLite 迁移与领域类型
  |- src/provider.rs      实验性模型请求封装
  |- src/process.rs       无窗口外部命令
  `- src/oauth.rs         实验性 OpenCode 兼容 OAuth/PKCE
```

面向源码学习的分层调用图、模块职责、集成、测试和风险清单位于 `docs/codebase/`。该目录只描述可从当前源码和终端验证的现实；本文继续承担长期产品架构和安全边界。全部手写源码模块顶部提供中文职责导航，Rust crate 的 `lib.rs` 提供模块索引；权限、事务、恢复和非直观算法只在关键位置补解释，不逐行复述语法。

`App.tsx` 在 Tauri 环境中通过 `local-store.ts` 加载项目、剪辑任务、会话和消息。剪辑任务是项目内的创作目标；会话、storyboard、timeline 和 preview 均被限制在该任务内，素材保持项目级复用。自然语言消息先经项目内 Task Resolver 选择已有任务、创建新任务或澄清，目标任务确定后才写入其 conversation；首次消息或导入仍会在需要时创建项目、任务和会话。

### 作用域架构

产物与会话的归属关系：

```
Project (项目)
├── Assets (素材，项目级复用)
└── Editing Tasks (剪辑任务，创作目标单元)
    ├── Conversations (会话，对话容器；一个任务可有多个会话)
    │   └── Messages
    ├── Storyboard Versions (故事板，直接归任务)
    ├── Timeline Versions (时间线，通过 storyboard 归任务)
    └── Previews / Jianying Drafts (基于 timeline，归任务)
```

**会话（conversation）只是对话容器**，不拥有产物。用户可在同一剪辑任务下开启多个会话（例如第一轮讨论后重新开始），所有会话共享该任务的 storyboard、timeline 和 preview 版本。产物查询和创建只需 `(project_id, editing_task_id)`，不依赖 `conversation_id`。Task Resolver 负责把新消息路由到正确的任务或创建新任务；Conversation Router 决定是直接回复还是启动 Agent run。

前端按”入口组合、领域 controller、展示组件”分层。`useProviderController` 独占模型连接状态和凭据入口；`useAssetWorkspaceController` 独占素材分页、轮询、导入、健康、重链路和证据状态；`useArtifactWorkspaceController` 独占 storyboard、timeline、preview、Jianying 状态及交付动作；`useAgentRunReconciliation` 独占任务 ID、早到事件、终态轮询和持久化恢复对账。`App.tsx` 只协调项目/会话/消息作用域并组合这些 controller，不直接重新实现其副作用。Agent、素材、成果三种顶层模式互斥渲染为 `AgentWorkspace`、`AssetManagementPanel` 和 `ArtifactsWorkspace`；原先同时承载两个模式、拥有大量扁平 props 的 `ConversationWorkspace` 已删除。领域工作区只接受 `model/actions` 等一至两个顶层入口。素材目录的真实开合状态仍由 `AssetDirectoryTree` 局部拥有，不进入 controller 或 `App.tsx`。

Tauri 2 后端提供 SQLite、本地文件/文件夹导入、媒体分析、storyboard、内部时间线、FFmpeg preview 和实验性 Jianying Pro 8.0 仅视频草稿创建。`tauri.conf.json` 使用受限 CSP，仅允许作用域内的本地派生媒体协议。

## Agent 编程上下文架构

仓库把“给编码 Agent 的说明”分成三层，避免一份巨大 Markdown 同时承担所有领域细节：

```text
AGENTS.md                         全局产品、安全规则与读取路由
├─ src/AGENTS.md                 React 状态、IPC 与恢复边界
└─ src-tauri/src/AGENTS.md       Rust 可信执行、事务与外部集成边界

TASKS.md / ACTIVE_TASKS          当前任务、完成门与下一个已授权动作
docs/codebase/                   七份可验证的当前源码地图
docs/{architecture,api,...}.md   按需读取的长期事实和历史决策
.harness/agent-context.json      机器可读清单、作用域与允许列表
```

支持分层 `AGENTS.md` 的编码 Agent 会按目标路径获得就近约束；其他 Agent 必须手动读取根入口、当前任务窗口和目标目录指令。`scripts/check-agent-contracts.mjs` 在工作区和暂存区验证：上下文文件及七份代码地图完整、当前任务窗口有界、全部受控手写源码顶部存在中文职责导航；React 只能经 `local-store.ts` 调用 Tauri，公开命令在 bridge/`lib.rs`/`docs/api.md` 间可对账；外部进程、Credential Manager、HTTP/网络传输与 Agent 工具目录不能跨越既定所有者。它不尝试用正则证明注释或业务语义正确，语义仍由类型、Rust 校验、测试、真实桌面验收和独立审查负责。

这套结构把接手成本收敛为“先定位，再按风险加载”。清单与 Git `HEAD` 比较，只允许缩小任务窗口和可信允许范围；pre-commit 直接运行并要求 staged 检查器/配置与 working tree 一致。它仍不承诺仅阅读文档即可无状态地续写任何未提交工作；可靠交接还要求工作区干净、当前任务窗口准确、变更记录和测试结果可复现。

## 系统边界

```text
Windows 桌面应用（Tauri + React）
|
|- 展示层（已实现）
|  |- 互斥的 Agent、素材、成果顶层模式
|  |- Agent：会话、路由提示、可折叠执行任务卡与 composer
|  |- 素材：完整宽度的可开合目录、直属素材、健康恢复与证据 Inspector
|  |- 成果：唯一 Workflow、storyboard、timeline/审计与 preview
|  `- 项目与 Provider 状态
|
|- 本地 Agent 控制器（已实现基础）
 |  |- 受限工具选择与后端校验
 |  |- Task Resolver、会话/任务上下文
 |  `- 持久化调用状态、作用域查询与中断后待审阅恢复
|
|- 本地工具服务（部分实现）
|  |- 导入、FFprobe/FFmpeg/Tesseract 分析、时间线、preview
|  |- Jianying 仅视频适配器
|  `- 音频、字幕、生产运行时供应、voice Provider TODO
|
|- 模型 Provider（部分实现）
|  |- 实验性 OpenCode 兼容 OAuth/PKCE
|  |- 自定义 OpenAI 兼容 API（Base URL + API Key + Model，chat/completions）
|  `- 官方 OAuth 验证、本地模型 TODO
|
`- 本地存储（已实现基础）
   |- SQLite：项目、任务状态快照、任务路由澄清、会话、素材、版本、Agent 调用、操作日志
   `- Windows Credential Manager：实验性 OAuth 凭据与自定义 API 凭据
```

## 数据流

```text
导入本地文件或文件夹
  -> SQLite 保存源文件引用
  -> 后台 FFprobe 提取时长、尺寸、帧率和音频轨信息
  -> FFmpeg 生成缩略图、关键帧和启发式场景片段
  -> Tesseract 提取图片/关键帧英文 OCR
  -> 技术分析完成后，后台将最多六条素材的代表帧批量发送给实验性 Provider
  -> 保存按素材 ID 与源时间校验的视觉建议；每批请求 30 秒超时，失败原因随素材证据返回
  -> Provider 仅基于持久化证据生成 storyboard，后端验证素材与时间范围
  -> storyboard 先把文案拆为信息点（beats），再为每个已覆盖信息点选择源时间绑定的镜头
  -> 缺少真实画面证据的信息点只作为未覆盖项保存，绝不作为 `insufficient` 镜头写入时间线
  -> 创建源时间绑定的内部时间线版本
  -> FFmpeg 渲染 540 x 960 本地 preview 并执行质量检查
  -> 可选地创建新的 Jianying Pro 8.0 仅视频草稿
```

视觉建议是 AI 建议，不是经验证的媒体事实。语义相似度、质量评分和多帧重复检测仍为 `TODO`。

storyboard 的每个镜头额外保存 `beatId` 和 `matchLevel`。`direct` 只用于模型明确认为已有证据直接支撑的信息点；`contextual` 只用于诚实的场景承载并在选片理由中说明限制。模型同时提出 `targetDurationMs` 与 `scriptMode`（完整文案或关键表达）；镜头数、信息点数和时长不再是固定创作规格，只保留 30 镜头/信息点、120 秒的本地处理安全上限。生成校验拒绝 `insufficient`、未知/重复信息点、未覆盖且未声明缺失的信息点、跨镜头重叠复用的同一视频源范围，以及完整英文文案被压缩到低于最低阅读时长的 storyboard。`uncoveredBeatIds` 是创作缺口，不进入内部时间线；界面会提示该缺口，用户可据此补充素材或接受现有上下文剪辑。

## 数据所有权与安全

- 源文件默认仅被引用。素材列表不对每条源路径做同步探测，避免失联盘符或网络路径拖住 UI；分析、storyboard、preview 和 Jianying draft 在实际使用前检测可用性。缺失素材会保留记录，但不能进入新的 storyboard 或 preview。
- 用户可主动选择新的素材根目录触发两阶段重新定位：预览阶段只扫描候选目录并以唯一的旧相对路径和媒体类型匹配，不修改项目；人工素材工作区的确认阶段固定保留已有分析证据并只更新路径，避免恢复位置时意外触发重新分析。后端仍保留显式 `preserveAnalysis` 契约供受限 Agent/诊断流程使用。无法唯一验证的素材保持原引用，绝不按文件名猜测或自动错连。
- 应用绝不修改源媒体。
- 内部时间线是事实来源；Jianying draft 是单向交付物，不回读用户在 Jianying 中的编辑。
- OAuth 凭据只保存在 Windows Credential Manager，绝不进入 SQLite、浏览器存储、项目文件或日志。
- 模型仅接收获批的精简提示、证据文本和低分辨率派生帧，绝不接收原始媒体或本机路径。

## 当前实现细节

### 素材库

schema v12 将收藏、评分、备注、禁止使用、用户标签和素材集合保存在独立关系表中，不进入会被重新分析替换的 `assets.metadata_json`。这些能力保留为 Agent/诊断契约，不在人工素材工作区提供搜索、筛选、批量整理或集合管理入口。“禁止使用”仍从后续 storyboard 候选中硬排除，但不改写历史产物；既有数据、同项目校验和数量型审计均保持不变。

Agent 通过受限只读 `search_assets` 做目标化候选发现，而不是把整个素材库注入模型。查询可组合媒体类型、时长、最低评分、收藏、标签、集合和游标，单页最多 20 条；结果按收藏、评分和更新时间稳定排序并给出固定命中原因码。工具自动排除用户禁止使用的素材，不返回源路径、用户备注正文、OCR 正文、媒体内容或完整视觉证据。`list_assets` 继续只用于紧凑状态盘点和分析排队前观察。

素材浏览不访问源文件系统。schema v14 的 `asset_source_health` 独立保存大小、修改时间基线、最近观察结果和脱敏原因码；只有用户显式启动可取消的 `scan_asset_health` 后台任务时才逐项读取文件元数据。新导入和确认重链路会建立新基线；列表只展示持久化的 `unchecked/online/missing/changed/unreadable` 状态。Agent 通过 `get_asset_health_summary` 读取项目级计数、扫描状态和安全原因码，不接收路径或原始系统错误。

Agent 的片段发现通过 `search_asset_segments` 在已持久化的场景段内绑定 OCR/视觉证据，返回可直接用于剪辑工具的明确源时间范围。结果单页最多 20 条，保留 OCR 正文和本地路径的隐私边界，并排除用户禁止使用及健康状态明确异常的源文件。

“收集项目素材”是用户显式确认的本地复制操作。预览阶段重新验证源文件并估算体积；执行阶段只在用户选择的目录下创建全新 UUID 包，文件名带素材 ID 短后缀避免碰撞，manifest 仅记录项目/素材 ID、显示名、包内相对路径和字节数，不记录原始路径。操作不会覆盖已有包、删除文件或改变项目当前引用。

素材搜索、状态筛选、收藏、标签、集合、批量整理、任务明细和片段检索继续是受限的本地 Agent/诊断能力，不作为人工控件渲染。独立“素材”顶层模式只承担导入、目录浏览、整体分析摘要、源文件健康/异常恢复和派生证据检查。层级目录使用后端权威的安全目录投影：每个素材有直属 `directoryKey`，`list_asset_page` 返回完整项目的目录节点、父节点和直属素材计数；前端只组装父子关系，不再从当前最多 100 条结果猜整棵树，也不在 SQLite 已按目录筛选后再次过滤。安全导入根首次出现时自动展开，其他子目录默认折叠；行按钮的 `aria-expanded` 与条件渲染的子树一致，1.5 秒素材轮询不重置用户开合状态。选择目录会立即清空旧页，完成查询后只显示直属素材；“全部素材”显示当前有界页。旧记录的根引用若缺失或误存为不可展示根，后端先剥离并按普通/扩展盘符或 UNC server/share 隔离；单个安全卷组有共同父目录时以最末共同级为根，没有可公开共同父目录时以固定“导入素材”为根。多个可恢复卷组全部放入各自“导入素材 N”命名空间，防止同名相对树合并。不能安全解析的素材只让自身留在未归类，空目录和只含不支持文件的目录无法从素材记录恢复。任何响应都不包含盘符、server/share 或绝对路径。

最多 200 条的批量操作、技术分析重试和视觉分析跳过继续保留后端作用域校验和操作审计，但不再由素材工作区直接暴露；显式视觉跳过仍优先于在途粗视觉批次结果，避免状态竞态。顶层模式与前端删减只影响展示组合，不扩大工具副作用或数据访问范围。

运行时覆盖说明：显式 preview 或 Jianying draft 请求通常走受控直通工具；若请求含同一项目、剪辑任务内已验证的时间线但缺少 storyboard 上下文，后端不擅自选定渲染动作，而是把该受控事实交给模型技能循环决定直接渲染、先观察时间线或澄清。无论模型选择什么，文件、SQLite 与 FFmpeg 仍只能由通过作用域与范围校验的 Rust 工具执行。

Rust 后端按职责拆分为独立模块：`db.rs` 负责 SQLite 与迁移，`models.rs` 定义领域类型，各领域模块承载受控命令。当前 schema version 为 14；v14 为源健康快照增加脱敏原因码。目录树恢复是读取时的加性安全投影，不修改 schema、素材引用或分析证据。v10/v11 的任务快照、待归属请求与一次性路由凭证继续保持既有职责。通用 Agent 调用步骤与诊断不包含模型原文、会话内容或媒体证据。迁移只增不删。

`agentloop` 已完成分层拆分（2026-08-17）：`agentloop/policy.rs` 只拥有工具白名单、请求负向约束、目标解析和产物完成门；`agentloop/schema.rs` 为纯类型与常量；`agentloop/prompt.rs` 负责提示构建与历史加载；`agentloop/skills.rs` 为技能执行器；`agentloop/runtime.rs` 为路由决策与主循环；父 `agentloop.rs` 收缩为薄 re-export 层加测试。`assets.rs` 的素材库查询职责已提取为 `assets/library.rs`，技术/视觉分析提取为 `assets/analysis.rs` 与 `assets/visual.rs`，源健康提取为 `assets/health.rs`；`assets.rs` 收缩为薄协调层（2026-08-17）。热点均受只降不升预算保护，下一步按 `docs/codebase/CONCERNS.md` §6 的顺序渐进拆分。全过程保持 Tauri 命令名、SQLite schema、版本/审计语义和 Agent fixture 不变。

仓库包含两类开发期硬检查。`.harness/doc-sync-policy.json` 将高影响的桌面命令、持久化、Provider/凭据安全和运行时配置路径映射到必须同步的长期 Markdown 文档；`.harness/architecture-budgets.json` 对前端入口、组件、controller、命令桥接和当前 Rust 热点设置只能下降的复杂度预算，并禁止已删除的旧边界与跨层调用返回。结构预算同时检查行数、字符总量和最长单行，避免通过代码压缩绕过；async 箭头与 async function 都计数，受 props 预算保护的组件签名无法解析或使用 rest props 时直接失败。检查会与 Git `HEAD` 比较，拒绝提高已有数值、移除指标或撤掉禁止项；受保护文件迁移必须留下永久 `budgetReplacements` 映射、新目标预算和旧路径禁用记录，新目标还必须以不放宽的值继承全部数值指标与原路径跨层禁止规则，目录预算即使为空也不能删除。`harness:check` 同时运行架构预算和文档同步检查，`.githooks/pre-commit` 对暂存内容执行相同门；`docs/changes/` 保存可审计的架构变更记录。预算不是质量证明：超过预算时必须拆分职责；真正替换边界时删除旧文件、建立新预算并记录 ADR。对于触发文档规则的工作，独立上下文 Agent 会审查代码 diff、变更记录和文档语义，并在最多三轮修复后给出结果。详见 `docs/harness.md`。

### 媒体分析队列

导入后，每个素材会创建 `analyze_asset` 持久化任务。启动时会恢复未完成分析、取消同一素材的重复任务，并额外补齐“只有 `queued`/`analyzing` 素材但没有对应分析任务”的孤立行（例如导入被中断时），让这类素材也能真正完成分析而不是永远显示在“正在分析媒体”提示里。分析队列以有界批次推进避免打满 CPU：技术分析最多 2 个 worker（`MAX_TECHNICAL_ANALYSIS_WORKERS`），启动恢复只先处理前 4 条（`STARTUP_ANALYSIS_BATCH`），其余保持 `queued`，待用户查看某项目时由 `list_assets` 轮询每次再排空至多 4 条（`DRAIN_ANALYSIS_BATCH`）。FFprobe、缩略图、场景扫描、回退抽帧与 Tesseract 分别有 20、30、45、20、20 秒硬超时；任一阶段超时都会使技术分析失败而队列继续，OCR 正常完成但未识别文字仍不失败。Windows 超时会以无窗口的 `taskkill /T /F` 请求终止子进程树，并在短时退出窗口内回收直接子进程；若终止请求或确认失败，调用不会无限等待，因此不能保证该进程树已退出。启动把中断的本地 `running` 任务重排为 `queued`。`list_assets` 只返回持久化的分析状态，不再对所有源路径同步 `stat`；实际分析和交付工具才校验文件，避免大批量失联素材令 1.5 秒轮询阻塞。单素材首次分析只扫描视频前 30 秒（`SCENE_SCAN_CAP_SECONDS`）、最多生成 4 张关键帧，视频 OCR 只处理前 2 张；视觉分析独立在后台批次完成。SQLite 连接启用 WAL 与 5 秒 busy_timeout（`db.rs::open_connection`），消除并发写导致的 `database is locked`。前端轮询活动项目素材状态，并在右下角显示最多三个正在分析的显示名及任务总数；不展示源路径。生成的缩略图与关键帧位于应用数据目录，通过作用域 Tauri asset 协议展示；UI 不接收或展示原始源路径。Windows 上所有外部命令均通过 `process::hidden_command` 使用无控制台窗口标志执行，避免媒体分析或 Jianying 适配器闪现命令行。

### 视觉分析

当前视觉分析覆盖前述历史“单素材首次分析”描述：`analyze_asset` 只执行 FFprobe、缩略图、有限关键帧和 OCR，完成后即为技术 `ready`。单一后台 worker 将最多 6 条技术就绪素材的中间代表帧组成 `analyze_asset_visual_batch`；任务 payload 仅保存素材 ID，结果仅保存安全数量和错误码。模型返回的素材 ID 与源时间必须属于同一批次才会写入视觉证据。视觉状态独立为 `queued`、`running`、`ready`、`failed` 或 `skipped`，Provider/帧/响应失败绝不回退技术 `ready` 或自动无限重试。启动恢复会将有效的中断视觉批次重新排队、将无效 payload 的关联素材封闭为失败，并为旧技术 `ready` 素材补建缺失视觉批次。storyboard 只使用视觉状态为 `ready` 且有视觉证据的素材；brief 会优先推进并有界等待最高相关视觉批次。候选多帧精检仍为 TODO。

### Provider 认证

实验性 OAuth 使用系统浏览器 loopback PKCE 流程，回调校验 state，并通过原生 Windows `keyring` 后端保存凭据。该流程只用于个人测试，不是官方通用 OpenAI 第三方 OAuth。前端通过 Tauri 事件接收状态，并以轮询作为恢复路径；模型弹窗在已连接状态下可调用 `clear_experimental_openai_oauth` 删除凭据并退出登录。同时支持自定义 OpenAI 兼容 API：用户在模型弹窗填写 Base URL、Model 与 API Key，`save_custom_api` 把三者一并保存到 Windows Credential Manager（`clear_custom_api` 可清除）。`ModelAccess::resolve()` 在自定义 API 已配置时优先使用它，否则回退到实验性 OAuth；自定义 API 经 `{baseUrl}/chat/completions` 以 Bearer API Key 鉴权，Rust 侧把 Responses 风格载荷转换为 chat/completions 的 `messages`/`response_format`。API Key 与 OAuth 令牌一律不进 SQLite、浏览器存储、日志或工具结果。视觉分析请求带 30 秒超时，失败或未连接时以 `visualAnalysisNote` 随素材证据返回原因，避免分析线程无限阻塞。

### Agent 循环与请求策略

自然语言编辑控制器（`agent.rs`）把请求分为两条路径。显式单命令走 `run_explicit_command` 确定性直通路径；其余请求进入 `run_agent_loop`。循环加载最近 12 条、总字符预算 8000 的会话历史。`fast_goal` 只锁定带明确动作的产物请求、明确编辑或清晰提问；仅含产物名词的状态短句和快路径未覆盖的自然语言动作保持 pending，不用扩张正向关键词表猜工具。模糊请求不再消耗独立分类调用，而由首次主模型响应同时声明 `goal`/`isQuestion` 并选择第一个技能或直接 `finish`，随后目标锁定且不能被模型改写。schema v7 的 `pending_clarifications` 按项目、剪辑任务和会话保存尚未回答的问题、来源、目标与生命周期；该结构化状态而非消息顺序启发式进入路由和 Agent 状态快照。长段创作文案即使标题使用修辞疑问句，也应结合待澄清问题和历史理解。模型最多编排 10 步；`finish`/`no_action`/`done` 仍只有在真实产物满足门时才结束。每步参数保持 JSON 顶层，技能继续复用作用域、范围、版本和审计边界。

每个普通自然语言请求还会生成只在本轮有效的 `RequestToolPolicy`。该策略不决定应该调用哪个工具，只把用户明确写出的负向副作用约束转换为禁用集合：例如“不生成 preview”“不创建 Jianying draft”“不分析素材”分别禁止对应写工具；排除素材分析还会禁止会下载媒体并触发分析的 `download_music`/`use_online_music`；“只读/readonly”禁止全部编辑与交付工具但保留观察工具，并阻止模型 `taskBrief` 写回任务。Agent `list_assets` 调用独立的无调度快照入口，不再复用会推进桌面后台队列的公开命令；Agent `generate_storyboard` 只消费已就绪证据，不会隐式优先、启动或等待视觉分析。`fast_goal` 不得把被否定的 deliverable 词误认成目标；Conversation Router 只向模型公布允许的首步工具，同时拒绝被排除的选择及依赖被禁工具的目标；Agent loop 对首次目标声明、初始技能和后续每一步再次校验，越界工具以 `user_restricted_tool` 硬拒绝。这样模型仍自主编排允许范围内的观察和编辑，但不能用“合理的后续步骤”为由越过当前请求边界。

交互 Agent 的模型决策共享 90 秒协作式总预算，每次 Provider 请求取 120 秒单步上限与剩余预算的较小值；达到预算后不启动新的模型调用或副作用，但不会强杀已经开始的 FFmpeg、下载、preview 或 Jianying 副作用。安全诊断只记录固定数字耗时与错误码。Provider 调度在请求边界让交互模型调用优先于尚未开始的粗视觉调用；粗视觉连续三次失败后熔断 60 秒，期间批次保持 `queued`，冷却后只允许一个半开探测并恢复 worker。已经开始的视觉请求允许完成，避免取消未知网络状态。

“剪好了吗”“完成了吗”等精确状态问题是无需模型判断的只读单命令：`get_edit_status` 在同一项目、剪辑任务和会话内读取上一条 Agent task 的运行终态，同时以当前 task 的最新 storyboard、该 storyboard 的最新时间线状态和磁盘中实际存在的 preview 文件作为产物事实。任务状态用于说明处理中、待澄清、失败或部分完成，不能把较早的 task result 覆盖到更新的真实产物上；查询不进入通用 loop、不创建 Agent task，也不受后台视觉任务影响。

### 会话路由与持久化

Conversation Router 的公开入口是 `submit_conversation_turn`。它先验证会话作用域，再让精确只读状态直接返回；其他自然语言请求通过一次首轮路由决策选择 `respond`、`clarify` 或 `run`。顶层 route 不增加分支；`goal=question` 额外声明 `informationScope=general|project`。只有 `general` 可即时 `respond`，依赖当前项目素材、任务、产物、数量、状态或故障原因的 `project` 问题必须 `run` 并由模型选择首个观察工具。前两者不创建 `agent_tasks`，`run` 才创建异步任务；若首轮选择了执行工具，该工具通过 `run_agent_loop_with_initial_skill` 作为第 1 步执行，避免再次调用模型选择同一个工具。存在待澄清时，`respond`/`run` 必须在同一模型响应中声明 `keep` 或 `resolve`；新 `clarify` 会 supersede 旧问题。执行型任务创建与 `resolve`、Agent run 的 `needs_clarification` 终态与新问题写入均使用 SQLite 事务。旧 `execute_agent_edit` 保留给兼容调用。

Conversation Router 之前新增 Task Resolver：`resolve_conversation_task` 读取当前项目最近 12 个结构化任务快照，并始终额外包含仍属于该项目的前端显式活动任务，避免旧任务因候选上限被排除；它不读取 `conversations.summary` 或完整聊天历史，输出 `continue_current`、`switch_existing`、`create_new` 或 `clarify`。任何模型自动归属都要求至少 0.85 置信度；低于门槛时保存项目级 `pending_task_routes` 并询问用户。`pendingAction=keep` 保留原请求；旧 pending 只有在绑定它的 route receipt 被提交入口消费后才变为 resolved。已有任务路由与新任务/会话的原子创建都会签发绑定确切目标和完整请求的一次性凭证；user message 在写入事务中唯一占用凭证，胜出 pending receipt 消费时删除未消费 sibling，避免落败凭证污染其他 conversation。`submit_conversation_turn` 与兼容入口在后端强制消费，不能仅靠前端调用顺序绕过。Task Resolver 不选择工具；任务确定后仍由 Conversation Router 和 Agent loop 决定意图、首个技能和后续执行。

Agent run 完成时，`finalize_agent_task` 在同一 SQLite 事务中写入 task 终态、可选产物审计、以任务 ID 派生的确定性最终回复和 conversation 终态；事务提交后才发出 `agent-edit-completed`。事件只是低延迟通知，不再是最终回复的唯一载体。`submit_conversation_turn` 的 `run` 分支以 `{ kind: "run", agentTaskId }` 返回真实任务 ID；Rust enum 字段显式序列化为 camelCase，前端收到空 ID 会失败封闭，不能建立不可对账的 pending ref。前端仍缓存任务 ID 返回前的早到事件，同时在 composer 仍归当前请求所有或持久化 conversation 仍为 `working` 时轮询 `agent_tasks` 终态；一次任务列表暂缺或 task 从 active 变 terminal 都不能结束轮询。没有内存 pending 时，持久化 `working` 本身允许前端对最新同作用域 terminal task 做一次恢复对账，覆盖首次快照已经 terminal 的窗口。事件丢失、窗口切换或快速完成时，会从 SQLite 重载原会话消息、storyboard、时间线和 preview。启动恢复若发现 `working` conversation 的最新 task 已终态却缺少 `agent-task-result-{agentTaskId}`，会把任务标为 `needs_review`、写入固定恢复消息并将会话改为 `review`，绝不猜测已经丢失的模型回答。只有活动项目和剪辑会话与任务作用域一致时才更新当前可见产物。模型响应解析失败只记录固定阶段和响应长度，不记录响应原文。`ModelAccess` 只有在确认自定义凭据不存在时才回退 OAuth，凭据读取错误会阻止请求。Agent loop 超时、解析失败或耗尽步数且未满足目标时，无中间产物持久化为 `failed`，已有真实中间产物为 `partially_completed`；`ask_user` 为 `needs_clarification`。终态状态重新读取失败仍采用 `failed` 的封闭结果。`change_clip_duration` 同时校验已验证源窗口的上下界，并令视频 `sourceEndMs` 精确等于新 `sourceStartMs + newDurationMs`。

Agent loop 的工具失败会在 Provider 边界前转换为临时、脱敏的结构化诊断，只含操作、阶段、安全码、计数事实、可重试性和恢复建议。完整路径、原始日志、媒体证据及用户内容不进入该上下文，也不新增持久化 payload。模型可据此自然解释失败，但任务终态和产物存在性仍由后端决定；模型不可用时继续使用确定性诚实降级。

Jianying 适配器在 Rust 中预校验所有源引用，将版本化 JSON 输入写到应用数据目录后交给 Python 适配器，并在执行后删除输入文件。适配器只支持源时间绑定的视频片段，创建唯一目录，跨进程串行化注册表写入，并在 Jianying Pro 运行或注册表快照变化时中止。唯一 draft 名必须解析为草稿根目录内的单层目录；目录创建后，若轨道构建、保存或注册失败，Python 适配器会回滚本次新建且尚未成功交付的目录，避免失败结果遗留孤立 draft 或重试生成重复产物；既有 draft 从不进入该回滚范围。

Agent loop 每轮调用模型前会从数据库和当前内存产物重建紧凑 `AgentStateSnapshot`，以当前项目/任务/会话、素材分析可用性、真实产物存在性、已执行步骤、剩余步数与未满足条件作为权威状态；确定性前置条件提示只约束真实依赖，不强制所有合法编辑经过 storyboard。循环技能和显式直通技能均持久化步骤开始/终态；应用中断后运行进入 `needs_review`，未完成步骤标记为 `interrupted_requires_review`，但不自动重放。

对话区会把当前作用域最近一次 `agent_tasks` 调用显示为可折叠执行卡。卡片轮询同项目、剪辑任务和 Agent 调用下的 payload-free `agent_run_steps`；父级对话状态同时轮询 `agent_tasks` 终态，避免步骤已结束而卡片仍沿用旧的 `running` 快照。固定工具名会映射为用户可读动作，`finish` 显示为“整理并回答”，不会被误写成副作用；卡片显示步骤状态、已完成数量、运行时长与后端已记录的安全产物类型。模型推理、工具参数、错误原文、本机路径和媒体证据不会进入该卡片；右下角提示仍只表示项目级后台媒体分析，避免与当前对话任务混淆。

### 当前 Agent runtime 覆盖说明

当前实现覆盖上文保留的历史 6 步描述：模型拥有 10 步顶层编排预算，storyboard 另有 3 次内存修订预算。模型可调用 `request_asset_analysis` 对项目内已导入、未分析或分析失败的素材排队；文件、SQLite 和 FFprobe/FFmpeg/Tesseract 一直由 Rust 受控执行。模型可以生成解释性回复，但产物完成事实只能来自工具返回的后端验证摘要，不能被模型总结覆盖；固定降级仅用于 Provider 不可用等无模型回复场景。

### 文本轨

文本轨的第一项受限编辑工具为 `replace_text_tracks`：模型可提交当前作用域时间线的完整文本轨，Rust 会校验时间、样式/布局范围、受限动画与唯一 ID，并按已验证矩阵分配剪映兼容性。已启用文本轨会编译为 ASS 并通过本地 FFmpeg/libass 叠加在 preview；`jianying_default` 字体的静态、淡入/淡出、向上滑入、向下滑入和弹入 cue 可写入 Jianying draft。文本适配器把嵌套文本 JSON 写为 Unicode 转义而非裸 UTF-8，已在当前剪映 11.2 实机验证中文正确显示；其余文本请求仍会明确拒绝，绝不静默丢弃。

模型在制作或改写文本轨前必须先观察目标 `get_timeline` 与 `get_text_capabilities`；若时间线未足以说明画面语义，再观察 `get_storyboard`。每个文本预设提供 `selectionHint`：`subtitle_safe` 用于对白/旁白，`headline_rise` 用于递进或开场揭示，`headline_pop` 用于反差、意外或关键结果，`headline_drop` 用于结论、规则或警示；`callout_card` 与 `cta_card` 仅在用户明确接受 local preview 时可用。同一视觉 beat 至多一个 headline，headline 不得代替普通字幕或与另一 headline 重叠，后端也拒绝跨轨 headline 重叠。`replace_text_tracks` 对阅读密度、超过两行、动画占比和相邻重复文案返回非阻断 `qualityWarnings`，供模型在下一步自主修正；因尚无主体定位证据，它不虚构“文字遮挡人物”的判断。预设由 Rust 边界解析成完整且可审计的样式、布局和动画配方，任何冲突的模型字段都会被覆盖。已验证字幕和标题预设同时固定淡出，避免模型以模板制作入场后遗漏出场；`headline_drop` 使用已验收的向下滑入。目录同时标记每项为可交付 Jianying 或仅 local preview，避免模型把未验证能力当作已交付。

前端会在 Agent 对话工作区展示当前时间线文本 cue 的文案、时间、已解析文本预设、字体、入场和出场模板与 Jianying 兼容状态，供用户审阅模型实际落地的文本设计，而不读取或同步其他 Jianying 草稿。

文本轨的 `layer` 是可交付的叠放语义：local preview 把它写入 ASS event layer，Jianying adapter 按 layer 创建独立且命名的文本轨。一个文本轨内不允许 cue 时间重叠，以匹配 Jianying 轨道段的约束；不同 layer 可以重叠并按层级显示。

## 本地音乐轨（2026-08-12）

版本化 `TimelineContent` 增加 `musicTracks`。每个 cue 绑定已分析的本地音频素材和明确源/时间线范围，可设置循环、音量与淡入淡出。preview 通过 FFmpeg 本地处理和混音，源媒体保持不变。Jianying 适配器现在通过本机 `pyJianYingDraft` 的 `AudioMaterial`/`AudioSegment` 创建独立音频轨，并映射源范围、循环拆段、音量和首尾淡入淡出；已用合成素材创建并注册新的草稿、检查到 1 条音频轨、1 个素材和 3 个循环片段。该结果尚未在 Jianying UI 中试听，所有音乐 draft 均为实验性且需要用户复核，绝不覆盖既有 draft。

Jamendo 是首个可替换线上音乐 Provider。其 `client_id` 仅存 Windows Credential Manager；`search_music` 仅返回 API 明示可下载且为 CC0/CC-BY 的曲目，CC-BY 的曲名、作者和许可 URL 会随 music cue 保存。`download_music` 才按需将单曲写入当前 local project 并交给既有本地分析队列；`use_online_music` 在一个具名、受限且可审计的调用内下载一首、等待分析完成并新建含循环背景音乐的时间线版本。每个下载副本使用唯一文件名，绝不覆盖既有本地副本。不会抓取网页、批量缓存曲库或把未验证的远程 URL 写入时间线/Jianying draft。

场景检测当前覆盖前述历史滤镜描述：固定时间采样（第 1 秒、1/3、2/3、最后 1 秒）替代旧的前 30 秒场景检测，精确提取 4 帧关键帧并拼接为 2×2 网格（640×360 JPEG）供多模态选镜使用。关键帧提取路径记录到 `TechnicalMetadata.keyframe_grid_path`，素材导入时自动生成，网格生成失败只记录警告不阻塞导入。

生成 storyboard 前，brief 仅在本地与素材显示名、文件夹组织 hint 和 OCR 做词汇重合排序；只把纯数字 priority 写入 queued 视觉批次，相同分数按创建时间和任务 ID 稳定排序。最高相关的 queued 或 running 批次最多等待 65 秒。文件名、文件夹和路径不进入 Provider；OCR 不进入粗视觉请求，但仍可作为明确标注的本地提取文字证据进入 storyboard，不能冒充画面语义。storyboard 生成会记录详细日志：入口参数（project_id、editing_task_id、brief 长度）、素材库存统计（总数、视觉就绪数、视频/图片/音频/其他分类计数）、素材样本（前 10 个的 ID/类型/时长）、候选排序与 TOP-5 清单、多模态内容构建、模型请求/响应、每轮重试进度、候选接收情况（shots/beats/时长/未覆盖 beats）、归一化修正（视频范围修正、脚本模式降级）、验证结果及最终失败总结，所有日志使用 Rust `log` crate 的 info/warn/error 级别。素材样本日志可快速识别素材池中视频/图片的实际比例，用于诊断数据库分类与文件系统不一致等异常情况。模型传输复用进程级 `ureq::Agent` 以共享 keep-alive 连接，同时保留每次请求超时。自定义 API 可配置独立粗视觉 Model，空值沿用主 Model；OAuth 不猜测未经验证的替代模型。

## 技术约束

历史“未限定草稿直接指 Jianying draft”的说明已废止。只有明确“创建剪映草稿”才是直通命令；未限定草稿及普通交付请求交给模型工具循环。preview/Jianying draft 必须使用模型已显式选择或已有的作用域时间线，后端绝不暗中创建时间线版本。

素材分析请求由模型分类为无产物门的观察目标，避免“分析素材”被误设为 `generate_storyboard`；模型仍在 `run_agent_loop` 的受限循环中自主选择观察、分析请求或澄清工具，不存在关键词直通执行。

已明确文案的 storyboard 请求若因非前置条件校验失败，循环会把真实失败事实回读给模型继续决策；不得再向用户重复索要主题、风格或时长。顶层 Agent 编排预算为最多 10 步，后续 storyboard 草案修订使用独立的有界预算，避免耗尽创建时间线或 preview 的步骤。模型可基于既有文案重试有效 storyboard 或生成自然语言解释，只有缺少已分析素材等真实前置条件时才允许 `ask_user`。

- FFmpeg/FFprobe、Tesseract（英文 `eng` 数据）、Python 与 `pyJianYingDraft` 是当前开发机依赖，尚未随生产安装包分发。
- Jianying Pro 8.0 的视频草稿与最小文本矩阵（默认字体的静态、淡入、向上滑入）已人工验证能在首页出现并以完整片段打开；图片和音频轨道尚不支持。内部时间线内容使用版本化 `textTracks`，旧时间线安全读取为空；文本 preview、受限文本工具和小范围剪映文本映射已实现。适配器可写入描边、背景、阴影和若干剪映内置字体资源，但在每项经过实机视觉验收前，仍不得将它们表述为可交付能力。
- `App.tsx` 仍较大；在新增可复用领域功能时应继续将类型、组件和服务拆出。

项目事实问答证据门不新增顶层 route：`project + respond` 在原 90 秒预算内最多纠正一次；项目问答的首步和后续技能均为只读观察，且至少一次观察成功后才能结束回答。Conversation Router 返回无效目标/工具而降级为无初始技能 loop 时，后端仍会从显式只读或当前项目的产物、数量、版本、状态措辞保守恢复该证据门，不能因路由失败直接 `finish` 猜测。循环提示明确要求：一次成功观察已经包含用户所问的数量、状态或事实时，下一步直接 `finish` 并基于该结果回答，不调用语义重叠的观察工具只为重复确认；只有明确缺少被问事实时才继续观察。纠正或观察仍失败时封闭失败，不展示模型猜测。

维护记录（2026-08-15）： 的 FFmpeg `-t` 参数改为 `min(source_range, timeline_slot)`，防止源素材短于时间线槽位时生成黑帧。测试模块提取为独立 `preview_tests.rs`，`preview.rs` 预算从 1015 降至 608 行。


维护记录（2026-08-15）：render_timeline_clip 的 FFmpeg -t 参数改为 min(source_range, timeline_slot)，防止源素材短于时间线槽位时生成黑帧；测试模块提取为独立 preview_tests.rs，preview.rs 预算从 1015 降至 608 行。
维护记录（2026-08-15）：agentloop.rs::decide_conversation_route 和 taskrouter.rs::resolve_conversation_task 新增 validate-then-correct 重试逻辑；路由验证失败时把错误原因反馈给模型后重试一次，不改变公开命令或运行时边界。
维护记录（2026-08-16）：移除 agent.rs、agentloop.rs、assets.rs 中所有静默 fallback（遇错返回硬编码合成值），补全被丢弃的真实错误日志；降级路径仍封闭失败，不伪造成功结果，公开命令与运行时边界不变。
维护记录（2026-08-18）：实现四阶段 storyboard 选镜优化系统。models.rs 新增 visual_quality_score 和 scene_duration_ms 字段（Option 类型向后兼容）；新增 storyboard/scoring.rs 独立评分模块（质量/时长/语义/多样性/新鲜度综合评分），request_storyboard 只向模型提供 top-5 候选；新增多样性硬门（连续禁止、同素材≤40%）；新增 storyboard/semantic.rs 和 validation.rs 架构层（接口定义，实现体 TODO）。公开命令与 schema 不变。
维护记录（2026-08-18）：新增 storyboard/multimodal.rs 多模态选镜架构层。定义关键帧网格配置（4-8 帧拼成 2x2/2x4 网格）、generate_keyframe_grid（FFmpeg I 帧提取 + image crate 拼图）和 build_multimodal_content（base64 编码图像块）接口。request_storyboard 检测 keyframe_grid_path 并构建多模态输入，让模型直接从关键帧画面判断语义匹配度。当前阶段接口定义完成，FFmpeg 和 base64 实现体 TODO。公开命令与 schema 不变。
