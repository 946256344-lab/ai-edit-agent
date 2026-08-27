# Assembly Video Agent：Agent 入口

本文件是代码修改的最小入口，先读当前任务和所在目录的说明，再按需补充其他文档。

## 开始修改前

1. 阅读 `CONTRIBUTING.md` 和 `TASKS.md` 当前优先级。
2. 改 `src/` 时读 `src/AGENTS.md`，改 `src-tauri/src/` 时读 `src-tauri/src/AGENTS.md`。
3. 用 `docs/codebase/STRUCTURE.md` 定位代码，需要时再读：
   - `docs/architecture.md` 产品链路
   - `docs/api.md` Tauri 命令与工具
   - `docs/decisions.md` 现行决策

## 产品底线

- Windows 本地优先，原始媒体和项目数据留在本机。
- 媒体语义以真实分析和源时间范围为准，不靠文件名猜测。
- 最终导出、覆盖导出或删除项目前需明确确认。
- 凭据走系统凭据库，不写入日志或前端存储；Provider 可替换，失败时不静默切换。
- Jianying 只创建新 draft，不覆盖也不反向同步；模型文本不能当作产物事实。

## 代码原则

- 保持 React 19、TypeScript、Rust 现有风格；`src/App.tsx` 只做组合，状态进 controller，展示进 component。
- 前端 `invoke` 集中在 `src/lib/local-store.ts`，其他 Tauri 能力按目录规则使用。
- Rust 是可信边界，校验作用域和副作用；失败返回真实原因，不用假成功掩盖问题。
- 源码文件顶部保留中文职责导航，注释说明边界和恢复方式即可。
