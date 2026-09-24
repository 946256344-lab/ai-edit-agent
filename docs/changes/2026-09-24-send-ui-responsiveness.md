# 发送时界面响应

发送消息前的任务归属命令会同步请求模型（单次最长 30 秒），Tauri 同步命令运行在主线程，期间桌面窗口可能无响应。对话提交命令也同步执行数据库工作。

两个命令现改为异步入口，并用 `spawn_blocking` 执行原有同步逻辑。任务归属、一次性 receipt 校验和 Agent 后台任务创建逻辑未改。前端在等待任务归属时显示“正在确认剪辑任务…”。

验证：`npm run lint`、`npx tsc -b --pretty false`、`cargo check --manifest-path src-tauri/Cargo.toml`、`npm run harness:check` 通过，开发版窗口已启动。尚未在真实桌面发送测试消息；下次发送需检查窗口滚动、停止按钮、状态提示，以及正常回复或澄清。当前状态为 `implemented_unverified`。
