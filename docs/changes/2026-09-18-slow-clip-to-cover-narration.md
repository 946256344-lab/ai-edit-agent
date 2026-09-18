# 2026-09-18：源窗短于口播时放慢镜头

## 结果

Phase 3 选出镜头后，口播/节奏时钟仍是成片时长。源窗不够长时不再换一条更长的片，也不再问用户；保留已选画面，按源窗对成片时长放慢。预览用 FFmpeg `setpts` 拉长时间；剪映草稿把源窗和轨道槽位分开，由 `VideoSegment` 自行计算速度。禁止冻帧、禁止为凑时长拼下一段硬切。

## 范围

- `src-tauri/src/storyboard/length.rs`：短窗改为放慢
- `src-tauri/src/storyboard.rs`、`phases.rs`、`phase4.rs`、`timing.rs`：成片 `durationMs` 与源窗解耦
- `src-tauri/src/preview.rs`、`preview_cache.rs`：短源窗拉长到槽位
- `src-tauri/scripts/create_jianying_draft.py`：源窗短于槽位时变速而不是取更长源范围

## 禁止变化

- 不改公开 Tauri 命令
- 不补第 2 镜、不拆拍、不冻帧
- Phase 4 仍禁止换片，只改切点

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- storyboard::length storyboard::timing preview_tests::render_timeline_clip_slows`
- 真实成片：源窗短于旁白的拍应留下原画面并放慢，而不是换成更长的错片
