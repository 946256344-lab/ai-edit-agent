# Studio 内置工作台：moviemasher.js 整合落地

## 触发范围

`src-tauri/src/lib.rs` 新增 `studio::commit_studio_edits`；新增 `src-tauri/src/studio.rs`；前端 `src/lib/local-store.ts` 新增 `commitStudioEdits` 与对应类型；新增前端适配层 `src/lib/moviemasher-adapter.ts`、controller `src/hooks/useStudioWorkspaceController.ts`、视图 `src/components/StudioWorkspace.tsx`；`src/App.tsx`、`src/components/WorkspaceHeader.tsx`、`src/components/workspace-types.ts` 新增 `studio` 工作区；样式 `src/App.css`；`docs/api.md`、本变更记录。

## 改动

### 依赖与事实源
- 引入 `@moviemasher/moviemasher.js@5.1.1`（MPL-2.0）作为前端时间线工作台的 mash 中间态概念参考与类型依托，未替换既有的 `TimelineVersion` 事实源。
- 内部持久化事实仍以 Rust 侧 `TimelineVersion`（`timeline_versions` + `TimelineContent`）与版本化 audit 为准，前端 mash 仅为可编辑投影。

### 适配层（纯前端）
- `moviemasher-adapter.ts`：`TimelineVersion -> Mash`（tracks/clips 映射，保留稳定 `shotIndex` 与 cue ID）、`Mash -> MashDiff`（reorder / duration / text 三类变更检测与 `TextTrack` 重建），以及共享时间格式化与文本样式默认值。
- 映射遵循方案：一条 `TimelineVersion` 对应一个 mash；`clips` 为视频主轨，`textTracks` 各为一轨；回写为 patch/diff，不做无版本原地覆盖。

### Studio controller / 视图
- `useStudioWorkspaceController`：以 `TimelineVersion` 为基底派生并维护可编辑 `Mash`，提供时长微调（200–12000ms 段，超出后续平移）、镜头上移/下移重排并重算连续位置、字幕文案编辑、缩放与 `hasChanges` / `diff` 派生。
- `StudioWorkspace`：顶部 `studio` 工具栏、缩放与保存入口；左置 preview（与 `ArtifactsWorkspace` 一致的 `convertFileSrc` + `previewNonce`）、中置可视化时间线（ruler + tracks + clips）、底部属性面板；空状态引导回到 Agent 生成故事板；提交成功后通过 `renderPreview` 刷新预览。

### 后端
- `studio::commit_studio_edits`（`payload: StudioCommitPayload`）：以 `TimelineVersion` 为基底，顺序应用 `reorder`（全量排列校验）、`adjustments`（`shotIndex/newDurationMs/newSourceStartMs`，复用与 `timeline.rs::change_clip_duration` 一致的已验证源范围校验：视频窗口不得超出原验证窗口与文件时长，图片为零源范围）、`textTracks`（复用 `validate_text_tracks`）；任一类至少一项才落库；生成新 `version_number` 的 `TimelineVersion` 与 `TimelineContent`，写入 `timeline_versions` 与一条 `user/studio_commit` 的 `operation_logs`；返回 `StudioCommitResult { timeline, applied }`。预览由前端另行 `renderPreview` 触发。
- 作用域校验：`project_id` 与 `editing_task_id`（对应前端 `sessionId`/`editingTaskId`）均需匹配 storyboard 归属。

### 路由与状态
- `WorkspaceView` 扩展 `studio`；`WorkspaceHeader` 增加 `工作台` 入口。
- `App.tsx` 组合 `useStudioWorkspaceController`，`studio` 视图接入并通过 `applyStudioCommit` 将新版本与预览写回 `useArtifactWorkspaceController` 事实。

## 同步文档

- `docs/api.md`：新增 `commit_studio_edits` 行（输入/结果/说明）。
- `docs/changes/2026-09-02-moviemasher-studio-integration.md`：方案草案（前置讨论产出）。
- `docs/changes/2026-09-02-studio-workspace-moviemasher-integration.md`：本记录。

未同步 `README.md` / `TASKS.md` / `AGENTS.md`：本期仅落地可微调工作台基础能力与版本化保存，未新增需要用户侧文档的操作与任务清单语义；`docs/codebase` 本期忽略。

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml` 通过。
- `npm run build`（`tsc -b` + `vite build`）通过（45 modules，dist `assets/index-Cthk_yLj.js` 等）。
- `npm run lint` 通过。
- `npm run harness:check` 前：`agent-contracts` 因 `commit_studio_edits` 未写入 `docs/api.md` 失败；补文档后需复检。

## 决策

无 ADR。沿用现状：Rust 侧为 timeline 唯一可信边界，前端工作台状态不直接持久化，保存即创建新版本。
