# 一体化窗口顶栏

按用户截图要求移除突兀的 Windows 系统标题栏，将窗口控制合并到现有工作区顶栏，不新增高度。顶部统一暖白背景，侧栏保留浅灰底色。

- Tauri 设置 decorations=false，增加最小化、最大化切换、关闭和拖动所需窗口权限。
- WorkspaceHeader 展示三枚线性窗口按钮；useWindowController 负责 API 调用及最大化/还原状态，监听随组件卸载释放。
- 顶栏文字、空白和 Assembly 字标支持原生 drag region；控制按钮不属于拖动区。
- 不变更项目、素材、剪辑与持久化流程。

验证：npm run build 通过；npm run lint 仅原有 StudioWorkspace 常量比较警告；git diff --check 通过；Tauri dev 编译成功，桌面进程已运行。React 技能检查确认窗口行为留在 controller，事件监听有清理。agent-browser CLI 不可用，改用内置浏览器确认开发页面正常加载桌面专用提示；当前桌面进程没有可用调试端点，未自动验证原生拖动、最大化/还原、最小化、关闭与边缘缩放，需桌面手动验收。
