# FellowCut 模型网关接入

同步文档：`TASKS.md`、`docs/api.md`。

## 触发范围与改动

- `src-tauri/src/fellowcut_account.rs` 从 Windows 凭据库中的刷新令牌取得短期 Firebase ID token；令牌只在请求内存中使用。
- `src-tauri/src/provider.rs` 新增网关访问模式，沿用现有 Chat Completions 适配。配置网关后，每次模型请求都向网关发送 ID token；登录或网关失败不切换到本机自定义 API 或 OAuth。
- `src-tauri/src/release_readiness.rs` 报告网关配置状态；账号与模型设置界面同步说明。
- 正式构建必须在编译时配置公开的 `FELLOWCUT_GATEWAY_BASE_URL`（形如 `https://<站点>/api/model`），且只能使用 HTTPS；缺失或无效时模型调用失败封闭。开发构建可用同名环境变量指向本机 HTTP 网关，未配置时保留原有开发者模型访问方式。
- 服务端资格与上游模型密钥由独立的网站仓库维护；桌面安装包不包含模型密钥。

## 验证与限制

`cargo check`、前端 lint、TypeScript 编译、harness 与网站网关模拟测试通过。2026-09-25 合入 `origin/master` 后，在独立端口启动真实 Tauri 开发版：原有项目和会话可读取，桌面账号显示邮箱已验证、试用中；中英切换后账号弹窗文案跟随语言。真实测试账号从桌面发起只读项目状态请求，经本机临时代理访问受 Vercel 登录保护的预览网关；Native Provider trace 中前两次 Chat Completions 请求均为 HTTP 200，界面收到模型回复。首次 Agent 最终状态为「部分完成」：SQLite 步骤记录显示 `list_assets` 与 `get_asset_health_summary` 成功，`get_library_visual_overview` 因未在执行白名单而记为 `tool_not_allowed`。该工具与 `get_asset_visual_detail` 已在工具目录中，此分支同步补入白名单并增加目录与白名单一致的回归断言；针对性 Rust 测试通过。修复后重跑真实桌面只读请求，经预览网关返回 HTTP 200，界面状态为「完成」并显示模型回复。预览保护无法由公开桌面客户端直接访问；图像批量请求、完整剪辑链路及正式安装包仍待验证。此阶段为部分已验证。

开发版启动检查提示缺少 Tesseract、剪映草稿目录与 pyJianYingDraft/pycapcut；本轮未验证剪映草稿交付。原有项目的素材库侧栏加载完成后显示 95 条，模型回复称 102 条。只读 SQLite 核对表明当前项目可访问资产有 102 条，其中 7 条已标记从素材库移除；素材列表过滤移除标记，而 Agent 健康摘要尚未过滤。模型回复中的总数不等同于当前可见素材数，需另行对齐统计口径。

## 决策

公开构建只走 FellowCut 网关，失败时不回退到可绕过试用资格的本机 Provider。未新增 ADR。
