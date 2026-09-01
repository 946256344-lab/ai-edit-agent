# 2026-09-01: storyboard 完成后自动合成配音与字幕

## 问题

直连前端流程（`generate_storyboard` → `create_timeline_draft` → `render_preview`）生成的成片既没有配音也没有字幕：

- `create_timeline_draft` 创建 timeline 时 `voiceover_tracks` 恒为空，配音只能由 Agent 显式调用 `synthesize_voiceover` 工具生成，前端直连路径从不触发。
- 字幕轨道只从 shot 的 `onScreenText` 生成，模型漏写该字段（如 agnes-2.5-flash 只写 `narrationText`）时整条字幕轨道为空。

## 修复方案

- 新增公开 Tauri 命令 `synthesize_storyboard_voiceover`（`voice_provider.rs`）：从 timeline 反查其 storyboard，取全部 shot 的 `narrationText` 合成整段配音，按 ElevenLabs alignment 生成对齐字幕，写回 timeline 新版本并返回 `VoiceoverApplyResult`。与既有 `synthesize_voiceover_for_timeline` 共用同一 fingerprint 缓存与字幕对齐链路。
- 前端 `createStoryboard` 流程在 `create_timeline_draft` 之后自动调用该命令：成功时用配音后的最新 timeline 版本渲染预览；ElevenLabs 未配置或合成失败时记录警告、跳过配音并继续渲染，不阻塞主流程。
- `normalize_storyboard_candidate`（`storyboard.rs`）新增字幕兜底：shot 的 `onScreenText` 为空时用 `narrationText` 的第一句（最多 40 字符）生成字幕文本。
- 修正预先存在的 Agent 契约断言漂移：`agent_contract_assets.rs` 中工具数量从 24 更新为 25（`retry_failed_asset_analysis` 已加入工具目录但断言未同步）。

## 变更范围

**Rust**：
- `src-tauri/src/voice_provider.rs`（新命令）
- `src-tauri/src/storyboard.rs`（字幕兜底 + 测试）
- `src-tauri/src/lib.rs`（命令注册）
- `src-tauri/tests/agent_contract_assets.rs`（既有断言修正）

**前端**：
- `src/lib/local-store.ts`（`synthesizeStoryboardVoiceover` 封装与 `VoiceoverApplyResult` 类型）
- `src/hooks/useArtifactWorkspaceController.ts`（自动配音接入）

**文档**：
- `docs/api.md`
- 本记录

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml`（含新增字幕兜底测试）
- `npm run lint`、`npm run harness:check`

## 决策

- 自动配音失败不阻塞 storyboard → timeline → preview 主链路（降级为无声预览并提示）。
- 字幕兜底只解决"完全没有字幕"的问题；`onScreenText` 非空时仍优先使用模型字幕。