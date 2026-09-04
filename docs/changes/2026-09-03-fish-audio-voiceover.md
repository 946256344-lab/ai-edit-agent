# 2026-09-03 Fish Audio 配音

## 触发范围

`src-tauri/src/music_provider.rs`、`voice_provider.rs`、`agentloop/{snapshot,tools}.rs`、`lib.rs`、`src/{components/ProviderSettingsModal.tsx,hooks/useProviderController.ts,lib/local-store.ts}`、`docs/api.md`、`docs/architecture.md`、本记录。

## 改动

新增 Fish Audio 配音 Provider。API Key 通过设置页保存到 Windows Credential Manager，也可从 `FISH_API_KEY` 一次性导入；配置后配音链路明确优先使用 Fish Audio，请求失败不会静默切换到 ElevenLabs。

合成调用 `s2.1-pro-free` 的 `/v1/tts/stream/with-timestamp`，拼接 SSE 音频分片并把分段时间戳转换为现有字幕 cue，继续复用配音指纹缓存、真实音频时长、时间线写入与预览混音链路。

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `npm run lint`
- `npm run build`
- `npm run harness:check`
