//! Windows 外部进程统一入口：隐藏控制台窗口，并为可控调用提供超时与回收。
//! 业务模块不得自行创建 Command，以免重新引入可见窗口或无限等待。

use std::{
    ffi::OsStr,
    io::{self, Read},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HiddenCommandError {
    TimedOut,
    Failed,
}

/// Builds an external command that never shows a console window from the GUI app.
pub(crate) fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // Console-based media tools must not create a visible window from the GUI app.
        command.creation_flags(0x0800_0000);
    }
    command
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if Instant::now() >= deadline => return false,
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(_) => return false,
        }
    }
}

#[cfg(windows)]
fn terminate_child(child: &mut Child) -> bool {
    let mut taskkill = hidden_command("taskkill");
    taskkill
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let Ok(mut taskkill) = taskkill.spawn() else {
        return false;
    };
    if !wait_for_exit(&mut taskkill, Duration::from_secs(2)) {
        return false;
    }
    matches!(taskkill.try_wait(), Ok(Some(status)) if status.success())
}

#[cfg(not(windows))]
fn terminate_child(child: &mut Child) -> bool {
    child.kill().is_ok()
}

fn terminate_and_reap(child: &mut Child) {
    // Never wait indefinitely after a failed termination request. On Windows,
    // taskkill targets descendants as well as the direct child.
    let _ = terminate_child(child);
    let _ = wait_for_exit(child, Duration::from_secs(2));
}

// FFmpeg 的输出可能填满管道；必须在等待退出的同时读取 stdout/stderr。
fn drain_pipe(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<io::Result<Vec<u8>>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe.read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    receiver
}

/// Runs a hidden command with a hard deadline. On Windows it asks `taskkill /T
/// /F` to terminate the child tree, then reaps the direct child when it exits
/// within a short cleanup window. If termination cannot be requested or
/// confirmed, this returns without an unbounded wait and cannot guarantee that
/// the process tree has exited.
pub(crate) fn run_hidden_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<Output, HiddenCommandError> {
    let timeout =
        crate::execution_deadline::timeout(timeout).map_err(|_| HiddenCommandError::TimedOut)?;
    let deadline = Instant::now() + timeout;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| HiddenCommandError::Failed)?;
    let stdout = drain_pipe(child.stdout.take().unwrap());
    let stderr = drain_pipe(child.stderr.take().unwrap());
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let mut output = child
                    .wait_with_output()
                    .map_err(|_| HiddenCommandError::Failed)?;
                output.stdout = stdout
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| HiddenCommandError::TimedOut)?
                    .map_err(|_| HiddenCommandError::Failed)?;
                output.stderr = stderr
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| HiddenCommandError::TimedOut)?
                    .map_err(|_| HiddenCommandError::Failed)?;
                return Ok(output);
            }
            Ok(None) if Instant::now() >= deadline => {
                terminate_and_reap(&mut child);
                return Err(HiddenCommandError::TimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(_) => {
                terminate_and_reap(&mut child);
                return Err(HiddenCommandError::Failed);
            }
        }
    }
}

/// 探测本机程序是否能在超时内成功启动并退出（用于发行就绪检查，不解析输出正文）。
pub(crate) fn program_responds(
    program: impl AsRef<OsStr>,
    args: &[&str],
    timeout: Duration,
) -> bool {
    let mut command = hidden_command(program);
    command
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    matches!(
        run_hidden_command_with_timeout(&mut command, timeout),
        Ok(output) if output.status.success()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_command_drains_both_pipes_before_waiting_for_exit() {
        let mut command = if cfg!(windows) {
            let mut command = hidden_command("powershell");
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Console]::Out.Write('x' * 1048576); [Console]::Error.Write('e' * 1048576)",
            ]);
            command
        } else {
            let mut command = hidden_command("sh");
            command.args([
                "-c",
                "head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2",
            ]);
            command
        };
        let output = run_hidden_command_with_timeout(&mut command, Duration::from_secs(5))
            .expect("large FFmpeg-style output must not block child exit");
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 1048576);
        assert_eq!(output.stderr.len(), 1048576);
    }

    #[test]
    fn hidden_command_timeout_reaps_the_child() {
        let mut command = if cfg!(windows) {
            let mut command = hidden_command("cmd");
            command.args(["/C", "ping 127.0.0.1 -n 5 >NUL"]);
            command
        } else {
            let mut command = hidden_command("sh");
            command.args(["-c", "sleep 5"]);
            command
        };

        assert!(matches!(
            run_hidden_command_with_timeout(&mut command, Duration::from_millis(50)),
            Err(HiddenCommandError::TimedOut)
        ));
    }
}
