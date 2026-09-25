# FellowCut 模型网关接入

同步文档：`TASKS.md`、`docs/api.md`。

## 触发范围与改动

- `src-tauri/src/fellowcut_account.rs` 从 Windows 凭据库中的刷新令牌取得短期 Firebase ID token；令牌只在请求内存中使用。
- `src-tauri/src/provider.rs` 新增网关访问模式，沿用现有 Chat Completions 适配。配置网关后，每次模型请求都向网关发送 ID token；登录或网关失败不切换到本机自定义 API 或 OAuth。
- `src-tauri/src/release_readiness.rs` 报告网关配置状态；账号与模型设置界面同步说明。
- 正式构建必须在编译时配置公开的 `FELLOWCUT_GATEWAY_BASE_URL`（形如 `https://<站点>/api/model`），且只能使用 HTTPS；缺失或无效时模型调用失败封闭。开发构建可用同名环境变量指向本机 HTTP 网关，未配置时保留原有开发者模型访问方式。
- 服务端资格与上游模型密钥由独立的网站仓库维护；桌面安装包不包含模型密钥。

## 验证与限制

`cargo check`、前端 lint、TypeScript 编译、harness 与网站网关模拟测试通过。真实账号经已部署网关调用模型、图像批量请求、正式安装包仍待验证；此阶段为 `implemented_unverified`。

## 决策

公开构建只走 FellowCut 网关，失败时不回退到可绕过试用资格的本机 Provider。未新增 ADR。
