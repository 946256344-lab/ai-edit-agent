//! 当前同步 Agent 调用的截止时间：下层 HTTP、子进程和等待共享剩余预算。
//! 后台分析不继承本轮预算；显式派生的同步工作线程须传入同一截止时间。

use std::{
    cell::Cell,
    time::{Duration, Instant},
};

pub(crate) const EXCEEDED: &str = "native_tool_loop_deadline_exceeded";

thread_local! {
    static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

pub(crate) struct DeadlineScope(Option<Instant>);

impl DeadlineScope {
    pub(crate) fn enter(deadline: Option<Instant>) -> Self {
        Self(DEADLINE.replace(deadline))
    }
}

impl Drop for DeadlineScope {
    fn drop(&mut self) {
        DEADLINE.set(self.0);
    }
}

pub(crate) fn current() -> Option<Instant> {
    DEADLINE.get()
}

pub(crate) fn check() -> Result<(), String> {
    timeout(Duration::MAX).map(|_| ())
}

pub(crate) fn timeout(requested: Duration) -> Result<Duration, String> {
    match current() {
        Some(deadline) => deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .map(|remaining| remaining.min(requested))
            .ok_or_else(|| EXCEEDED.to_owned()),
        None => Ok(requested),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_deadline_caps_work_and_restores_outer_scope() {
        let _outer = DeadlineScope::enter(Some(Instant::now() + Duration::from_secs(1)));
        assert!(timeout(Duration::from_secs(120)).unwrap() <= Duration::from_secs(1));
        {
            let _expired = DeadlineScope::enter(Some(Instant::now()));
            assert_eq!(check().unwrap_err(), EXCEEDED);
        }
        assert!(check().is_ok());
        assert!(std::thread::spawn(|| current().is_none()).join().unwrap());
    }
}
