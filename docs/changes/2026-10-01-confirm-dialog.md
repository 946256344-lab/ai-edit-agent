# 2026-10-01：恢复先确认、再执行

## 原因与改动

09-27 的真实会话删除暴露了 P0 数据安全问题：Tauri dialog 2.7.2 注入的 `window.confirm` 返回 Promise，旧的同步判断将 Promise 当成已确认；该注入函数还调用 Rust 未注册的 `plugin:dialog|confirm`，产生权限/命令不存在错误。现有 `dialog:default` 已包含 message 权限，单纯增加旧 confirm 权限不能修复不存在的命令。

会话删除、导航中的项目删除、项目设置中的清预览缓存全部改为等待同一个 `src/lib/local-store.ts` 确认入口。入口使用官方 JS `confirm()` 调用实际注册的 `plugin:dialog|message`，严格接受 true；取消时不调用后端、不更新领域数据。确认失败进入原有安全失败提示，不继续执行。缓存按钮在等待确认期间保持忙碌状态，取消或失败后恢复可用。

全仓搜索 `window.confirm` 与 `confirm(`，产品源码中原有三处均已替换；保留的官方插件调用只有统一入口。标题和确认/取消按钮新增中英词典键，正文沿用已有双语文案。`src-tauri/capabilities/default.json` 显式声明 `dialog:allow-message`，开发与正式构建共用；Rust 插件注册以及删除、缓存清理后端逻辑保持原有实现。

## 验证

- 仅新增一条回归：`node scripts/test-confirm-dialog.mjs`。运行真实 `App.tsx` 会话删除处理函数、确认桥和官方 dialog JS，模拟桌面 IPC 延迟返回取消；断言确认前以及取消后均未删除、未更新会话列表，并检查实际调用 message 命令。修复前处理函数上重现失败（删除调用为 1），修复后通过（删除调用为 0）。不访问真实数据库或媒体。
- `npm run lint`、`node node_modules/typescript/bin/tsc -p tsconfig.app.json --incremental false`：通过。类型检查不写入共享 node_modules 的构建缓存。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过（4 分 42 秒），仅有既有 storyboard 模块的 6 条 dead_code 警告；capabilities 中的显式 message 权限被构建接受。
- `npm run harness:check`、`git diff --check`：通过。

## 边界与文档

同步 `docs/api.md` 的确认入口契约，以及 `docs/release-checklist.md` 附录 A 的待桌面确认项。不改 `TASKS.md` 或 `docs/decisions.md`。

按派发要求未启动应用或开发服务器，状态为已实现、待桌面确认。需在开发版与正式版、中英文界面逐一核对会话删除、项目删除、缓存清理：先弹框；取消保留数据；确认后才执行；控制台不出现 dialog 权限或 Command not found 错误。自动回归不代替这项桌面验收。
