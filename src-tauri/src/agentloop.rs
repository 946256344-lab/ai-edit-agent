//! Native Function Tool 循环的模块入口。
//!
//! 请求策略、会话历史、Schema、工具执行与工具目录各自保留单一事实源；生产对话只
//! 重导出 NativeToolLoop，不存在前置模型 Router 或首工具选择协议。

mod native;
mod native_policy;
mod policy;
mod prompt;
mod schema;
mod skills;
mod tools;

pub(crate) use native::run_native_tool_loop;
