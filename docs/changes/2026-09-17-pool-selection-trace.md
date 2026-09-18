# debug 落盘 P2 九条与 P3 所选

## 结果

debug 构建默认追加写入 `src-tauri/target/storyboard-pool-trace.jsonl`：每个 beat 的 Phase 2 九条候选（asset/segment/分数/证据摘要），以及 Phase 3 实际选中的序号与是否附带网格图。不写路径或图片。用于复测「好片没进池」还是「进池没选」。

## 范围

- `src-tauri/src/storyboard/provider_trace.rs`
- `src-tauri/src/storyboard/phases.rs`

## 禁止变化

- 不改公开 Tauri 命令
- 不改选片硬规则
- release 构建不写该文件
