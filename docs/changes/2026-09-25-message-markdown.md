# 2026-09-25：助手消息 Markdown 渲染

## 结果

助手回复按 Markdown 显示：标题、加粗、列表、表格、引用、行内代码与代码块、分割线都有排版，不再露出 `##`、`**` 等符号。模型常用单个换行分行，渲染时保留为分行。用户自己的消息仍按原文显示。

## 触发范围

- 新增 `src/components/MessageMarkdown.tsx`，`AgentWorkspace` 对助手消息改用它渲染。
- `src/styles/conversation.css` 新增 `.message-markdown` 排版，只引用设计变量。
- `package.json` / `package-lock.json` 新增依赖 `react-markdown`、`remark-gfm`（GFM 表格、删除线、任务列表）。

## 安全边界

- 不执行原始 HTML：`react-markdown` 默认把 HTML 当文本，未启用 `rehype-raw`。
- 链接只放行 `http`、`https`、`mailto`，点击交给系统浏览器（`opener:default` 已有权限），应用窗口不跳转；其余协议只显示文字。
- 不加载图片：模型给出的图片地址不可信，也不应让本地应用静默联网，只显示图片说明文字。

## 同步文档

- `TASKS.md`、`docs/codebase/STRUCTURE.md`。

## 验证

- `npx tsc -p tsconfig.app.json --noEmit`、`npm run lint`、`npm run i18n:check`：通过。
- 通过开发服务器临时页面渲染仿真回复（标题、加粗分行、编号列表、表格、引用、行内代码、代码块、https 与 `javascript:` 链接、远程图片）：排版正确；只生成一个 https 链接，点击后页面不跳转；没有生成 `img` 或 `script`。临时页面已删除。
- 待真实桌面验收：点击链接后由系统浏览器打开。

## 决策

无新增 ADR。新增的两个依赖只用于前端展示，不涉及数据或 IPC。
