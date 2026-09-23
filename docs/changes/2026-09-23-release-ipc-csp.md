# 修复 Release IPC 安全策略

## 问题

正式版 WebView 能显示静态界面，但 CSP 的 `connect-src` 没有允许 Tauri 2 在 Windows 上使用的 `ipc:` 与 `http://ipc.localhost`。因此窗口插件、事件订阅、项目初始化和 Provider 状态命令都被浏览器拦截；开发服务器模式没有暴露这个缺口。

## 修改

- `tauri.conf.json` 的 `connect-src` 加入 `ipc:` 与 `http://ipc.localhost`，保留原有最小来源列表。
- 同步 `docs/architecture.md`，明确生产 IPC 是受限 CSP 的允许来源。

## 验证

- 完整 NSIS 重新构建成功：`Assembly Video Agent_0.1.1_x64-setup.exe`，275,047,509 字节（262.3 MiB），SHA-256 `7e46c90d60633f67205428937fce65a6158b4c4424614c768589404044a3295c`。
- 完整封装后的 Release 程序运行 `npm run tauri:verify`：真实项目初始化、素材页切换、设置弹窗均通过，运行时错误为 0。
- 真实 Release IPC 返回剪映适配器与 Tesseract 均为 `ok`。
- 同一新安装产物的 FFmpeg 540×960 H.264、无系统 Python 的草稿 SDK、无系统 Tesseract 的英文数据验证均通过。
