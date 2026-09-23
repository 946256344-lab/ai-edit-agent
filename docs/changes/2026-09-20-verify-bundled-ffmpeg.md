# 2026-09-20：NSIS 安装包与无系统 FFmpeg 验证

## 触发范围

- `scripts/verify-packaged-ffmpeg.ps1`
- `scripts/verify-release-ffmpeg-app.mjs`
- `package.json`
- `README.md`

## 结果

在 D 盘打出 NSIS 安装包：

`src-tauri/target/release/bundle/nsis/Assembly Video Agent_0.1.1_x64-setup.exe`（约 126.3 MB；随包两个静态 exe 经 NSIS 压缩）

随包媒体位于 `target/release/resources/ffmpeg/ffmpeg.exe` 与 `ffprobe.exe`。去掉系统 PATH 中的 WinGet FFmpeg 后：

1. 随包 `ffmpeg`/`ffprobe` 能探测，并渲染 1 秒 540×960 H.264（与 preview 同规格）。
2. 启动该 release 可执行文件（PATH 仍不含系统 FFmpeg），`get_release_readiness` 中 `ffmpeg`/`ffprobe` 均为 `ok`。`overall` 仍为 `degraded`（模型/Python/剪映等其他项，与本次无关）。

本机 release 数据目录当前没有项目，因此没有对真实导入素材再跑一遍 `render_preview`。导入仍走系统文件对话框，未做 GUI 点选。

## 同步文档

- `README.md`
- `TASKS.md`
- `package.json`

## 验证

- `npm run tauri:build -- -b nsis`
- `npm run ffmpeg:verify`
- `node scripts/verify-release-ffmpeg-app.mjs`（release 进程 + CDP 9222）
