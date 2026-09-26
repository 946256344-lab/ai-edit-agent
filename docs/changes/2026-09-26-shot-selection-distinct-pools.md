# 选镜：召回按拍区分，已用片段不再进入后续拍

## 现象

审片「测试1」最新预览（9 拍英文旁白，库约 95 条工业素材）：

- 「Robotic arms move with pinpoint accuracy」配了展会机械臂（描述写明 trade show with onlookers），库里有白色机械臂搬箱子且在该拍候选内。
- 「Every day, precision meets power」配了池内画质分最低（2.9/10）的一条。
- 9 拍候选几乎相同：同一批 5–6 条出现在每拍前九，总分都在 56–64 之间。

## 根因

来自 debug 选镜轨迹 `src-tauri/target/storyboard-pool-trace.jsonl`：

- 词面匹配用子串且不去虚词：`at` 命中 operate、`in` 命中 machine，`matchedKeywords` 里大量 at/in/or/the，描述越长分越高。
- Phase 1 给每拍的 `visualKeywords` 都带 factory floor / machinery / operating，区分词只占一小部分。
- CLIP 原始余弦挤在 0.17–0.33，直接 ×25 后候选之间只差 1–3 分，画面相似度几乎不起作用。
- Phase 3 按拍串行，提示词写了「不得选与已选相似的候选」，但已选片段仍在后续拍的候选里。第二拍重复选中第一拍的片段，`similar_used_segment` 同时点名两个镜头，修复轮两拍都重选；白色机械臂此时已被后面一拍用掉，只剩展会镜头和低画质镜头。

## 触发范围

- `src-tauri/src/storyboard/scoring.rs`：词面查询去英文虚词与泛化描述词；英文词只按词首命中（worker 命中 workers，不再 at 命中 operate）；出现在至少一半拍（且 ≥3 拍）查询里的词按 0.25 权重计入命中率（`shared_lexical_terms`）；CLIP 分按本拍全库余弦中位数到最高值相对换算，全库差距小于 0.01 时仍按原始值。
- `src-tauri/src/storyboard/phases.rs`：Phase 3 进入选片前由 Rust 从池中剔除与已选镜头交叠/相似、或素材复用已到上限的候选（`pool_excluding_used`），模型返回序号再映射回原池；全被剔除时沿用原池交校验兜底。修复轮先把所有未点名拍的保留镜头计入已选。`similar_used_segment` 只点名靠后镜头，`asset_over_diversity_limit` 只点名超出上限的靠后镜头。局部重选同样过滤。
- `src-tauri/src/storyboard/local_edit.rs`：局部重选召回传入整条故事版的共用词。

## 改动

公开命令、工具与持久化不变。候选池与 `matchedKeywords` 的内容会变化。CLIP 相对换算后最高候选总分普遍上升，`PHASE2_LOW_MATCH_THRESHOLD`（30）的弱匹配扩池此前已基本不触发，本次未调整。

未做：视觉分析标记画面文字/品牌/人群/展会（需重跑视觉分析，另记 `TASKS.md`）。「exacting standards」上半白墙经查是原片构图（回流焊机白色护罩占画面上约三分之一），16:9 裁 9:16 已用满高度，不是裁切问题。

## 同步文档

`docs/architecture.md`（Storyboard 五阶段 Phase 2/3）、`TASKS.md`。

## 验证

`cargo test --lib storyboard::` 145 条通过，新增回归 `phase3_pool_hides_candidates_already_used_by_earlier_beats`、`lexical_match_ignores_function_words_and_substrings`。用上次轨迹的 9 拍候选离线重排（只有池内 9 条，CLIP 用池内最小/最大近似）：robotic arms 与 tandem 首位变为白色机械臂，not just manufacturing 变为航拍厂区，raw to finished 变为传送带；绿色机器特写仍在数拍前列，由 Phase 3 过滤保证只用一次。待桌面同一素材库重跑一条确认。

## 决策

无。
