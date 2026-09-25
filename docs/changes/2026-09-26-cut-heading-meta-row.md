# 2026-09-26：版本切换移到预览面板，标题区只留时长与镜头数

## 结果

- 剪辑标题下原来折成两行（时长镜头数胶囊、「故事版」加版本下拉）。现在标题下只留「18.1 秒 · 7 个镜头」，去掉易与故事版号混淆的时间线版本号（「第 7 版」）。
- 故事版版本下拉移到视频预览面板标题行右侧，淡紫胶囊样式，窄时省略；它决定预览播放的是哪一版，与镜头替换、撤销、重做放在一起。可见的「故事版」字样改为读屏标签与悬停提示；只有一个版本时不显示；替换镜头过程中换成保留时长提示，不允许中途切版本。
- 预览标题旁的状态不再显示「等待开始剪辑」「预览已就绪」（画面本身已说明），只在有信息量时以灰色小字跟在「视频预览」后面：第一版镜头已选好、正在生成预览、剪辑已完成 · 等待预览、草稿待剪映注册、剪映草稿已就绪、编辑器文件已导出。

## 触发范围

- `src/App.tsx`、`src/components/RoughCutPreview.tsx`、`src/components/StoryboardVersionPicker.tsx`、`src/hooks/useArtifactWorkspaceController.ts`（`getDeliveryStatus` 平稳状态返回 null）。
- `src/styles/shell.css`、`src/styles/preview.css`。
- `src/lib/i18n/zh-CN.ts`、`en.ts`：`app.timelineSummary` 去掉版本参数。

## 验证

- `npx tsc -p tsconfig.app.json --noEmit`、`npm run lint`、`npm run i18n:check`：通过；架构预算仍只有改动前已存在的 `App.tsx` useState 超额。
- 真实桌面开发版中查看：待用户确认。
