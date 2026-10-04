# 长期文档历史补充

原实现与验证记录按当时文档保留，不代表当前契约。

## 原长期文档补充：素材分析进度与剪辑等待（2026-09-16）

来源：`docs/architecture.md`，文档整理前的历史表述；现状以长期文档为准。

`assets/progress.rs` 将技术分析和首次画面识别投影为四种互斥状态，为素材页筛选及项目级进度提供统一口径。导入弹窗保留 `AssetAnalysisProgress`；素材库把筛选与进度条收到列表标题区，有素材时始终显示进度条。侧栏「素材库」用状态点提示进行中或待处理。对话只在用户已发送且素材未分析完时弹出确认：是否只用已分析素材开始。可取消发送、去素材库，或在已有可用视频时继续。分析全部完成后自动开始，不再在输入框里排队等待。`useAssetWorkspaceController` 负责导入状态、筛选和失败重试，`useAnalysisGateController` 负责可取消的确认。发送时冻结文案与媒体选项，确认或分析完成后才启动原有任务路由。切换项目/会话取消这次发送，不跨范围发送。候选读取要求首次画面分析 ready，不使用失败素材的部分证据，不等待剪辑中的 Phase 4 精修。

`assets/controls.rs` 持有首次分析取消/继续与素材库编辑。取消标志和移除标志存于现有 metadata JSON；任务提交同时检查任务状态，避免取消后迟到响应覆盖继续分析的结果。取消混合视觉批次时保留未选中素材并重新排队。移除为索引隐藏，不删除源文件和资产行；新库查询/选镜排除隐藏项，已有时间线仍能使用原始引用。重命名仅改显示名。

`useAssetAnalysisController` 与 `AssetAnalysisModal` 跟踪本次导入，提供按实际速度估算的剩余时间、后台分析和取消/继续。`useAssetLibraryEditController` 管理当前列表批量选择、重命名和移除确认；目录、状态筛选及项目切换清空选择。取消素材分析后，再次发送会弹出确认，需明确选择只用已分析素材或取消这次发送。

## 原长期文档补充：2026-09-16：素材分析进度与剪辑等待

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

新增命令：

| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `get_asset_analysis_progress` | projectId, assetIds? | AssetAnalysisProgress |
| `cancel_asset_analysis` | projectId, assetIds? | 取消数量 |
| `resume_asset_analysis` | projectId, assetIds? | 继续数量 |
| `rename_library_asset` | projectId, assetId, name | void |
| `remove_library_assets` | projectId, assetIds | BatchAssetActionResult |

- `get_asset_analysis_progress`（projectId, assetIds?）：轻量读取项目或本次导入进度；省略 ID 集合为整个项目。
- `cancel_asset_analysis` / `resume_asset_analysis`（projectId, assetIds?）：取消/继续首次分析，返回处理数量；已完成结果保留。取消持久化为 `metadata.analysisCancelled`，计入 queued 和 cancelled；活动数为 analyzing + queued - cancelled。取消的任务不在启动时恢复，当前外部调用允许结束，但不可回写或继续后续阶段。
- `rename_library_asset`（projectId, assetId, name）：仅改显示名，不改本地文件名。
- `remove_library_assets`（projectId, assetIds）：返回批量操作计数，先取消未完成首次分析，再标记 `metadata.libraryRemoved`；库列表、统计、搜索与新剪辑候选不再读取它，保留资产行、源文件和已有时间线引用。共享素材的编辑作用于该素材及引用它的所有项目，前端移除确认说明此范围。

导入弹窗跟踪本次导入的 ID，预计剩余时间按本批次实际完成速度估算，尚无完成项时显示“正在估算”。“后台分析”仅关闭弹窗；弱化的“取消分析”取消该批次。素材库列表标题提供项目级取消/继续/重试；对话等待提示不再承担分析控制。

`list_asset_page` 增加可选 `analysisState: 'ready' | 'analyzing' | 'queued' | 'failed'`，与目录等现有筛选组合。新增返回 `progress: { total, ready, analyzing, queued, failed, readyVideo, cancelled }`，按当前项目的共享素材范围去重统计，不受分页或筛选影响；旧 `counts` 保持原义。

首次分析状态同时考虑技术分析和画面识别，视频/图片只有两者完成才为 ready；音频/其他类型只需技术分析。技术失败、画面失败或已跳过画面识别归入 failed（详情保留已跳过说明），queued/running 不算已完成。`readyVideo` 另排除手动排除、缺失、变化和不可读的视频。`retry_asset_analysis_batch` 复用两阶段失败重试，最多 200 项，仅重试失败步骤，返回原有 requested/updated/skipped 计数。

对话入口在调用任务路由和 `submit_conversation_turn` 前检查当前项目首次分析。素材未全部完成时弹出确认，询问是否只用已分析素材；取消则保留输入框文案。分析全部完成后自动继续。文案和媒体选项按点击提交时快照保留。确认仅存在当前应用页面，取消、切换项目/会话会取消这次发送；重启不会自动续发。素材导入及重试入队期间不自动放行。`storyboard_sources` 只接收技术和首次画面分析均 ready 的视频，失败素材上的部分段卡不参与召回；Phase 4 精修仍在剪辑中执行。

## 原长期文档补充：2026-09-16：共享子素材库与新建项目

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

`list_shared_libraries` 无参数，返回 `{ id, name, assetCount, unfiled }[]`，不暴露源目录；`unfiled` 标出无导入根目录的默认子库，存储名仍为「未归类素材」，前端按界面语言显示。`create_project` 接受 `{ name, libraryIds?: string[] }`：省略时选取当前全部子素材库，空数组不关联素材库；名称与库关联在同一事务提交。前端先弹出表单并默认全选。

Schema 18/19 增加 `shared_libraries`、`project_libraries`、`shared_library_assets` 和 `project_asset_access` 视图。按已有导入根目录建立子库，无目录素材归入“未归类素材”；成员通过素材 ID 关联，重链路不丢失库归属。源文件及分析结果不复制，旧项目保留原有素材访问。新导入自动关联当前项目；浏览、搜索、选片、替换、预览与交付统一使用共享范围。`assets.project_id` 保留为导入来源，不再是素材读取的唯一范围。
