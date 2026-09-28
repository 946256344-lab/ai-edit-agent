# 2026-09-28：配音经 Voycut 网关，网关每日额度提示

## 现象

10-01 内测不设邀请名单、资格整个内测期有效（`docs/decisions.md`「首发」D1、D3、D5）。内测用户没有自己的配音密钥，配音要用开发者的 Fish Audio 密钥，且密钥不能进安装包。网站网关同时加了每人每日额度，超限返回 429 `daily_quota_exceeded`；桌面原来把网关 429 当限流按 `Retry-After` 退避重发，提示也只是笼统的限流。

## 触发范围

- `src-tauri/src/music_provider.rs` 新增 `gateway_voice` 子模块（与 `fish_audio` 并列，出站 HTTP 按 harness 规则只放在该文件）：构建内置网关时，配音经 `https://<站点>/api/voice/voices` 与 `/api/voice/tts`，每次刷新 Voycut ID token 并带 `X-Voycut-Version`。网关原样转发 Fish 带时间戳的 SSE 流，解析复用 `music_provider::fish_audio::read_timestamp_stream`。失败带 `voice_gateway_*` 码：`auth`、`entitlement`、`upgrade_required`、`daily_quota`、`not_configured`（旧网关 404 或服务端未配配音密钥）、`rejected`、`unavailable`（唯一可重试）。新增命令 `get_voice_availability`。
- `src-tauri/src/voice_provider.rs`：内置网关时只用 `GatewayVoiceTransport`，不读本机配音密钥、不回退 ElevenLabs。Provider 名、模型 `s2.1-pro-free`、音色设置与输出格式与 Fish 直连相同，生成指纹不变，已缓存的配音、alignment 字幕和草稿配音轨照旧复用。未内置网关（开发构建）保持原来的 Fish 优先、ElevenLabs 回退。
- `src-tauri/src/agentloop/skills.rs`：`voice_gateway_*` 映射成 Agent 失败上下文（事实 + 恢复说明，不再提示去设置页填密钥）；audio-first 失败 `storyboard_voiceover_failed` 附带网关原因。
- `src-tauri/src/provider.rs`：网关 429 且正文为 `daily_quota_exceeded` 时直接返回 `provider_gateway_daily_quota`，不按 `Retry-After` 重发；`is_final_model_failure` 已把 `provider_gateway_*` 视为最终失败，Phase 3 / 4 也不再重试。其他 429 照旧退避。
- 前端：`src/lib/send-error.ts` 把 `provider_gateway_daily_quota` 映射为 `app.errors.gatewayDailyQuota`（中英文，写明 UTC 0 点恢复）。`useComposerMediaController` 启动时与回到窗口时（至少间隔 5 分钟）调用 `get_voice_availability`；网关明确没有配音能力时隐藏输入框下的配音开关，并按配音关闭发送（字幕随之关闭），草稿里的原选择保留。登录失效、网络失败等暂时原因不隐藏，由真实请求给出具体原因。正式版设置弹窗隐藏 Fish Audio / ElevenLabs 自带密钥区块，说明改为配音同样经 Voycut 服务。

## 改动

网站网关（`../website`，单独提交）：`FELLOWCUT_BETA_MODE=1` 时 trial 不按 7 天过期；每人每日额度按 UTC 日记在 Firestore `usage/{uid}/days/{date}`，模型按请求数、配音按字符数；配音代理 `/api/voice/*` 与模型网关同样校验登录、资格、总开关、最低版本与额度，密钥只在服务端环境变量。

## 同步文档

`docs/api.md`、`docs/release-checklist.md`（D5、§1.2、附录 A）。

## 验证

`cargo check`、`npx tsc -b`、`npm run lint`、`npm run i18n:check` 通过；`music_provider::gateway_voice` 状态码映射与 `provider` 最终失败单测。网站：网关单测 20 条、Firestore 规则模拟器测试 5 条通过，并用模拟器验证真实 REST 记账（超限、跨日恢复、他人令牌被拒）。

未桌面实测：登录后经网关配音、词级时间戳与字幕、CapCut / 剪映草稿配音轨；网关未配配音密钥时开关隐藏；额度用尽时的中英文提示。需网关部署后用 Release 包确认。

## 决策

执行 D5。开发构建内置网关时同样只走网关，设置页的自带密钥区块仍显示但不生效。
