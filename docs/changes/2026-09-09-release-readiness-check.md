# 2026-09-09：发行就绪检查

## 触发范围

- `src-tauri/src/release_readiness.rs`
- `src-tauri/src/jianying.rs`（草稿位置/脚本可用性探测）
- `src-tauri/src/storyboard/semantic.rs`（模型资源是否存在）
- `src-tauri/src/lib.rs`
- `src/lib/local-store.ts`
- `src/components/ReleaseReadinessBanner.tsx`
- `src/App.tsx`、`src/App.css`
- `docs/api.md`
- `TASKS.md`

## 改动

新增 `get_release_readiness`，启动后在本地数据就绪时展示横幅（仅 `degraded`/`blocked`）：

- FFmpeg / FFprobe
- 应用数据目录可写
- 剩余磁盘空间（建议 ≥ 2 GB）
- AI 模型连接（自定义 API 或 ChatGPT OAuth）
- 剪映草稿目录
- Python + 剪映适配器脚本
- 本地语义模型资源

不写库、不探测用户源媒体。凭据失败只给安全提示。

## 同步文档

- `docs/api.md`
- `TASKS.md`
- `docs/changes/2026-09-09-release-readiness-check.md`

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `npm run lint`
- `npm run build`
- `npm run harness:check`（架构预算）

## 决策

无 ADR。缺 FFmpeg/数据目录为 `blocked`；模型未连接、剪映/Python/语义模型缺失为 `warn`，不阻断导入。
