# 2026-09-25：界面改为 Voycut 品牌风格

## 结果

按 Voycut 品牌设计稿调整桌面界面，只改视觉，不改布局结构与功能：

- 配色：主色由深紫 `#5b43b8` 改为靛紫 `#6558f5`，灰阶统一偏冷；新增蓝到紫的品牌渐变变量，只用于标志和会话开场标记。
- 层次：侧栏与工作区共用冷灰画布（`--canvas`），对话区和预览区改为浮在画布上的白色圆角面板；侧栏去掉右侧分割线。
- 按钮：主操作（新建剪辑、导入文件、生成草稿、发送、播放）由黑色改为靛紫实心加柔和投影；次操作为白底细边。停止键保持深色以区别于发送。
- 选中状态：素材库入口、目录、项目列表选中项用淡紫底；素材卡与候选卡选中用紫色描边加淡紫光晕；悬停素材卡轻微上浮。
- 品牌：侧栏字标前加蓝紫播放标，助手头像由遗留的「F」改为同一标志（`BrandMark` 组件，颜色取自设计变量；正式图标源文件到位后替换路径）。窄窗口时侧栏只显示标志。
- 侧栏素材库入口改为两行：名称与「N 个素材」。
- 播放条已播部分改为浅紫、当前镜头为主色，拖动柄为主色。

## 触发范围

- `src/index.css`：颜色、圆角、阴影变量；新增 `--canvas`、品牌色、`--accent-fill`、`--shadow-accent`、`--shadow-button`、`--focus-ring`。
- `src/styles/shell.css`、`conversation.css`、`preview.css`、`assets.css`、`dialogs.css`、`responsive.css`。
- 新增 `src/components/BrandMark.tsx`；`AppSidebar`、`AgentWorkspace` 使用它。
- `src/lib/i18n/zh-CN.ts`、`en.ts`：新增 `sidebar.libraryCount`。

## 禁止变化

- 不改公开 Tauri 命令、前端 bridge、SQLite schema、Agent 工具目录或剪辑、交付行为；不加暗色模式。

## 验证

- `npx tsc -p tsconfig.app.json --noEmit`、`npm run lint`、`npm run i18n:check`、文档同步检查、`git diff --check`：通过。
- 开发版桌面（本机真实数据，WebView2 调试端口截图）核对：有预览的会话、失败回合会话、空会话、素材库、模型设置弹窗，1440×900 与 720×900 两种尺寸。
- 本工作区完整 Rust 调试构建因系统内存配额不足（os error 1453）中断，截图使用主检出已构建的调试版加载本分支前端；本次无 Rust 改动。
