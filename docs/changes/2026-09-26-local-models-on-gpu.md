# 本地选镜模型改用显卡，同一模型串行推理

## 现象

重新识别的分步计时（65 条素材）：核实切点时 CLIP 比对 6–20 张小图要 8–18 秒，合计 124 秒；画面分析写回时的本地向量计算合计 125 秒。单独测同一 CLIP 模型，8 张图只要约 0.18 秒。

## 根因

- fastembed 把一次调用拆成多批，用 rayon 并行跑同一个 ONNX 会话，每批又默认占满全部 CPU 线程；再叠加多个分析线程同时调用，CPU 被严重超额占用。
- 模型只在 CPU 上跑。ONNX Runtime 静态库本身带 DirectML，但 Windows 自带的 DirectML 是 1.8，ONNX Runtime 1.20 在它上面推理 Softmax / LayerNorm 就报错。
- 开发版依赖库不开优化，fastembed 的图片解码缩放慢约 6 倍（实测每张 105–120 毫秒，开优化后 18–33 毫秒），开发版测出的耗时严重失真。
- 另外，并行跑同一个 DirectML 会话会直接访问违规崩溃（实测复现）。

## 触发范围

- `src-tauri/src/onnx_device.rs`（新）：按完整路径载入随包 `DirectML.dll`；载入成功才对模型建显卡会话并试算，失败回退 CPU，日志写 `Local model <名称>: DirectML GPU / CPU`。`sequential_batches` 让同一模型串行推理、每次只交一批。
- `src-tauri/src/storyboard/clip.rs`、`semantic.rs`：CLIP 图像、CLIP 文字、bge 三个模型经上述加载与串行推理。
- `src-tauri/build.rs`：`DirectML.dll` 改为延迟加载（`/DELAYLOAD`），进程启动不再绑定系统旧版。
- `src-tauri/Cargo.toml`：ort 打开 `directml` 功能；开发版依赖库 `opt-level = 3`。
- `scripts/fetch-directml.ps1`（新）、`npm run directml:fetch`、`scripts/run-tauri.mjs`：从 nuget.org 拉取 `Microsoft.AI.DirectML` 1.15.4，校验包与 DLL 的 SHA-256 及微软签名；开发启动缺失时自动拉取（失败只提示、回退 CPU），安装包构建缺失则中止并通过 `tauri.directml.conf.json` 打包。`src-tauri/resources/directml/NOTICE.md` 记录来源与许可，DLL 不进 git。

## 改动

公开命令与 schema 不变。安装包增加约 18 MB。没有可用显卡、DirectML 缺失或试算失败时行为与原来一致（CPU）。

## 实测（本机 RTX 3070，CLIP 图像，4 线程并发各 3 次 × 20 张）

| 构建 | 显卡 | CPU |
|---|---|---|
| 开发版未优化依赖 | 25.3 秒 | 29.0 秒 |
| 优化 | 4.4 秒（约 18 毫秒/张） | 8.0 秒（约 33 毫秒/张） |

只算模型本身（ort 直接调用，8 张一批）：显卡约 24 毫秒，CPU 约 175 毫秒。

## 同步文档

`docs/api.md`、`docs/decisions.md`、`docs/codebase/STACK.md`、`docs/codebase/STRUCTURE.md`、`docs/release-checklist.md`、`TASKS.md`。

## 验证

`cargo test --lib` 439 条通过（含新增 `sequential_batches_hands_one_batch_per_call_in_order`）；构建后确认 exe 以延迟导入方式引用 `DirectML.dll`。待桌面重启开发版后看日志设备行，并重新识别对比分步计时。
