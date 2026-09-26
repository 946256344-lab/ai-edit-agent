# 2026-09-26：桌面登录框加网站账号页链接

## 现象

登录框只写「注册、验证邮箱和找回密码请在网站完成」，没有链接，新用户找不到网站。

## 触发范围

- `src-tauri/src/fellowcut_account.rs`：账号状态新增 `accountPageUrl`。网关与网站同域（`https://<站点>/api/model`），账号页为站点根下的 `account.html`；未配置网关时为 `null`。
- `src/lib/local-store.ts`：`FellowCutAccountStatus` 增加同名字段。
- `src/hooks/useFellowCutAccountController.ts`：新增 `openAccountPage`，用系统浏览器打开账号页。
- `src/components/FellowCutAccountModal.tsx`：未登录、邮箱未验证、没有试用资格时显示「打开网站账号页」按钮。
- `src/lib/i18n/zh-CN.ts`、`en.ts`：新增 `account.openWebsite`。

## 改动

`get_fellowcut_account_status`、`sign_in_fellowcut`、`sign_out_fellowcut` 的返回值多一个字段；地址不单独配置，跟随编译期的 `FELLOWCUT_GATEWAY_BASE_URL`。

## 同步文档

`docs/api.md`、`docs/release-checklist.md`。

## 验证

`cargo check`、`npx tsc -b`、`npm run lint` 通过。正式包里点击后打开的地址随安装包验收。

## 决策

无。
