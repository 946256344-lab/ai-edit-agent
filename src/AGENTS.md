# 前端说明

本目录为 `src/`，负责 React 展示和领域状态组织。

- `App.tsx` 只做组合，领域状态放在对应 controller，展示放在 component。
- 应用 Tauri 命令经 `src/lib/local-store.ts` 调用，组件不直接 `invoke`。
- 持久化事实在 SQLite，浏览器内存只作展示；终态以持久化和磁盘结果为准。
- 只展示安全的用户提示，不暴露凭据、本机路径或底层错误原文。
- 界面文案只写进 `src/lib/i18n/zh-CN.ts` 与 `en.ts`（英文漏键会编译失败）；组件用 `useI18n()`，非组件代码用 `messages()`。`npm run i18n:check` 拦截写死的中文。
- 视觉为精修浅色：颜色、字号、圆角、阴影、动效只引用 `src/index.css` 的设计变量，不写死色值；样式按区域放在 `src/styles/`，响应式断点集中在 `src/styles/responsive.css`。
