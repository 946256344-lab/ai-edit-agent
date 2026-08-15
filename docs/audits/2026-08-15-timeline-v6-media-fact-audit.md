# Timeline v6 只读媒体事实审计

**日期**：2026-08-15  
**审计人**：Claude Code（只读，未修改任何数据）  
**项目**：`fe63c655`（891 条素材项目）  
**Timeline v6 ID**：`ef596160-c5cc-456b-a893-fe215bd58923`  
**约束**：不重新分析素材、不修改 timeline、不生成 preview、不创建 Jianying draft、不导出

---

## 1. Timeline v6 基本事实

| 字段 | 值 |
|---|---|
| version_number | 6 |
| status | `preview_ready` |
| storyboard_version_id | `fb8348d3` |
| 片段数 | 8 |
| 文本轨 | 0 |
| 音乐轨 | 0 |
| 总时长 | 31,689 ms（31.689 s）|

---

## 2. v5 → v6 变更核实

仅 shot2 发生变化，与 TASKS.md 记录完全一致：

| | v5 | v6 |
|---|---|---|
| shot2 source_start_ms | 250 | 250 |
| shot2 source_end_ms | 2900 | 2750 |
| shot2 timeline_dur | 3000 ms | 2500 ms |
| 总时长 | 32,189 ms | 31,689 ms |

`change_clip_duration` 正确将 `source_end = source_start + new_duration = 250 + 2500 = 2750`，shot2 是唯一 `src_dur == tl_dur`（均为 2500 ms）的片段。

---

## 3. 片段逐一核查

### 3.1 素材状态

全部 8 个 asset 均满足以下条件，无异常：

| shot | asset（前8位） | display_name | analysis_status | source_health | excluded |
|---|---|---|---|---|---|
| 1 | 89283dbc | 2026_05_20_16_41_32_IMG_1430.MOV | ready | online | 0 |
| 2 | 37013203 | 2026_05_20_16_41_35_IMG_1432.MOV | ready | online | 0 |
| 3 | 1748b118 | 2026_05_20_16_41_36_IMG_1433.MOV | ready | online | 0 |
| 4 | 0cc27838 | DJI_20000206171646_0021_D.MP4 | ready | online | 0 |
| 5 | 2f8fab50 | DJI_20000206171724_0022_D.MP4 | ready | online | 0 |
| 6 | c80c398d | DJI_20000206180042_0030_D.MP4 | ready | online | 0 |
| 7 | 4295897c | DJI_20000206201301_0059_D.MP4 | ready | online | 0 |
| 8 | cde3efa2 | DJI_20000206203609_0072_D.MP4 | ready | online | 0 |

### 3.2 源范围边界核查

所有源时间点均在 asset 已分析时长之内，无越界：

| shot | source_start | source_end | tl_dur | asset_dur | src_end ≤ asset_dur | src_start ≥ 0 |
|---|---|---|---|---|---|---|
| 1 | 961 ms | 1822 ms | 3000 ms | 1922 ms | ✅ | ✅ |
| 2 | 250 ms | 2750 ms | 2500 ms | 2900 ms | ✅ | ✅ |
| 3 | 250 ms | 2967 ms | 3000 ms | 2967 ms | ✅ | ✅ |
| 4 | 0 ms | 1518 ms | 3036 ms | 3036 ms | ✅ | ✅ |
| 5 | 0 ms | 1518 ms | 3036 ms | 3036 ms | ✅ | ✅ |
| 6 | 0 ms | 1518 ms | 3036 ms | 3036 ms | ✅ | ✅ |
| 7 | 0 ms | 2453 ms | 4905 ms | 4905 ms | ✅ | ✅ |
| 8 | 0 ms | 4588 ms | 9176 ms | 9176 ms | ✅ | ✅ |

### 3.3 Timeline 连续性

全部8片段首尾相接，无间隙、无重叠，从 0 ms 到 31,689 ms 连续。

---

## 4. 设计层面发现（系统性，非 v6 特有）

### 4.1 `source_end_ms` 在 preview 渲染中未被执行

`preview.rs::render_timeline_clip` 的 FFmpeg 命令为：

```
ffmpeg -ss <source_start_ms/1000> -i <source_file> -t <timeline_dur/1000> ...
```

