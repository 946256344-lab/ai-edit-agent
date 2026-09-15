# 2026-09-15: 硬切片段内用运动能量收缩可用窗

分支：`cursor/motion-energy-trim`（基于 `cursor/hard-cut-segments`）。

## 结果

素材切段仍只认已验证硬切。每个硬切片段额外写一条帧差运动能量曲线，只砍掉明显静止的开头和已经收住的结尾。对比不够、手持/流水线或抽帧失败时保持硬切两端。Phase 4 锁定片段时用该可用区间，曲线拿不准才标 `uncertain`。

## 范围

- `src-tauri/src/assets/motion.rs`：灰度帧差 SAD、保守 trim、单测
- `src-tauri/src/assets/segments.rs`：`analysisVersion=4`，分析后挂曲线
- `src-tauri/src/assets/analysis.rs`：v3 只补曲线不重切、不改视觉
- `src-tauri/src/storyboard.rs`、`phase4.rs`：候选源范围与精修窗用可用区间
- 素材详情展示并播放可用窗
- 同步 `docs/architecture.md`、`docs/api.md`、`docs/decisions.md`、`TASKS.md`

## 禁止变化

- 不改硬切规则，不用运动曲线发明新切点
- 不改 Phase 2/3 选镜 JSON 与两次视觉识别 JSON
- 不上光流、姿态或 YOLO
- 不把尾部未收敛当成 completion gap 逼 Agent 重跑分镜

## 契约

- `TechnicalMetadata.analysisVersion`：4=硬切 + 运动可用窗（3=已验证硬切、无曲线）
- `SceneSegment.motionProfile`：能量样本与 `usableStartMs`/`usableEndMs`/`tailSettled`/`uncertain`
- `get_asset_evidence.segments[]` 增加可选 `usableStartMs`/`usableEndMs`/`motionTailSettled`
- 公开 Tauri 命令不变；`segmentPending` 仍统计低于当前分析版本的就绪视频
