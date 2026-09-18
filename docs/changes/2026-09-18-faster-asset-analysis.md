# 2026-09-18: 技术分析少整段解码、超时不整条失败

本机日志显示素材在 `\\Shared-huiquan\...` 共享盘上。短 DJI 4K 默认整段解码，场景检测/FFprobe/缩略图/运动能量频繁超时；缩略图或单帧抽帧一超时就把整条素材标失败。

## 结果

场景检测一律先扫关键帧。全帧补扫只给本机、不超过 60 秒、且关键帧切点 ≤1 的片子；共享盘、长片或关键帧已切开则不再整段解码。全帧超时仍保留关键帧切点。缩略图或单帧抽帧超时跳过该步，FFprobe 仍失败该条。FFmpeg/FFprobe 打开容器时限制 probesize，运动能量采样 1.5–2.5 fps。公开命令不变。

## 范围

- `src-tauri/src/assets/segments.rs`：按关键帧结果、片长和 UNC 路径决定是否全帧；抽帧失败不再 `?` 中断
- `src-tauri/src/assets/analysis.rs`：缩略图超时返回空；阶段耗时日志
- `src-tauri/src/assets/motion.rs`、`src-tauri/src/process.rs`：降采样与 probesize
- 同步 `docs/architecture.md`、`docs/api.md`、`TASKS.md`

## 禁止变化

- 不改硬切 CLIP 验真阈值，不恢复按秒均分
- 不把原始媒体拷到本地
- 不加技术分析 worker、不加视觉 worker
- 不改公开 Tauri 命令或 SQLite schema
