//! Native Function Tool 循环的模块入口。
//!
//! 请求策略、会话历史、Schema、工具执行与工具目录各自保留单一事实源；生产对话只
//! 重导出 NativeToolLoop；完成条件检查不选择首工具，不能以模型声明证明写入。

mod auto_graphics;
mod auto_music;
mod context;
mod continuation;
mod facts;
mod logs;
mod native;
mod native_policy;
mod policy;
mod prompt;
mod schema;
mod skills;
mod snapshot;
mod tools;
mod trace;

pub(crate) use native::run_native_tool_loop;
