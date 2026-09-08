# 2026-09-07: 配音 HTTP 走环境代理

## 问题

本机 `HTTPS_PROXY=http://127.0.0.1:7892`，`api.fish.audio` 直连会 Windows 连接超时（os error 10060）。应用配音请求使用裸 `ureq::get/post`，不读环境代理；设置页却可能只显示「密钥已存」，看起来像 Provider 正常。

复现对照：

- Python/`ureq` 走代理：Fish / ElevenLabs 均 200
- 同款 `ureq` 直连：Fish `TRANSPORT` 连接失败；ElevenLabs 多数时候仍可达
- 成片日志因此反复出现 `Fish Audio is unavailable` → 回退 ElevenLabs；若回退当时再遇 401/抖动，整段无声

附加诊断缺口：中文 Windows 超时文案不含英文 `timeout`，被收成笼统 `unavailable`，排障困难。

## 改动

- 新增 `outbound_http.rs`：配音共用 `voice_agent()`，读取 `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`
- Fish / ElevenLabs 请求改走该 Agent；传输失败写真实细节，并把中文 10060 归类为 timed out
- 设置页区分「已连接」与「密钥已存·未探通」

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml chinese_windows_connect_timeout_is_classified_as_timeout`
- 本机探针：直连 Fish 失败、代理 Fish 成功（修复前对照）

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `TASKS.md`
