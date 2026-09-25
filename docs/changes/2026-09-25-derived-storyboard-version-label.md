# 故事版切换显示派生版来源

## 背景

后端将新增 `reselect_shots` / `refine_shot_ranges`，在已有故事版上局部改镜并生成派生版本（设计见 `docs/changes/2026-09-25-storyboard-edit-primitives.md`）。版本下拉原来只显示 `v5`，看不出它改自哪一版、动了哪几拍。

## 契约

`StoryboardVersion`（`list_storyboard_versions` / `get_storyboard_version` 返回）加两个可选字段：

- `derivedFromVersionId?: string | null`：改自哪个故事版。
- `changedBeatIds?: string[]`：改动的拍 id。

加性变化：旧版本和当前后端不返回这两个字段，缺省或 null 一律按普通版本显示。

## 前端

- 版本下拉移入 `src/components/StoryboardVersionPicker.tsx`，`App.tsx` 只做组合，未新增状态。
- 派生版显示「v5（改自 v4，第 3 拍）」/ "v5 (from v4, beat 3)"：
  - 来源版本号从已加载的版本列表按 `derivedFromVersionId` 查找，找不到时省略来源。
  - 拍号是改动拍 id 在该版本 `beats` 中的 1 起序号，按升序；有 id 找不到或超过 3 拍时只报数量（「改了 4 拍」/ "4 beats changed"）。
  - 来源和拍都无法说明时显示「局部修改」/ "edited"。
- 下拉加最大宽度，长标签截断，完整文字放在 `title`。

## 同步文档

- `docs/api.md`（`StoryboardVersion` 可选字段说明）、`TASKS.md`。

## 验证

`npx tsc -p tsconfig.app.json --noEmit`（本次改动无错误；worktree 未装 `react-markdown` 导致 `MessageMarkdown.tsx` 报找不到模块，与本次无关）、`npm run lint`、`npm run i18n:check`、Agent 契约与文档同步检查、`git diff --check` 通过。`harness:check` 的架构预算项在改动前即失败（`App.tsx` 15 个 `useState` 超过 14），本次未增加，反而把下拉移出了 `App.tsx`。当前后端尚不返回新字段，下拉仍显示 `vN`；派生版显示待后端落地后用真实回合验收。