`source_end_ms` **不传给 FFmpeg**。实际渲染逻辑是：
- 从 `source_start_ms` 开始 seek
- 读取 `timeline_dur`（即 `tl_end - tl_start`）长度的内容

这意味着7/8片段的实际 preview 渲染与 `source_end_ms` 存储值不一致：

| shot | FFmpeg 实际渲染范围 | source_end 存储值 | 差值 |
|---|---|---|---|
| 1 | 961 ms 起，最多 3000 ms（受 asset 限制仅 ~961 ms 可用） | 1822 ms | —— |
| 2 | 250 ms 起，2500 ms | 2750 ms | **一致**（change_clip_duration 保证） |
| 3 | 250 ms 起，最多 3000 ms（受 asset 限制仅 2717 ms 可用） | 2967 ms | —— |
| 4 | 0 ms 起，3036 ms（= 完整 asset） | 1518 ms | 多渲染 1518 ms |
| 5 | 0 ms 起，3036 ms（= 完整 asset） | 1518 ms | 多渲染 1518 ms |
| 6 | 0 ms 起，3036 ms（= 完整 asset） | 1518 ms | 多渲染 1518 ms |
| 7 | 0 ms 起，4905 ms（= 完整 asset） | 2453 ms | 多渲染 2452 ms |
| 8 | 0 ms 起，9176 ms（= 完整 asset） | 4588 ms | 多渲染 4588 ms |

### 4.2 根因溯源

`create_timeline_draft` 从 storyboard 创建 timeline 时，timeline slot 用 `shot.duration_ms`（storyboard 期望时长），source 范围保留 storyboard 的证据采样区间（通常是 asset 中的某个分析子段）。这两个字段从设计上就可能不相等；原意是 storyboard `duration_ms` 描述"故事节奏"，而 source 范围描述"有信息量的证据片段"。

`change_clip_duration` 在用户主动调整时会同步 `source_end = source_start + new_duration`（shot2 正确），但对其余片段无影响。

### 4.3 对实际 preview 的影响

- **shot1**（`IMG_1430.MOV`）：从 961 ms seek，仅有 ~961 ms 素材（asset 总长1922 ms），但时间槽为 3000 ms。FFmpeg 会在 EOF 处停止，实际 preview 该片段比时间槽短。已知问题，不影响时间线元数据完整性。
- **shot3**（`IMG_1433.MOV`）：从 250 ms seek，仅有 2717 ms 素材，时间槽 3000 ms。同上，preview 该片段比时间槽短 283 ms。
- **shots 4–8**：source_end 被忽略，渲染内容长度 = 完整 asset 时长 = timeline slot，preview 可正常渲染。storyboard 的证据子范围未被 preview 强制执行，但不导致截断。

---

## 5. 审计结论

| 维度 | 结论 |
|---|---|
| v5 → v6 变更内容 | ✅ 与 TASKS.md 记录完全一致，仅 shot2 缩短 500 ms |
| 全部 asset 可用性 | ✅ 8/8 ready、online、未 excluded |
| 源时间点边界 | ✅ 全部 source_start ≥ 0，source_end ≤ asset_dur |
| Timeline 连续性 | ✅ 无间隙无重叠 |
| source_end 执行 | ⚠️ preview.rs 未传 source_end 给 FFmpeg；shot1/shot3 会渲染不足，shots 4–8 的 source_end 约束静默失效 |
| 影响范围 | 这是贯穿 v1–v6 的系统性问题，非 v6 引入；不影响 timeline 元数据完整性，影响 preview 与未来交付的精确性 |

---

## 6. 建议后续动作（需独立任务处理）

1. **`preview.rs::render_timeline_clip`** 应使用 `min(source_end_ms, source_start_ms + timeline_dur)` 计算实际 `-t`，确保不超出 `source_end_ms`。
2. **`create_timeline_draft`** 创建 timeline 时，考虑将 `timeline_slot_dur` 对齐到 `source_end - source_start`，或增加校验门拒绝 `tl_dur > src_dur + tolerance`。
3. shot1 和 shot3 的实际可用素材短于 timeline slot，需重新选镜或接受截断。

> 以上建议不在本次审计任务范围内，不得在本分支实施。
