# 每个会话的版本号从 v1 开始

## 现象

项目「测试1」新开一个会话，第一次生成的故事版就是 v7，时间线是 v8，预览也显示 v7。v4–v6 属于另一个会话。

## 根因

故事版和时间线的下一个版本号按 `project_id` 取 `MAX(version_number) + 1`，同项目所有会话共用一条序列（`storyboard.rs` 写入故事版，`timeline.rs` 两处、`studio.rs`、`storyboard.rs` 音频优先路径写入时间线）。两张表建表时就有 `UNIQUE(project_id, version_number)`，不能直接改成按任务编号。

## 触发范围

- `src-tauri/src/db.rs`：schema 19→20，给 `storyboard_versions`、`timeline_versions` 追加可空列 `task_version_number`，不回填；新增 `NextVersionNumbers`。
- `src-tauri/src/storyboard.rs`：新增 `next_storyboard_version_numbers`，按 `(project_id, editing_task_id)` 编号；`insert_storyboard_version`（含 `reselect_shots` / `refine_shot_ranges` 派生版本）写入两个编号；读取改为 `COALESCE(task_version_number, version_number)`。
- `src-tauri/src/timeline.rs`：新增 `next_timeline_version_numbers`，经 `storyboard_version_id` 关联到任务后编号；草稿创建、`insert_timeline_version_with_log` 与全部读取同步。
- `src-tauri/src/studio.rs`、`storyboard.rs` 音频优先时间线：改用同一编号函数。
- `src-tauri/src/agentloop/snapshot.rs`、`src-tauri/src/taskrouter.rs`：快照与任务状态显示会话内版本号。
- `src-tauri/src/timeline_tests.rs`：手写测试表补 `task_version_number` 列与所属故事版行。

## 改动

Tauri 命令签名不变。`StoryboardVersion.versionNumber`、`TimelineVersion.versionNumber`、Agent 快照和工具返回的 `versionNumber` / `timelineVersionNumber` 都改为会话内版本号，每个剪辑任务从 1 开始。`version_number` 列仍是项目内序号，只用于唯一约束和排序（`ORDER BY` 不变）。

为什么不重建表：去掉 `UNIQUE(project_id, version_number)` 需要重建两张带外键的表（`storyboard_recommendations` 对故事版是级联删除），和「迁移只追加」冲突，重建出错会损坏用户数据。为什么不改历史编号：旧会话里用户和 Agent 消息已经用「v6」指代具体版本，改号会让这些引用对不上。旧行 `task_version_number` 为空，读取时回退到原序号，旧会话的新版本从该会话已有最大号往后接（例如「测试1」当前会话已有故事版 v7、时间线 v8，下一版分别是 v8、v9），新会话从 v1 开始。

## 跨会话串用排查

逐项确认后，没有发现把其他会话的产物当成本会话产物的情况：

- 状态快照、`get_edit_status`、任务路由状态按 `editing_task_id` 取最新故事版，再按该故事版取时间线；`get_latest_timeline`、`list_timeline_versions` 限定到具体故事版，后者还校验它属于当前任务。
- 前端 `useArtifactWorkspaceController` 按当前会话列故事版，时间线按所选故事版读取；切换会话时清空状态，异步结果回来时核对项目和会话。
- `previews/cache/<project_id>` 只存中间文件，键是源文件路径、大小、修改时间、区间和画面参数的哈希，相同输入复用相同字节；成片预览写在 `previews/<timeline_id>/`。
- 剪映 / CapCut 草稿名为「项目名-随机 8 位」，注册记录绑定 `timeline_version_id`；FCPXML / OTIO 文件名同样带随机后缀，重名再追加后缀。

保留一处项目级影响：选镜打分的「新鲜度」读同项目各任务最新时间线的素材使用次数，给用过的素材降权。它只影响候选排序，不引用产物。经确认保持项目级，避免同项目多条成片反复用同一批镜头，已写入决策和架构说明。

## 同步文档

`docs/architecture.md`（作用域架构）、`docs/decisions.md`、`docs/api.md`、`TASKS.md`。

## 验证

`cargo check` 通过；`cargo test --lib` 430 条通过，新增回归 `each_editing_task_numbers_storyboards_and_timelines_from_one`：用真实迁移后的库，第二个任务的第一版故事版和时间线都是 v1，重新读取也是 v1。待桌面确认：新建会话生成后，预览和故事版显示 v1。

## 决策

`docs/decisions.md` 新增「每个会话完全独立，包括产物和版本号」，取代原先「会话只是对话容器、多会话共享产物」的描述。
