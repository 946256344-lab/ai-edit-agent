//! Native Function Tool 的项目观察判定。
//!
//! 不再用关键词猜测用户是否在问项目事实；权威状态快照已覆盖高层事实，
//! 细节观察由模型按需调用工具。本模块不持有 Provider、SQLite 或副作用执行能力。

/// 不再强制本轮先观察；保留函数签名供 RunReceipt 字段赋值。
pub(super) fn request_requires_project_observation(_request: &str) -> bool {
    false
}
