# 瘦安装包与运行时模型下载

## 结果

- 安装包 `bundle.resources` 去掉 BGE/CLIP 三个 `model.onnx`；tokenizer/config/NOTICE 仍随包。
- 新增 `runtime_models`：启动缺权重时后台从 HuggingFace 下载到 `app_data/runtime-models/`，SHA-256 校验、`.partial` 断点续传；事件 `runtime-model-progress`；命令 `get_runtime_model_status` / `start_runtime_model_download`。
- 模型目录解析优先 `app_data`，再安装包/开发路径；失败缓存可在下载成功后重载。
- 下载不挡工作台；缺失时语义降级词面、CLIP 加权为 0。就绪条展示进度与失败重试。

## 边界

不下载 FFmpeg/Tesseract/Python；不换源；不阻塞首次进入；不从 git 删除开发用 BGE onnx。

同步：`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`docs/roadmap.md`、`docs/codebase/STRUCTURE.md`、`TASKS.md`、`README.md`、`.harness/agent-context.json`（`outbound_http` 纳入 HTTP 边界）。
