# 运行时模型下载：超时、续传、镜像与完整包

## 结果

- `runtime_models`：单次下载超时由 60s 提至约 20 分钟；传输中断保留 `.partial`，按 Range 自动续传，最多 10 次，官方与 `hf-mirror.com` 轮换。
- 未下完不校验、不删半成品；仅完整文件 SHA 失败才清除。
- 提醒条文案说明自动换源/续传；选镜仍可降级使用。
- 完整安装包：`npm run models:fetch` 预拉 ONNX 后 `npm run tauri:build:full`（合并 `tauri.full-models.conf.json`）。默认 `tauri:build` 仍为瘦包。

## 边界

不静默更换模型身份或哈希；镜像只换 CDN 主机。不下载 FFmpeg/Tesseract/Python。不阻塞首次进入工作台。

同步：`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`README.md`、`package.json`、`scripts/run-tauri.mjs`、`scripts/fetch-clip-models.ps1`。
