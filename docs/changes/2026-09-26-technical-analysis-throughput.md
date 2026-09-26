# 技术分析：按核数并行，同素材抽帧合并

## 现象

本地模型改用显卡后，94 条素材的技术分析从 14.3 分钟降到 6.9 分钟，CLIP 与向量计算已不是瓶颈。剩余时间几乎都在 FFmpeg：抽样帧合计 359 秒（每帧中位 0.44 秒）、运动分析 150 秒、切点检测 110 秒、切点核实抽帧 105 秒；且同时只跑 2 条，16 线程机器大部分时间闲着。

## 根因

- `MAX_TECHNICAL_ANALYSIS_WORKERS` 固定为 2，每次领取 4 条。
- 每抽一帧启动一次 FFmpeg，重复打开文件、探测、定位。

## 触发范围

- `src-tauri/src/assets/analysis.rs`、`src-tauri/src/assets.rs`：并行数改为 `max_technical_analysis_workers()`（逻辑核数一半，2–8，本机 8），每次领取 `analysis_claim_batch()`（并行数两倍）。
- `src-tauri/src/assets/segments.rs`：新增 `extract_frames`，同一条素材的切点核实前后帧、各段抽样帧各合并为每进程最多 12 帧的 FFmpeg 调用（每帧一个 `-ss` 快速定位输入，各输出一帧）；整批失败或缺帧逐帧补抽。

## 改动

公开命令与 schema 不变，抽出的帧与逐帧抽取一致。

## 实测

18.mp4 抽 7 帧：逐帧 3.1 秒，合并 1.6 秒，逐像素平均差 0。

## 同步文档

`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`TASKS.md`。

## 验证

`cargo test --lib` 439 条通过（worker 上限测试改为按实际上限）。待桌面重新识别对比总耗时与分步计时。
