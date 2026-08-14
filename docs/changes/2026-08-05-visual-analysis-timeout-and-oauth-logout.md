# 视觉分析超时、失败原因与 OAuth 退出

## 触发范围

- `src-tauri/src/store.rs`：`analyze_asset` 视觉分析请求超时与失败原因记录。
- `src-tauri/src/oauth.rs`、`src-tauri/src/lib.rs`：新增 `clear_experimental_openai_oauth` 命令。
- `src/lib/local-store.ts`、`src/App.tsx`、`src/App.css`：证据面板展示视觉分析失败原因，模型弹窗增加退出登录按钮。

## 改动

- 内部 `analyze_visual_frame` 的同步 `ureq` 请求设置 30 秒超时；失败时返回描述性错误，不再因接口不响应而无限阻塞分析线程。
- `extract_visual_evidence` 聚合所有画面失败原因并随 `TechnicalMetadata` 保存为 `visual_analysis_note`；未连接 OAuth 时提示已跳过。`get_asset_evidence` 将其以 `visualAnalysisNote` 返回。
- 技术分析在视觉证据失败或跳过时仍以 `ready` 完成，素材卡在 `analyzing` 的永久阻塞场景被消除。
- 新增 `clear_experimental_openai_oauth` 命令：删除 Windows Credential Manager 中的实验性凭据并重置连接状态。
- 前端模型弹窗在已连接状态下显示退出登录按钮；素材证据面板展示视觉分析失败或跳过的原因。

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/changes/2026-08-05-visual-analysis-timeout-and-oauth-logout.md`

## 验证

- `cargo check`、`cargo test` 通过（13 项；3 项依赖认证实验性 Provider 的集成测试按设计跳过）。
- `npm run lint` 通过。
- `npm run build` 通过。
- `npm run harness:check` 通过。

## 决策

- 新增 ADR-026：视觉分析请求必须带超时并返回失败原因；凭据需要可用的退出登录出口。
