# 2026-09-18：确认剪映草稿可用性

## 结果

本机可以新建剪映草稿。Python / pyJianYingDraft / FFmpeg / 草稿库齐全。用当天 `123` 项目已生成时间线写出并注册了 `123-0c9499f5`。逐字字幕相邻 1ms 重叠会触发剪映同轨冲突，适配器改为只收这一格边界，真重叠仍失败。

## 范围

- `src-tauri/scripts/create_jianying_draft.py`：同轨字幕 1ms 边界收齐
- `src-tauri/scripts/test_create_jianying_draft.py`：该边界的回归
- 本机核验：适配器单测、HandoffPlan 单测、当天时间线实写草稿

## 禁止变化

- 不覆盖已有剪映草稿
- 不从剪映回读
- 不把旁白写入剪映草稿
- 不实现 CapCut

## 验证

- `py -3 -m unittest test_create_jianying_draft.py`（18 passed）
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- handoff jianying`（12 passed）
- 当天时间线 10 镜 / 122 条字幕写出 `D:\JianyingPro Drafts\123-0c9499f5`，注册状态 `registered`
