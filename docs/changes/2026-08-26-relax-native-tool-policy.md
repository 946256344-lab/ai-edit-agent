# 放宽 Native 工具策略：模型选路径，Rust 守执行边界

## 背景

“给我剪辑一个视频”没有命中旧的“生成/创建/制作”动作词，导致 NativeToolLoop 只向模型提供观察工具。模型只能持续搜索素材；后续又重复提交超出 schema 上限的片段搜索参数，最终耗尽步骤且没有创建产物。

## 变更

- 非只读请求默认向模型开放可逆本地能力：素材分析排队、storyboard、内部时间线版本、片段/文本/本地音乐编辑和低清 preview。
- `RequestToolPolicy` 不再用正向动作词决定这些工具是否可见，只处理明确只读、明确禁止和敏感能力授权。
- 外部音乐下载、在线音乐落地、付费配音和 Jianying 交付草稿仍需用户明确请求。
- 工具可见性与完成清单分离。请求中的最低产物期望只用于 `NativeRunReceipt` 验真，不选择首工具，也不隐藏可逆工具；“剪辑一个视频”现在期望真实 storyboard 和时间线收据。
- 相同工具与语义相同的 JSON 参数第一次返回 `invalid_arguments` 后不再真实执行；JSON 会先规范化，空白或键顺序变化不能绕过。第二次返回不可重试诊断，第三次终止本轮；两次拦截仍各写一条 payload-free 失败步骤审计。
- “不要修改任何内容 / do not modify anything / inspect … only / only inspect …”等明确表达纳入只读策略。
- “不要替换片段 / 不要调整片段时长 / 不要重排片段 / 不要替换背景音乐”等逐工具负向约束在发送目录和执行入口同时生效。

## 保持不变的边界

- 模型不能提供项目、任务、本机路径或外部进程参数；Rust 从当前 LoopState 注入并复核作用域。
- strict schema、参数上限、素材证据、版本化写入、SQLite 事务、许可证、确认门和磁盘事实验真保持不变。
- 模型自然语言不能创建 artifact，也不能替代 storyboard、时间线、preview 或 Jianying 的真实收据。
- 最终导出、覆盖和删除边界未改变。

## 回归覆盖

- 原始中文表达“给我剪辑一个视频，体现专业、负责、供应链强大”无需敏感授权即可获得本地主链工具，同时产生 storyboard 与时间线的终态验真期望。
- 普通聊天获得可逆工具但不获得外部下载、配音或 Jianying 工具；明确只读请求只获得观察工具。
- “生成视频”不再隐式授权付费配音；明确配音请求仍可授权。
- `search_asset_segments(limit=30)` 这类完全相同的无效调用只执行一次并在第三次原样调用时停止。

## 同步文档

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/codebase/ARCHITECTURE.md`
- `TASKS.md`
