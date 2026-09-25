# Phase 3 网格上限跟候选池走

Phase 2 在某拍最高分低于 30 且库内候选超过 9 条时扩池到 12 条。Phase 3 过去的网格上限写死 9 张，扩出来的 3 条只有文字卡、`keyframeGridAttached=false`；而提示词要求先看图，结果这 3 条几乎不会被选中，偏偏扩池只发生在文字标签最不可靠的弱匹配拍。

现在去掉 `PHASE3_MAX_GRID_IMAGES`，Phase 3 为池内每条候选都附网格，安全上限等于 Phase 2 最大池（`PHASE2_EXTENDED_POOL_SIZE` = 12）。提示词改为写明实际附图数与池大小，不再写死 “up to 9”。常规 9 条的拍请求不变；弱匹配拍每次多 3 张 2×2 网格，仍远低于 Phase 4 单批 40 张的上限。

不改候选池构成、评分、选镜解析或公开契约。`cargo check` 通过；待真实弱匹配回合验收扩池候选被选情况。

同步文档：`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`。
