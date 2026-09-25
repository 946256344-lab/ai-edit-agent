# 发送失败显示真实原因

## 现象

发送「用 产品素材替换第 4个镜头」后只看到「无法准备当前剪辑任务，请重试或重新选择项目。」，控制台里的真实错误是自定义 API 读取响应超时（`os error 10060`）。用户无从判断是网络、配置还是项目问题。

## 根因

`src/App.tsx` 的 `sendMessage` 失败分支默认给笼统文案，只特判凭据、OAuth、项目缺失等几个英文子串；Provider 超时、连接失败和 HTTP 拒绝全部落进笼统句子。

## 触发范围

- `src-tauri/src/provider.rs`：新增 `with_model_failure_code`，给错误加上 `classify_model_request_failure` 已有的稳定码前缀（`provider_timeout` / `provider_network` / `provider_http_{status}` / `provider_empty_response` / `provider_unknown`）。
- `src-tauri/src/taskrouter.rs`：任务归属的两次模型请求失败时带上该前缀。其他调用方的错误文本不变。
- `src/lib/send-error.ts`：新纯函数 `describeSendError`，先认稳定码，没有码的旧错误按 Rust 传输错误原文兜底；`App.tsx` 只调用它，不新增状态。
- `src/lib/i18n/zh-CN.ts`、`en.ts`：新增超时、连不上、凭据被拒（401/403）、限流（429）、服务端错误（5xx）、请求被拒（其他 HTTP）、空响应和「笼统句 + 原因」文案。

## 改动

- 超时 / 连不上 / 请求被拒 / 兜底提示附带原因摘录；凭据被拒、限流、空响应只给固定说明。
- 摘录先脱敏：URL 只保留协议、主机、端口和路径（去掉查询串、片段和账号密码），`Bearer`、`sk-` 类密钥与 `api_key=`/`token=` 等值替换为 `[redacted]`，本机路径只留文件名，最长 160 字符。
- 公开命令签名不变；`resolve_conversation_task` 的错误字符串多了 `code: ` 前缀。

## 同步文档

`TASKS.md`。

## 验证

`cargo check`、`npm run lint`、`npx tsc --noEmit -p tsconfig.app.json`、`npm run i18n:check` 通过；用样例错误跑过 `describeSendError`：10060 超时（带码和不带码）、401、带查询串与账号的 404、带密钥和本机路径的兜底都输出预期文案且无敏感值。待桌面重建后用断网或错误地址实测。

## 决策

无。
