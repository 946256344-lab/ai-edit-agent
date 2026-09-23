# 恢复发布基线

## 问题

发布审计运行完整 Rust 测试时有两项失败：Phase 4 局部修复测试仍期待源窗长度覆盖成片时长，与当前“源窗不足时慢放并保持口播时钟”的规则冲突；`subtitle_newsbar` 内置半透明背景为 `#AARRGGBB`，文字校验却只接受 `#RRGGBB`。

## 修改

- Phase 4 现有回归改为确认局部修复保持原成片时长，同时继续检查只改目标镜头和 crop focus。
- 前景色与描边仍只接受 `#RRGGBB`；背景色接受 `#RRGGBB` 或内置模板使用的 `#AARRGGBB`。
- 文档同步 harness 自测改用当前公开 API 文件；内部 `agentloop/policy.rs` 不再被已经收窄的公开契约规则误判。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml`：413 个单元测试与 2 个跨模块契约测试通过。
- 随包 Python 3.12：19 个草稿适配器测试通过；移除系统 Python/py 后可导入 `pyJianYingDraft`、`pycapcut` 与 `pymediainfo`。
- `npm run build`、`npm run harness:check`、`npm run harness:test` 通过；lint 保留一个本轮未触及的 `StudioWorkspace.tsx` 常量比较警告。
- 现有 NSIS 安装产物在移除系统 FFmpeg/FFprobe 后完成 540×960 H.264 编码与探测。
