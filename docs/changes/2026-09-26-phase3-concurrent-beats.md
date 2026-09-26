# Phase 3 各拍同时发请求

## 现象

Token Plan key 下「测试1」同稿生成，`generate_storyboard` 共 90 秒，其中 Phase 3 占 35 秒：7 拍逐拍发请求，每拍约 4 秒，最后一拍 12 秒，后一拍必须等前一拍返回。

## 根因

Phase 3 每拍发请求前要把前面拍已选的片段从候选里剔除，并写进提示词，所以只能串行。Token Plan 每分钟 1000 次请求，串行没有必要。

## 触发范围

- `src-tauri/src/storyboard/phases.rs`：`phase3_select` 把所有待选拍同时发出（`select_beats_concurrently`，工作线程继承截止时间）。每拍请求的上下文只含修复轮保留的镜头。返回后按拍序分配：选中候选已被前面拍占用（交叠、相似或素材复用到上限，判定同 `pool_excluding_used`，抽出为 `available_candidate_indexes`）的拍，剔除已用候选后再同时补发，直到没有撞车。池里已无可用候选时不算撞车，沿用原结果交给 `collect_phase3_issues`，保证补发必然收敛。单拍重试逻辑抽为 `select_beat_with_retries`，行为不变，另记每拍耗时日志。
- 每拍请求的图片、候选卡与要求不变（仍是 3 个候选叠一张图）。提示词里的「已选镜头」首次生成时为空，模型不再看到其他拍的实际选择。
- 局部重选 `phase3_select_beats` 仍逐拍，未改。

## 改动

公开命令与 schema 不变。

## 同步文档

`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`TASKS.md`。

## 验证

`cargo check` 通过；`cargo test --lib storyboard::` 147 条通过。待桌面用「测试1」同稿实测 Phase 3 耗时、撞车补发次数和选镜结果。
