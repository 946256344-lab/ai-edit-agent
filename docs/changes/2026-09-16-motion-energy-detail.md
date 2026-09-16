# 2026-09-16: 素材详情展示运动能量曲线

分支：`cursor/motion-energy-detail`。

## 结果

素材详情每个硬切片段展示帧差运动能量曲线；有可用窗时用色带标出。没有曲线的旧分段显示占位，不发明数据。点击曲线可把原片预览定位到对应源时间。

## 范围

- `get_asset_evidence.segments[]` 增加 `motionEnergy[]`、`motionUncertain`
- `AssetEvidenceInspector` 绘制曲线
- 同步 `docs/api.md`、`docs/architecture.md`、`TASKS.md`

## 禁止变化

- 不改切段、运动 trim 或 Phase 4 算法
- 不把能量曲线写入选镜 JSON

## 契约

- `AssetEvidenceSegment.motionEnergy`：`{ timeMs, energy }[]`，无曲线时省略
- `AssetEvidenceSegment.motionUncertain`：仅在曲线边界不确定时出现
