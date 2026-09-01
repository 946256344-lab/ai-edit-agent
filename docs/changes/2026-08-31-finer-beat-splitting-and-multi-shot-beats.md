# 2026-08-31: 更细的 beat 拆分与基于候选池的单 beat 多镜头

## 问题

当前 storyboard 的镜头数基本被 Phase 1 的 beat 数锁死：Phase 1 对文案拆分偏粗时，后续 Phase 2/3 无法补救，导致成片镜头偏少、叙事节奏过平。同时，`full_script` 的最小时长约束仍然偏粗，中文/英文混排短句与长句的朗读时长没有被统一地纳入 beat 数和镜头数的校验。

此外，Phase 3 请求会把 `RoughStoryboard` 整体序列化（其中带每个 beat 的完整候选池 `StoryboardSource`）并再次单独注入候选池 JSON，候选素材的 visual/ocr 证据被重复携带，请求体过大，容易触发模型侧响应超时。同时 `provider.rs` 把 `into_string` 的一切读取失败（超时/连接中断/响应过大）统一误报为"自定义 API 响应为空"，无法定位真实原因。

## 修复方案

- Phase 1 的 prompt 增加"一个信息点 = 一个 beat"的拆分要求，强调每个 beat 应尽量承载单一具体信息点、动作或情绪转折，并对 verbose brief 提供更细的 beat 粒度建议。
- Phase 1 增加带反馈重试，若 beat 拆分过粗或信息覆盖不足，会将错误原因回传给模型再次修订。
- `storyboard.rs` 新增朗读时长估算器：以英文词元为基准，两个中文字符约等于一个英文词元，用于估算 brief 与 beat narration 的语音时长。
- `full_script` 校验改为同时约束：
  - 总时长不得短于估算朗读时长；
  - beat 数不得少于按"每 8 秒一个 beat"推导出的最低值；
  - 单个 beat 的 narration 不得超过 8 秒的估算上限。
- Phase 2 为每个已覆盖 beat 生成候选池（`RoughStoryboard.candidate_pools`），候选池来自该 beat 的 Top-12 预选集；该字段 `skip_serializing`，只用于内存中的 Phase 3 素材约束，不再随 prompt 序列化。
- `StoryboardShot` 新增子镜头身份字段：`beatPartIndex`（组内 1-based 序号）、`beatPartCount`（该 beat 拆出的子镜头总数）、`splitRole`（lead / bridge / tail）。未拆分的 beat 恒为 1/1/lead。
- Phase 3 允许单个 beat 拆成多个连续 shot，但拆出的每一个 shot 都必须使用**该 beat 自己候选池内**的素材，且同一 beat 内各 shot 素材不得重复；首 shot 必须保留 Phase 2 选择的主素材。
- `enforce_phase3_scope` 与 `normalize_storyboard_candidate` 都会按 beat 分组重新计算 `beatPartIndex` / `beatPartCount` / `splitRole`，保证模型缺字段时数据仍自洽。
- Phase 3 prompt 改为注入精简卡片：每个 beat 仅展示 mainShot + 至多 3 个备选的精简卡片（`assetId`、场景段、少量视觉标签），已选素材也以精简卡片呈现，不再携带完整 visual/ocr 证据，显著压缩请求体。
- `provider.rs` 修正自定义 API 的错误翻译：`into_string` 读取失败透出真实 I/O 错误（区分超时/中断/过大），HTTP 空响应体单独提示，不再统一误报"响应为空"。

## 变更范围

**Rust**：
- `src-tauri/src/storyboard/phases.rs`
- `src-tauri/src/storyboard.rs`
- `src-tauri/src/models.rs`
- `src-tauri/src/provider.rs`
- `src-tauri/src/storyboard/validation.rs`

**文档**：
- `docs/changes/2026-08-31-finer-beat-splitting-and-multi-shot-beats.md`

## 同步文档

- 本记录

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml storyboard::`
- `npm run lint`、`cargo build`、`npm run harness:check`

## 决策

- 无新增 ADR。
