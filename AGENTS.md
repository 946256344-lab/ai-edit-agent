# Assembly Video Agent：Agent 上下文

## 产品规则

Assembly Video Agent 是 Windows 优先的本地视频剪辑 Agent。用户主要通过自然语言协作；视频分析、storyboard、时间线、preview 和 Jianying draft 创建是 Agent 可调用的工具。

- 构建 Agent，不构建带聊天框的传统剪辑器。
- 必须使用真实媒体分析和明确源时间范围；不得从文件名推测媒体内容。
- Agent 可不经逐项确认创建内部时间线、低清 preview 和新的 Jianying draft。
- 最终导出、覆盖既有导出，或删除项目、素材、版本前必须获得明确确认。
- 原始媒体、项目数据、preview、内部时间线和 Jianying draft 必须保留在本机。
- Provider 必须可替换；OpenAI OAuth 只是支持入口之一。
- Jianying 交付是单向的：MVP 中不得覆盖既有 Jianying draft，也不得尝试同步用户在 Jianying 内的编辑。

## 当前实现边界

仓库包含 React/Vite 前端和 Tauri 2 Windows 后端，具备本地 SQLite、原生素材导入、FFprobe/FFmpeg/Tesseract 分析、右下角媒体分析任务提示、实验性 OAuth 模型调用、证据绑定 storyboard、源时间绑定时间线、本地 preview，以及实验性的 Jianying Pro 8.0 仅视频草稿创建。外部命令在 Windows 上必须无窗口运行。自定义 Provider、生产安装包媒体运行时、多轨音频/字幕、最终视频导出和反向同步尚未实现，不得声称已经具备。

进行非简单修改前，必须阅读 `docs/architecture.md`、`docs/decisions.md`、`docs/api.md`、`docs/roadmap.md`、`docs/harness.md` 与 `TASKS.md`。涉及架构决策、公开工具契约或任务状态时，必须更新对应文档。

## 编码标准

- 使用仓库已有的 TypeScript 和 React 19 模式。
- 保持 `src/App.tsx` 聚焦组合；实现真实可复用功能时提取领域类型、UI 组件与服务。
- `verbatimModuleSyntax` 已启用，纯类型导入使用 `import type`。
- 严格 TypeScript 检查中，未使用的局部变量与参数都是错误。
- 优先小而明确的函数和领域名称，避免泛化工具函数。
- 没有明确需要不得新增依赖；新增依赖必须在 `docs/decisions.md` 记录理由。
- 保持既有深色、信息密集的视觉语言和响应式断点，除非有明确设计变更。
- 面向用户的中英文文案统一使用 Agent、storyboard、draft、preview、Jianying draft、local project 等产品词汇。
- 不得将 token、API key、含用户数据的本机路径或媒体内容写入源码、浏览器存储、日志或文档示例。

## 开发流程

修改前：

1. 检查相关实现并阅读上下文文档。
2. 判断变更是否影响产品规则、数据所有权、工具契约或持久化。
3. 开始实质任务前检查并更新 `TASKS.md`。
4. 未知项标记为 `TODO`；不得编造 API 行为、OAuth scope、Jianying JSON 字段或兼容性结论。
5. 触发 `.harness/doc-sync-policy.json` 的改动必须在同一变更集中更新要求的文档和 `docs/changes/` 记录。

修改中：

1. 浏览器原型状态与生产持久化保持分离。
2. 每个副作用必须是具名、可审计的工具调用。
3. 作用域受限请求只能修改目标时间线或 storyboard 区域。
4. 不得为了便利而覆盖或删除用户资料。

修改后：

1. 前端变更运行 `npm run lint` 和 `npm run build`。
2. 运行 `npm run harness:check`；触发架构规则时按 `docs/harness.md` 完成独立 Agent 审查 loop。
3. 更新 `TASKS.md` 和相关 `docs/` 文件。
4. 精确报告实现行为、验证结果和剩余 `TODO`。

## 目录职责

- `src/App.tsx`：当前原型 UI 与本地展示状态。
- `src/App.css`、`src/index.css`：应用和全局样式。
- `src/lib/agent-tools.ts`：Agent 工具目标契约。
- `src-tauri/`：Tauri 2 Windows 壳与 Rust 命令边界。
- `docs/`：长期产品和工程上下文。
- `TASKS.md`：当前执行状态。
- `README.md`：入门与本地运行说明。

## 命令

```powershell
npm install
npm run dev
npm run lint
npm run build
npm run tauri:dev
npm run tauri:build
```
