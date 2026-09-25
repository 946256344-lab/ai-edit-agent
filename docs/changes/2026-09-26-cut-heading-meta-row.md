# 2026-09-26：剪辑标题下的信息行收成一行

## 结果

- 标题下原来会折成两行：时长镜头数胶囊一行，「故事版」标签加版本下拉一行。现在两者并排在同一行，窄时版本下拉先省略。
- 胶囊去掉时间线版本号（「第 7 版」），只留时长与镜头数，避免和故事版版本号（v6）同时出现造成混淆。
- 版本下拉去掉可见的「故事版」文字，改为读屏标签与悬停提示（「故事版 v6（改自 v5，第 2 拍）」）；只有一个版本时不显示下拉。

## 触发范围

- `src/App.tsx`、`src/components/StoryboardVersionPicker.tsx`、`src/styles/shell.css`。
- `src/lib/i18n/zh-CN.ts`、`en.ts`：`app.timelineSummary` 去掉版本参数。

## 验证

- `npx tsc -p tsconfig.app.json --noEmit`、`npm run lint`、`npm run i18n:check`：通过。
- 真实桌面开发版中查看：见 `TASKS.md` 对应条目。
