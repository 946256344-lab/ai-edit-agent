# 2026-09-25：单镜长短偏好从用户原话透传到 Phase 3

## 结果

用户在对话里说「快节奏 / 多切几个镜头」或「慢一点 / 长镜头 / 别切太碎」时，这句话不再被丢掉。Phase 1 按用户自己的措辞产出 `shotLengthHint`，随叙事结构传到 Phase 3；Phase 3 用它对每拍时长上下界和提示词做区分，而不是对所有请求都套同一句「2-3 秒」和同一个 `clamp(1_200, 6_000)`。

## 范围

- `src-tauri/src/storyboard/phases.rs`：`NarrativeStructure.shotLengthHint`、`ShotLengthHint` 枚举、`normalize_shot_length_hint`、Phase 1 提示词、`RoughStoryboard` 透传、`select_one_beat` 提示词行、`assemble_phase3_selection` 时长钳制
- `src-tauri/src/shot_replacement.rs`：新增字段的构造点补齐

## 禁止变化

- 不改公开 Tauri 命令，不改工具目录
- 不改任何硬约束：候选去重、40% 同素材上限、不相邻相似、重叠禁止、时长上限、音频优先
- `default` 分支逐字等于改造前行为：提示词仍是「Picture shots should last about 2-3 seconds.」，上下界仍是 `(1_200, 6_000)`
- 旧项目与旧 JSON 缺字段时落回 `default`，不报错、不打回重试

## 边界

- 只表达单镜长短这一维。总时长仍由 `targetDurationMs`、配音时钟和现有门禁决定
- `key_message` 与 `full_script` 两种模式下行为一致；配音模式下口播时钟仍是权威，本字段只影响镜头切分粒度
- 模型给出不认识的值（含空串）一律落回 `default`

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml --tests`：通过，警告数与改动前一致
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- storyboard::`：142 通过，0 失败
- `npm run harness:check`：通过，未触发架构文档同步规则

## 未做

- 总时长偏好（`pacing`）暂不实现。`full_script` 下 TTS 对齐是时钟，单独加该字段会变成一个用户以为生效、实际不改总时长的静默失效，比现状更糟
