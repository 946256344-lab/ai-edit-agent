# 2026-09-03: 镜头上限 100 + 短 brief 时长收敛 + Phase5 路由

## 问题

- 旧安全上限 30 镜误杀合理密度（如 80s/35 镜），Phase5→Phase4 空转。
- 短目标/提纲被 Phase1 扩成 ~80s `full_script`。
- 结构类校验失败不应回 Phase4（Phase4 不能改镜数）。

## 改动

- `MAX_STORYBOARD_SHOTS` / `MAX_STORYBOARD_BEATS` = **100**。
- Phase1 prompt + `short_brief_duration_issue`：无大段口播稿时偏 `key_message`、目标 ≤45s、beat 宜 3–8（用户明确要更长则放行）。
- Phase5：`normalize` 机械自修后校验；结构/硬上限失败**不**回 Phase4；仅精修类（重叠源范围、时长贴近等）回 Phase4。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml storyboard::`
- `npm run harness:check`
