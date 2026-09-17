# 细拍默认一镜，关配音不写屏幕字

分支：`feature/agent-led-storyboard`（PR3）。

## 结果

Phase 1 按语义拆成约 2–3 秒一拍，时长可以波动。不再用 8 秒旁白或最少拍数把结构打回。Phase 3 默认一镜；两条不相似且对得上才加第 2 镜，不拿第二镜填时长。关配音默认不写 `onScreenText`。

## 范围

- `src-tauri/src/storyboard/phases.rs`：拆拍与选片提示词
- `src-tauri/src/storyboard.rs`：去掉 P1/P5 的 8 秒和最少拍数硬门；空标记不再失败
- `src-tauri/src/agentloop/tools.rs`：`generate_storyboard` 说明改为一拍一镜

## 禁止变化

- 不改公开 Tauri 命令、40% 上限、相似硬拒
- 不在一拍里机械补第 2 镜
