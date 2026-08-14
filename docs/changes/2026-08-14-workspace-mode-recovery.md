# 2026-08-14 工作模式恢复

## 问题

真实 Tauri 桌面审计确认，后期组件拆分把原本互斥的 Agent 与 storyboard 变成长页面追加关系，丢失 storyboard 基础样式，并把三栏素材工作台放入固定窄侧栏。构建通过，但用户无法直接找到和检查真实产物。

## 改动

- 顶层模式收敛为 Agent、素材、成果，一次只渲染一个主工作区。
- Agent 模式只显示消息、执行任务卡和 composer。
- 素材模式让 `AssetManagementPanel` 占用完整主区域，并让“全部素材”直接展示当前 100 条有界页。
- 成果模式只保留一套 Workflow，集中展示 storyboard、timeline/审计和 preview。
- 恢复 storyboard 镜头网格、卡片、时间范围与调整入口样式；补齐 Agent 执行卡和审计卡样式。
- 应用固定为 viewport 高度，各模式使用独立内部滚动，禁止模式内容通过全局长页面叠加。

## 边界

- 没有修改 Tauri 命令、Agent 工具、Provider、SQLite schema、媒体分析、timeline、preview 或 Jianying 实现。
- 没有执行最终导出、覆盖、删除、重新分析或新的 Provider 请求。

## 验证

- `npm run lint`：通过。
- `npm run build`：通过。
- `npm run harness:check`：通过；本次前端与文档变更未触发额外架构同步规则。
- `git diff --check`：通过，仅有 Git 的 LF/CRLF 工作区提示。
- 真实 Tauri WebView 以 1440×900 viewport 验收：
  - Agent 模式没有全局页面溢出；消息区独立滚动，composer 保持可见；素材、Workflow 和 storyboard DOM 均不存在。
  - 素材模式占满 1188×773 主区域，三栏宽度分别为 308/520/360px；“全部素材”显示当前 100 条有界页，匹配总数为 891；对话与 Workflow DOM 均不存在。
  - 成果模式仅有一套 Workflow 和一套 storyboard，显示 8 个镜头；成果区独立滚动，preview 位于同一产物流中；composer 与素材工作台 DOM 均不存在。
- 已检查三个模式的桌面截图；截图包含本地项目名称、会话和真实媒体缩略图，因此不写入仓库。

## 同步文档

- `TASKS.md`
- `docs/audits/2026-08-14-desktop-product-baseline.md`
- `docs/architecture.md`
- `docs/decisions.md`
- `docs/roadmap.md`
