# 长期文档历史补充

原实现与验证记录按当时文档保留，不代表当前契约。

## 原长期文档补充：2026-09-17：运行时下载本地选镜模型

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

安装包不再捆绑 BGE/CLIP 的三个 `model.onnx`。新增：

| 命令 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `get_runtime_model_status` | 无 | `RuntimeModelStatus` | 三项权重状态与总体 `idle/pending/downloading/ready/failed`；不写副作用。 |
| `start_runtime_model_download` | 无 | `RuntimeModelStatus` | 幂等启动后台下载到 `app_data/runtime-models/`；已在下或已齐则返回当前状态。官方失败后换国内镜像，断点续传并自动重试。 |

事件 `runtime-model-progress` 推送同结构进度。`initialize_local_store` 在缺权重时自动开下，不阻塞。路径解析优先 `app_data`，其次安装包/开发目录。完整安装包可用 `npm run tauri:build:full` 捆绑 ONNX。见 `docs/changes/2026-09-19-runtime-model-download-resilience.md`。
