# 2026-09-29：网关默认不按人限额，网关配音改用 ElevenLabs

## 现象

09-29 用户定（`docs/decisions.md`「首发」）：内测不设每人额度，用到开发者上游套餐（Agnes Token Plan）限额为止；配音改用效果更好的 ElevenLabs，Fish Audio 只作可切换备选。上一轮（`2026-09-28-gateway-voice.md`）网关默认按人每日计数、配音只代理 Fish，桌面经网关时按 Fish 的参数计算缓存指纹、按 Fish 的流解析。

## 触发范围

- `src-tauri/src/music_provider.rs` `gateway_voice`：音色列表解析出网关当前服务商（`provider`：`elevenlabs` / `fish`，不带即旧网关，按 Fish 处理；不认识的服务商报 `voice_gateway_upgrade_required`）、模型 `model_id` 与默认音色 `default_voice_id`。合成请求带 `provider`（网关换了服务商时返回 409，桌面按 `voice_gateway_unavailable` 处理，可重试）；ElevenLabs 按直连的 `{ audio_base64, alignment, normalized_alignment }` JSON 解析，Fish 仍复用 `fish_audio::read_timestamp_stream`。
- `src-tauri/src/voice_provider.rs`：`GatewayVoiceTransport` 先取网关音色列表，再按服务商取与直连相同的 Provider 名、模型、音色设置与输出格式；ElevenLabs 默认音色取网关的 `default_voice_id`（缺省 Charlie）。缓存命中除指纹相同外还要求 manifest 的 Provider 名相同，Fish 的缓存不会被当成 ElevenLabs 用；经网关的 ElevenLabs 与直连 ElevenLabs 共用缓存。合成前不再重复请求音色列表。未内置网关（开发构建）的 Fish 优先、ElevenLabs 回退不变。
- 前端无改动：网关明确没有配音能力（`voice_gateway_not_configured`）时仍隐藏配音开关；`voice_gateway_daily_quota`、`provider_gateway_daily_quota` 的提示保留，只在网关启用额度时出现。

## 改动

网站网关（`../website`，分支 `feature/gateway-elevenlabs`，单独提交）：

- 每人每日额度默认关闭：`FELLOWCUT_DAILY_MODEL_REQUESTS` / `FELLOWCUT_DAILY_VOICE_CHARS` 设为正整数才启用对应计数；未设时不读写 Firestore 用量，也不会因记账失败返回 503。上游 429 照旧透传为 `model_rate_limited` / `voice_rate_limited`，桌面按现有退避规则处理并如实提示。Firestore 用量规则保留。
- 配音服务商由 `FELLOWCUT_VOICE_PROVIDER` 选择（默认 `elevenlabs`，可设 `fish`），不自动回退另一家。ElevenLabs 读 `FELLOWCUT_ELEVENLABS_API_KEY`、`FELLOWCUT_ELEVENLABS_MODEL`（默认 `eleven_multilingual_v2`）、`FELLOWCUT_ELEVENLABS_VOICE_ID`（默认 Charlie），请求参数与桌面直连相同；Fish 改读 `FELLOWCUT_FISH_API_KEY`、`FELLOWCUT_FISH_MODEL`。所选服务商没配密钥时返回 503 `voice_not_configured`。
- 不带 `provider` 的旧桌面构建只会解析 Fish 的流：网关用 ElevenLabs 时对它们的合成返回 426 `upgrade_required`。
- 官网去掉每日额度与 00:00 UTC 重置的描述，改为共享套餐可能暂时不可用；隐私政策去掉按 uid 的每日计数，配音子处理方改为 ElevenLabs（Fish Audio 列为备选）。

## 同步文档

`docs/api.md`、`docs/release-checklist.md`（D5、§1.2 额度与限流、附录 A）。

## 验证

- 桌面：`cargo check`、`npm run lint`、`npm run harness:check`、`git diff --check` 通过；`cargo test --lib` 过滤 `voice_provider::` 与 `gateway_voice` 的单测通过（新增：网关音色列表的服务商解析与旧网关兼容；经网关的 ElevenLabs 复用直连缓存、Provider 不同不复用）。
- 网站：`npm run test:gateway` 24 条通过（默认不记账且记账失败不挡请求、上游 429 透传、启用额度时照旧计数与拦截；ElevenLabs 默认、Fish 可切、所选服务商缺 key 不回退、服务商不符 409、旧构建 426、失败不泄露 key）；`npm run build` 通过。

未桌面实测：经网关的 ElevenLabs 配音、词级时间戳字幕、CapCut / 剪映草稿配音轨；ElevenLabs 长旁白（约 5000 字）经 Vercel 函数的响应大小与耗时；网关没配 ElevenLabs key 时开关隐藏。需网关部署后用 Release 包确认。

## 决策

执行 `docs/decisions.md`「首发」09-29 两条（不设每人额度、配音改 ElevenLabs），无新增决策。
