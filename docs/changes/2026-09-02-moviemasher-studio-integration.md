# Movie Masher 内置时间线工作台整合方案

## 触发范围

本方案面向当前桌面剪辑系统的前端工作台与后端时间线能力，目标是在现有 `storyboard -> timeline -> preview -> Jianying draft` 流程之上，引入 `moviemasher.js` 作为内置时间线工作台的可视化与交互内核。

涉及的现有边界包括：

- `src/App.tsx`：主应用装配、工作区切换、会话路由。
- `src/components/ArtifactsWorkspace.tsx`：成果工作区，展示 storyboard、timeline、preview 和交付审计。
- `src-tauri/src/lib.rs`：Tauri 命令注册。
- `src-tauri/src/models.rs`：`TimelineVersion`、`TimelineClip`、`TextTrack`、`TextCue`、`MusicTrack` 等时间线模型。
- `src-tauri/src/agentloop/tools.rs` 与 `src-tauri/src/agentloop/skills.rs`：Agent 可调用的时间线与交付工具。
- `src-tauri/src/timeline.rs`、`src-tauri/src/preview.rs`、`src-tauri/src/jianying.rs`：时间线生成、预览渲染与草稿交付。

## 背景

当前系统已经具备：

- 自然语言驱动的 Agent 剪辑流程。
- 基于真实素材证据的 storyboard。
- 内部时间线版本化与 preview 生成。
- Jianying 草稿交付。

但当前工作台仍以“成果展示”为主，用户对时间线的微调体验有限。为了实现“剪映式内置体验 + 自然语言 Agent 剪辑”，需要一个更强的可视化时间线工作台，让用户能在同一应用内查看、拖拽、微调和回写时间线，同时保留 Agent 的自动生成能力。

`moviemasher.js` 适合作为这一层的前端编辑/预览内核，因为它具备：

- TypeScript / ESM 体系，和本项目前端技术栈匹配。
- React client，可在现有 React 应用中集成。
- Web 端低分辨率编辑体验，适合内置工作台。
- 服务端渲染与 FFmpeg 输出能力，可作为最终导出方向参考。
- `MPL-2.0` 许可，适合与本项目组合使用时做明确依赖管理。

## 目标

1. 将 `moviemasher.js` 嵌入为应用内的时间线工作台。
2. 保留当前 Agent 剪辑主链路，不替换既有的 storyboard / timeline / preview / Jianying 事实源。
3. 支持时间线的可视化浏览、局部微调、版本化保存和预览刷新。
4. 让自然语言指令优先转化为“编辑意图”或“patch”，而不是直接操作前端状态。
5. 保持审计、回滚和版本对比能力，避免前端编辑状态与持久化事实混淆。

## 非目标

- 不把 `moviemasher.js` 作为唯一事实源。
- 不用它替代现有 Rust 后端的时间线存储与预览生成逻辑。
- 不在第一阶段重写整套 Agent 工具协议。
- 不以外部桌面编辑器替代内置工作台。
- 不在本阶段实现最终视频导出产品化流程的全部细节。

## 总体架构

### 1. Agent 层

Agent 继续负责把自然语言翻译成可执行编辑意图，例如：

- 替换镜头来源。
- 调整片段时长。
- 重排镜头顺序。
- 调整字幕轨。
- 调整音乐轨。
- 请求预览刷新。
- 请求导出或草稿生成。

Agent 只输出结构化的编辑意图，不直接操控前端组件树。

### 2. 内部时间线模型层

内部时间线继续以 Rust 结构为准，至少包括：

- `TimelineVersion`
- `TimelineClip`
- `TextTrack`
- `TextCue`
- `MusicTrack`
- `MusicCue`
- `VoiceoverTrack`
- `VoiceoverCue`

这些模型仍是数据库与后端命令的持久化事实源。

### 3. `moviemasher.js` 映射层

新增适配层，将内部 `TimelineVersion` 转换为 Movie Masher 所需的 mash / track / clip 数据结构；同时把前端编辑结果映射回内部 patch。

这一层只负责：

- 读取内部时间线。
- 构建可编辑 mash。
- 把编辑结果转成 patch。
- 请求后端创建新版本。
- 刷新 preview。

### 4. 后端渲染层

Rust 后端继续负责：

- 生成 timeline draft。
- 预览渲染。
- 音频/旁白/字幕合成。
- Jianying 草稿创建。
- 最终版本化持久化。

## 推荐工作区形态

建议在现有工作区基础上新增一个专门的 `studio` 视图：

- `chat`：自然语言编排与 Agent 交互。
- `assets`：素材库与证据浏览。
- `artifacts`：storyboard / timeline / preview / 审计总览。
- `studio`：内置时间线工作台，承载 `moviemasher.js` 交互编辑。

`studio` 视图应具备：

- 左侧：项目、会话、版本列表。
- 中间：`moviemasher.js` 时间线和预览。
- 右侧：属性面板、Agent 建议、编辑历史、操作审计。

## 数据映射原则

### 内部时间线 -> Movie Masher mash

映射时遵循以下原则：

