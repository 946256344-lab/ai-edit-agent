# 2026-09-25：精修浅色界面改版

## 结果

桌面应用按确认的设计稿改为苹果白风格：纯白画布配 `#F5F5F7` 侧栏，系统字体（Mac 用 SF 与苹方，Windows 回落到 Segoe UI Variable 与微软雅黑 UI，不打包字体），最小字号 12px。颜色、字号、圆角、阴影、动效收成 `src/index.css` 里的一套设计变量，其余样式只引用变量。

主要体验变化：

- 侧栏会话统一用单色竖屏框图标（输出固定为竖屏 540×960），只有正在剪辑的会话用紫色；项目图标改为统一的叠放卡片，不再取会话封面。
- 助手处理进度默认只露一行：状态、用时、已完成步数；点开才以小字列出步骤。需要回复、失败、已停止的提示始终可见。
- 用户消息改为右侧浅灰气泡；助手消息带 F 字标。停止键收进输入框（处理中时发送键变为停止），快捷键提示移入输入框底栏。
- 预览改为白色卡片里的浅灰舞台，画面带柔和投影；进度条按镜头分段（已播深色、当前紫色）；镜头条一屏 6 格，可横向滑动，当前镜头自动滚入视野。
- 顶栏去掉“粗剪工作台”标签行，面包屑显示“项目 / 会话”或“项目 / 素材库”；侧栏“素材库”按钮改为开关，再点一次回到剪辑。
- 素材库改为分段式状态筛选、大缩略图卡片（时长角标、状态圆点、悬停出现勾选）；弹窗统一白色圆角面板与模糊遮罩。

## 触发范围

- `src/index.css`、`src/styles/*.css`（新增 `shell`、`conversation`、`preview`、`assets`、`dialogs`、`responsive`）、`src/components/asset-analysis.css`、`src/components/asset-workspace/asset-evidence.css`。
- 删除 `src/App.css`（其中约 70 个类已无组件使用，其余深色规则被浅色层覆盖）与 `src/light-workspace.css`，由上述文件取代。
- `src/App.tsx`、`AppSidebar`、`WorkspaceHeader`、`AgentWorkspace`、`AgentRunCard`、`RoughCutPreview`、`RoughCutPlayer`、`EditorOutputPort`、`WorkspaceIcon`。
- 删除 `src/hooks/useSessionArtworkController.ts`：侧栏不再显示素材封面，每个会话一次的分镜与关键帧读取随之取消。
- `src/lib/i18n/zh-CN.ts`、`en.ts`：删除不再使用的文案键（对话小标题、进度折叠标题、会话首字、封面读取失败、工作台标签、预览空闲提示）。

## 同步文档

- `TASKS.md`、`src/AGENTS.md`（视觉规则由“保持深色视觉”改为引用设计变量）、`docs/codebase/STRUCTURE.md`。

## 禁止变化

- 不改公开 Tauri 命令、前端 bridge、SQLite schema、Agent 工具目录或任何剪辑、交付行为。
- 保留窗口的原生透明根背景，由应用外框绘制白底。

## 验证

- `npx tsc -p tsconfig.app.json --noEmit`、`npm run lint`、`npm run i18n:check`、Agent 契约与文档同步检查、`git diff --check`：通过。`harness:check` 的架构预算项在改动前即失败（`App.tsx` 15 个 `useState` 超过 14，来自 14ad3d7），本次未增减，已记入 `TASKS.md`。
- 开发版桌面（WebView2 调试端口截图）逐项核对：对话页、折叠与展开的处理进度、用户气泡、预览分段进度条与 6 格镜头条、素材库卡片与筛选、模型设置弹窗；1440×900、1100×760、720×900 三种尺寸布局正常，无整页滚动条。
- 待真实回合验收：处理中的转圈与文字微光、镜头替换面板、新建项目与素材分析弹窗。

## 决策

无新增 ADR。

## 后续

- 助手回复是 Markdown，但消息仍按纯文本显示，会露出 `##`、`**` 等符号；需另做消息 Markdown 渲染。
