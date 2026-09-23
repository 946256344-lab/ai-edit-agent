# 安装包捆绑 Tesseract

## 问题

FFmpeg 与 Python 已随安装包，但 OCR 仍依赖用户自行安装 Tesseract。干净 Windows 机器会缺少英文 OCR，而且原发行检查不会暴露这个缺口。

## 修改

- 固定 UB Mannheim Tesseract 5.4.0 Windows 安装器及 SHA-256；构建时用固定并校验的 7-Zip 26.03 解包，不修改系统安装状态。
- 安装包加入 `tesseract.exe`、运行 DLL、Apache-2.0 许可证和 `tessdata/eng.traineddata`；二进制不进 Git。
- `process.rs` 统一解析 `TESSERACT_PATH`、随包/开发资源、Program Files 与 PATH；素材分析不再自己解析程序位置。
- `get_release_readiness` 新增 Tesseract/英文数据硬检查；缺失时整体为 blocked。
- 同步 `docs/api.md` 与发行、架构、集成文档，明确新的发行检查契约。
- 新增 `npm run tesseract:fetch` 和 `npm run tesseract:verify`，正式构建自动准备资源。

## 验证

- `npm run tauri:build -- -b nsis` 成功，NSIS 安装包约 262.3 MB。
- 删除本轮临时产生的系统 Tesseract 后，`npm run tesseract:verify` 仍从 Release 资源读取 5.4.0 和 `eng` 数据。
- Release 应用真实 IPC 返回 `tesseract.status=ok`；随包程序把生成图片识别为 `HELLO 123`。
- 同一新安装产物的 FFmpeg 540×960 H.264 烟雾验证和 Python/草稿 SDK 验证继续通过。
