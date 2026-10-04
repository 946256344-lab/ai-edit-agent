# FellowCut 桌面账号与试用资格展示

同步文档：`docs/api.md`、`TASKS.md`。

## 范围

- 在 `codex/fellowcut-auth-trial` 独立工作树修改桌面应用；原 `master` 工作目录保持不动。
- 顶栏增加账号入口，使用网站已注册的邮箱和密码登录，展示邮箱验证状态、试用或已开通资格及试用截止时间。
- 后端调用 Firebase Authentication REST API 登录、刷新令牌和核验邮箱；使用 Firebase ID token 只读 Firestore `entitlements/{uid}`，沿用网站安全规则。桌面不创建或延长资格。
- 仅刷新令牌保存在 Windows 凭据库的新账号条目。密码和 ID token 不写入本地数据库、日志或前端持久化。
- 旧本机项目读取路径不变；未登录仍可打开项目。现有自定义模型 API 配置暂不改动。

## 限制与后续

当前资格仅供界面显示。开发者自己的模型 API Key 不能随公开安装包分发；公开试用前还要建设服务端模型网关，在每次模型请求时验证 Firebase ID token、试用期限及额度。网站仍需部署正式域名，提供注册和邮箱验证入口。

## 验证

`npm run lint`、`npx tsc -b --pretty false`、`cargo check --manifest-path src-tauri/Cargo.toml`、`npm run harness:check`、`git diff --check` 通过。独立工作树的开发版已启动；界面加载旧本机项目和原有剪辑会话。用户提供的真实桌面截图显示账号已登录、邮箱已验证、资格为“试用中”，有效期为 2026-10-02 17:31:35。关闭后重新启动，顶栏恢复显示同一测试账号及旧项目，证明已保存的登录状态可恢复；重启后的资格详情未再次打开核对。退出登录和更名安装包尚未验证。当前状态为部分已验证。

## 原长期文档补充：2026-09-25：Voycut 桌面账号展示

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

新增 `sign_in_fellowcut(email, password)`、`get_fellowcut_account_status()`、`sign_out_fellowcut()`。返回 `{ state, email, entitlement, trialStartedAt, accountPageUrl }`；`accountPageUrl` 由内置网关地址推出网站 `account.html`，未配置网关时为 `null`；`state` 为 `signedOut`、`unverified` 或 `verified`。失败时错误带 `account_*: ` 码前缀，前端按码显示当前语言（见 `docs/changes/2026-09-26-account-error-codes.md`）。账号使用网站同一个 Firebase Authentication 项目；已验证账号只读 Firestore `entitlements/{uid}`，缺失时返回空资格。桌面不创建或续期试用。刷新令牌只存 Windows 凭据库 `AssemblyVideoAgent/fellowcut-firebase-refresh-token`，密码与 ID token 不持久化。公开构建的模型请求通过内置 `FELLOWCUT_GATEWAY_BASE_URL` 指向账号网关，每次刷新 ID token 并由服务端复核资格，并带 `X-Voycut-Version` 请求头；网关失败返回 `provider_gateway_auth`、`provider_gateway_entitlement`、`provider_gateway_payload_too_large`、`provider_gateway_upgrade_required` 码前缀或 `:HTTP {status}` 结尾的英文原文；这些旧内部标识保留以兼容已登录用户。见 `docs/changes/2026-09-25-fellowcut-desktop-account.md`、`docs/changes/2026-09-25-fellowcut-model-gateway.md`。

| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `sign_in_fellowcut` | email, password | FellowCutAccountStatus |
| `get_fellowcut_account_status` | 无 | FellowCutAccountStatus |
| `sign_out_fellowcut` | 无 | FellowCutAccountStatus |
