use std::{
    ffi::OsStr,
    process::{Child, Command, Output, Stdio},
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

/// Runs a hidden command with a hard deadline. On Windows it asks `taskkill /T
/// /F` to terminate the child tree, then reaps the direct child when it exits
/// within a short cleanup window. If termination cannot be requested or
/// confirmed, this returns without an unbounded wait and cannot guarantee that
/// the process tree has exited.
pub(crate) fn run_hidden_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<Output, HiddenCommandError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| HiddenCommandError::Failed)?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|_| HiddenCommandError::Failed)
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

#[cfg(test)]
mod tests {
    use super::*;

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
