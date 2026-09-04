# 2026-09-04: storyboard 完成后统一自动配音

## 问题

`key_message` 路径不走 audio-first，Agent `generate_storyboard` 出时间线后直接预览，成片无声；前端成果区另有自动配音，两套行为不一致。

## 改动

- 新增 `voice_provider::auto_synthesize_storyboard_voiceover`：有 `narrationText`、Provider 已配置、且时间线尚无旁白轨时合成；已有轨 / 无旁白则跳过。
- Agent `generate_storyboard` 在创建时间线后、预览前调用同一 helper；失败只写入提示，不挡预览。
- `synthesize_storyboard_voiceover` 命令改走同一 helper；已有旁白轨返回软成功。
- `full_script` audio-first 仍只在 Phase1 后锁时长；其后自动配音会因已有轨而跳过。

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml voice_provider::`
- `npm run harness:check`
