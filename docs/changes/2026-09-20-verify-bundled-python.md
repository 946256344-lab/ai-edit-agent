# 2026-09-20：NSIS 安装包与无系统 Python 验证

## 触发范围

- `scripts/run-tauri.mjs`
- `scripts/verify-packaged-python.ps1`
- `scripts/verify-release-python-app.mjs`
- `src-tauri/src/jianying.rs`
- `src-tauri/tauri.python.conf.json`
- `package.json`
- `README.md`

## 结果

在 D 盘打出 NSIS 安装包：

`src-tauri/target/release/bundle/nsis/Assembly Video Agent_0.1.1_x64-setup.exe`（约 155.7 MB；相对仅 FFmpeg 的 126.3 MB，增加随包 Python/草稿 SDK）

Tauri 多次 `--config` 会**覆盖** `bundle.resources` 数组，不会拼接。构建脚本改为先合并 `tauri.conf.json` 的基础资源（适配器脚本、模型小文件）与 FFmpeg/Python 清单，再只传一份 `src-tauri/target/tauri.merged-resources.conf.json`。Python 目录用 `resources/python` 整树拷贝，避免 glob 匹配不到 gitignored 文件。

随包解释器位于 `target/release/resources/python/python.exe`。去掉系统 PATH 中的 python/py 后：

1. 随包 `python.exe` 为 3.12.10，可 `import pyJianYingDraft, pycapcut, pymediainfo`；PATH 加上随包 FFmpeg 目录后 `shutil.which('ffmpeg')` 指向安装产物。
2. 启动该 release 可执行文件（PATH 仍不含系统 Python/FFmpeg），`get_release_readiness` 中 `ffmpeg`/`ffprobe`/`jianying_adapter` 均为 `ok`。`overall` 仍为 `degraded`（模型未连接、剪映草稿目录等其他项，与本次无关）。

Release 下适配器脚本在 exe 旁的 `scripts/`，不在 `resources/`；`jianying_adapter_script` 改为与媒体工具相同的多路径查找。本机未在无 PATH 条件下再新建一条真实剪映草稿（需要已安装剪映且有已生成时间线）。

## 同步文档

- `README.md`
- `TASKS.md`
- `docs/decisions.md`
- `docs/changes/2026-09-20-bundle-python.md`

## 验证

- `npm run tauri:build -- -b nsis`
- `npm run python:verify`
- `npm run ffmpeg:verify`
- `node scripts/verify-release-ffmpeg-app.mjs`
- `node scripts/verify-release-python-app.mjs`（release 进程 + CDP 9222，PATH 无 python/py/ffmpeg）
