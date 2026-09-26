# 素材详情展示整段识别细节

日期：2026-09-26

## 背景

导入时整段识别（见 `2026-09-26-whole-shot-visual-analysis.md`）新增的变化、高光、主体边界、竖屏可裁性等信息只存进 `VisualEvidence.detail`，素材详情面板没有展示。用户重新识别后在界面上看不到这些信息，以为模型没做。

## 改动

- `get_asset_evidence` 早已随片段视觉证据序列化 `detail`，本次只补前端：`local-store.ts` 新增 `ShotDetail` 类型；
- `AssetEvidenceInspector` 在每个片段描述下展示「识别细节」（默认展开，可收起），每项一行：变化与高光带源时间（秒，一位小数），主体位置优先显示左右边界百分比，没有边界时显示五档位置；
- 枚举值（焦点、竖屏裁切、方向、室内外、昼夜、色调、亮度、人数、人群）按界面语言翻译，模型自由文本保持原文；防护装备里的 `none` 不显示，昼夜为 `unknown` 时不显示；
- 文案进 `zh-CN.ts` / `en.ts` 的 `assetDetail.detail`。

## 验证

`npm run lint`、`tsc -b --noEmit`、`npm run i18n:check` 通过；待桌面确认显示效果。

## 同步文档

- docs/api.md
