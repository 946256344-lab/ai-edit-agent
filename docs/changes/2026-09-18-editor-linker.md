# 2026-09-18：剪映改为编辑器链接器

## 结果

内部时间线仍是事实源。交付前先投影为与编辑器无关的 `HandoffPlan`（源窗、槽位、裁剪、文本、音乐、旁白）。剪映是当前唯一链接器：只新建草稿、不覆盖、不反向同步。CapCut / FCPXML / OTIO 只登记能力，未实现。公开命令仍是 `create_jianying_draft`。

主镜头 JSON 补上 `sourceEndMs`，与 Python 适配器的源窗变速一致。旁白轨进入计划但不写入剪映草稿，避免改变现有交付。

## 范围

- `src-tauri/src/handoff.rs`：计划、能力表、剪映适配器输入
- `src-tauri/src/jianying.rs`：解析素材后消费 `HandoffPlan`
- `AGENTS.md`、`docs/architecture.md`、`docs/decisions.md`、`docs/api.md`、`docs/codebase/`

## 禁止变化

- 不改公开 Tauri 命令名
- 不替换内部时间线
- 不实现 CapCut / FCPXML / OTIO 写出
- 不从剪映回读用户编辑

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- handoff jianying`
- `npm run harness:check`
