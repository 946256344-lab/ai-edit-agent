# 2026-09-26：账号错误改用稳定码，登录框按语言显示

## 现象

英文界面下，登录框的错误提示（如「邮箱或密码不正确」「登录已失效或网络不可用」）仍是 Rust 写死的中文。

## 触发范围

- `src-tauri/src/fellowcut_account.rs`：账号错误改为 `account_*: 中文原文`，码有 `account_credential_store`、`account_service_invalid`、`account_verify_failed`、`account_not_found`、`account_entitlement_unavailable`、`account_missing_credentials`、`account_invalid_credentials`、`account_sign_in_unavailable`、`account_signed_out`、`account_session_expired`、`account_task_interrupted`。网关地址缺失或无效改为 `provider_gateway_not_configured: <英文原文>`（换镜等路径会直接显示 `ModelAccess::resolve` 的错误）。
- `src/lib/account-error.ts`：新纯函数 `describeAccountError`，按码取当前语言文案，未知码显示码之后的原文。
- `src/hooks/useFellowCutAccountController.ts`：四处错误改用它。
- `src/lib/i18n/zh-CN.ts`、`en.ts`：新增 `account.errors`。

## 改动

账号三个命令的错误字符串多了码前缀；返回结构不变。模型请求路径对令牌错误仍统一归为 `provider_gateway_auth`，不受影响。

## 同步文档

`docs/api.md`。

## 验证

`cargo check`、`npx tsc -b`、`npm run lint`、`npm run i18n:check` 通过。英文界面下输错密码、断网刷新状态的显示随安装包验收。

## 决策

无。
