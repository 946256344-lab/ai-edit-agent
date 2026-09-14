# Windows 原生毛玻璃

## 范围

- 延续 codex/lavender-workspace 的已确认布局，仅调整窗口材质及中性色。
- tauri.conf.json 开启透明窗口、浅色原生主题和 Acrylic；保留系统标题栏、窗口按钮与尺寸约束，不新增依赖或业务命令。
- HTML/body 透明；移除模拟桌面的紫色渐变与外围留白。侧栏和顶部为低不透明度白色，内容层为 87.5% 白色，弹窗保持实色以便阅读。
- 不修改媒体、项目持久化、剪辑或导出逻辑；原有本地模型文件改动保留。

## 验证

- 前端 lint/build、架构预算、分支策略及 diff 空白检查通过；lint 保留 StudioWorkspace 的原有 const-comparisons 警告。
- Windows 11 build 22621 原生 debug 构建通过；保留既有 Rust dead_code 警告。
- computer-use 查看真实 Acrylic 窗口，确认背景透蓝、移动后透色变化、原生最大化与还原；没有修改桌面背景或系统透明效果设置。
- 真实 Tauri WebView 烟雾通过，无运行时错误；1024×650 内容无横向溢出，输入框底部位于 597px。
- 本轮不改 Rust 源码或公开契约，未新增测试；上轮记录的两项既有 harness 失败未在本轮修复。

## 交付

已同步当前开发目录并重新编译启动 debug 桌面程序。原生材质需新进程加载配置，不是 CSS 热更新即可生效；未构建安装包、未提交或推送。