1. 一条内部 `TimelineVersion` 对应一个 mash。
2. `clips` 映射为视频/图像片段轨。
3. `textTracks` 映射为字幕/标题/标注轨。
4. `musicTracks` 映射为音频轨。
5. `voiceoverTracks` 作为独立音频轨或旁白轨。
6. 所有结构都必须保留稳定 ID，以便回写 patch。

### Movie Masher 编辑结果 -> 内部 patch

前端编辑后，不直接覆盖整条时间线，而是生成 patch：

- `replace_clips`
- `change_clip_duration`
- `reorder_clips`
- `replace_text_tracks`
- `replace_music_tracks`
- `apply_editor_patch`

patch 应是可审计、可回放、可版本化的。

## 后端接口建议

建议增加下列能力：

1. `export_timeline_to_moviemasher_mash`
   - 将当前 `TimelineVersion` 导出为前端可消费的 mash。

2. `apply_moviemasher_patch_to_timeline`
   - 根据前端编辑生成新的 `TimelineVersion`。

3. `create_timeline_version_from_mash`
   - 用于保存编辑器当前状态并创建新版本。

4. `render_preview_from_timeline_version`
   - 根据最新版本刷新低分辨率预览。

5. `commit_editor_changes`
   - 对外暴露给前端，统一执行保存、审计与刷新。

这些接口应保持与现有 Tauri 命令风格一致，并继续采用版本化结果返回。

## Agent 工具层建议

为了让自然语言继续驱动剪辑，建议新增面向工作台的工具概念：

- `load_timeline_for_editing`
- `preview_timeline_patch`
- `apply_timeline_patch`
- `undo_timeline_patch`
- `commit_editor_changes`
- `export_preview_render`

这些工具不应泄露前端实现细节，Agent 只需要知道：

- 当前时间线是什么。
- 改了哪些内容。
- 是否需要刷新预览。
- 是否要创建新版本。

## 实施阶段

### 阶段 1：只读集成

目标：把现有 timeline 渲染进 `moviemasher.js`，不改变写入逻辑。

交付：

- 新增 `studio` 视图入口。
- 时间线到 mash 的只读转换。
- 可显示当前 timeline 与 preview。

### 阶段 2：单点微调

目标：支持局部修改并生成新版本。

交付：

- clip 时长调整。
- clip 顺序调整。
- text track 编辑。
- patch 落库。
- preview 自动刷新。

### 阶段 3：Agent 联动

目标：让自然语言可以直接驱动工作台编辑。

交付：

- 自然语言 -> 编辑意图。
- 编辑意图 -> patch。
- patch -> 工作台预览。
- 用户确认后保存为新版本。

### 阶段 4：导出与交付

目标：把工作台结果统一接入最终交付链路。

交付：

- 统一 preview / render 路径。
- 保留 Jianying 草稿作为可选交付方式。
- 为未来正式导出能力预留接口。

## 风险与缓解

### 风险 1：数据模型不一致

`moviemasher.js` 的内部 mash 结构与当前 timeline 结构不完全一致。

缓解：

- 采用显式适配层。
- 不让前端直接依赖数据库模型。
- 保持 patch 与版本号概念。

### 风险 2：前端状态漂移

编辑器状态与后端事实可能不一致。

缓解：

- 保存即创建新版本。
- 不做无版本号原地覆盖。
- 所有事实以 Rust 返回为准。

### 风险 3：Agent 误把前端状态当作最终事实

缓解：

- 只向 Agent 暴露稳定工具输出。
- Agent 读的是版本化结果，不是 UI 内部状态。

### 风险 4：复杂编辑导致交互性能下降

缓解：

- 前端只处理当前会话。
- 大部分计算仍放在 Rust/FFmpeg 层。
- 只刷新必要片段和预览。

## 验收标准

### 阶段 1 验收

- 当前 `TimelineVersion` 可成功渲染到 `moviemasher.js`。
- 预览区域可见，且与后端 timeline 对应。
- 不影响现有 chat、assets、artifacts 工作流。

### 阶段 2 验收

- 可拖拽/调整片段并保存为新 timeline version。
- 保存后可刷新 preview。
- 操作可审计并可回看版本。

### 阶段 3 验收

- 自然语言能触发局部编辑。
- Agent 给出的 patch 可被工作台执行。
- 用户可确认、撤销或继续微调。

### 阶段 4 验收

- 编辑结果能稳定进入最终导出或草稿交付链路。
- 预览、版本、审计与交付状态保持一致。

## 与现有实现的关系

当前项目中已经存在稳定的时间线与预览能力：

- `src-tauri/src/timeline.rs`
- `src-tauri/src/preview.rs`
- `src-tauri/src/agentloop/skills.rs`
- `src-tauri/src/agentloop/tools.rs`
- `src/components/ArtifactsWorkspace.tsx`

本方案不是推翻这些能力，而是在其上增加一个更强的内置工作台层，让用户和 Agent 可以在同一应用内完成更自然的时间线微调。

## 结论

`moviemasher.js` 适合用作本项目的内置时间线工作台内核，但前提是：

1. 它只负责编辑体验和可视化。
2. 内部时间线仍以 Rust 模型和版本化命令为准。
3. Agent 通过编辑意图和 patch 与工作台协作。
4. 保存、预览、审计与导出都要走后端可信链路。

这是最符合当前“剪映式内置体验 + 自然语言 Agent 剪辑”目标的整合方式。
