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

`npm run lint`、`npx tsc -b --pretty false`、`cargo check --manifest-path src-tauri/Cargo.toml`、`npm run harness:check`、`git diff --check` 通过。独立工作树的开发版已启动；界面加载旧本机项目和原有剪辑会话。用户提供的真实桌面截图显示账号已登录、邮箱已验证、资格为“试用中”，有效期为 2026-10-02 17:31:35。退出登录、重启后刷新令牌恢复以及更名安装包尚未验证。当前状态为部分已验证。
