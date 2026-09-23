//! Windows 外部进程统一入口：隐藏控制台窗口，并为可控调用提供超时与回收。
//! 业务模块不得自行创建 Command，以免重新引入可见窗口或无限等待。
//! FFmpeg/FFprobe/Python 优先安装包资源，其次环境变量，最后 PATH；不在此捆绑 Tesseract。

use std::{
    ffi::{OsStr, OsString},
    io::{self, Read},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::{mpsc, OnceLock},
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

static FFMPEG: OnceLock<PathBuf> = OnceLock::new();
static FFPROBE: OnceLock<PathBuf> = OnceLock::new();
static PYTHON: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HiddenCommandError {
    TimedOut,
    Failed,
}

/// Limits how much of a large MP4/MOV FFmpeg reads before `-i`.
/// DJI files on network shares otherwise scan the whole container during probe.
pub(crate) fn media_open_args() -> [&'static str; 4] {
    ["-probesize", "32M", "-analyzeduration", "10M"]
}

fn media_tool_file_name(name: &str) -> Option<&'static str> {
    match name {
        "ffmpeg" => Some("ffmpeg.exe"),
        "ffprobe" => Some("ffprobe.exe"),
        _ => None,
    }
}

fn env_override(name: &str) -> Option<PathBuf> {
    let key = match name {
        "ffmpeg" => "FFMPEG_PATH",
        "ffprobe" => "FFPROBE_PATH",
        _ => return None,
    };
    let path = PathBuf::from(std::env::var_os(key)?);
    path.is_file().then_some(path)
}

fn bundled_media_candidates(app: Option<&AppHandle>, file_name: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(app) = app {
        if let Ok(dir) = app.path().resource_dir() {
            paths.push(dir.join("resources").join("ffmpeg").join(file_name));
            paths.push(dir.join("ffmpeg").join(file_name));
            paths.push(dir.join(file_name));
        }
    }
    #[cfg(debug_assertions)]
    {
        paths.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("ffmpeg")
                .join(file_name),
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            paths.push(parent.join(file_name));
            paths.push(parent.join("resources").join("ffmpeg").join(file_name));
        }
    }
    paths
}

fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.is_file())
}

fn slot_for(name: &str) -> Option<&'static OnceLock<PathBuf>> {
    match name {
        "ffmpeg" => Some(&FFMPEG),
        "ffprobe" => Some(&FFPROBE),
        _ => None,
    }
}

fn python_env_override() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("PYTHON_PATH")?);
    path.is_file().then_some(path)
}

fn bundled_python_candidates(app: Option<&AppHandle>) -> Vec<PathBuf> {
    let file_name = "python.exe";
    let mut paths = Vec::new();
    if let Some(app) = app {
        if let Ok(dir) = app.path().resource_dir() {
            paths.push(dir.join("resources").join("python").join(file_name));
            paths.push(dir.join("python").join(file_name));
            paths.push(dir.join(file_name));
        }
    }
    #[cfg(debug_assertions)]
    {
        paths.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("python")
                .join(file_name),
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            paths.push(parent.join("resources").join("python").join(file_name));
            paths.push(parent.join("python").join(file_name));
            paths.push(parent.join(file_name));
        }
    }
    paths
}

fn resolved_media_path(name: &str) -> Option<PathBuf> {
    let file_name = media_tool_file_name(name)?;
    env_override(name)
        .or_else(|| slot_for(name).and_then(OnceLock::get).cloned())
        .or_else(|| first_existing(bundled_media_candidates(None, file_name)))
}

fn resolved_python_path() -> Option<PathBuf> {
    python_env_override()
        .or_else(|| PYTHON.get().cloned())
        .or_else(|| first_existing(bundled_python_candidates(None)))
}

/// 启动时记住安装包内的 FFmpeg/FFprobe/Python，后续解析不再依赖系统 PATH。
pub(crate) fn install_bundled_media_tools(app: &AppHandle) {
    for name in ["ffmpeg", "ffprobe"] {
        let Some(slot) = slot_for(name) else {
            continue;
        };
        if slot.get().is_some() {
            continue;
        }
        let Some(file_name) = media_tool_file_name(name) else {
            continue;
        };
        if let Some(path) = env_override(name)
            .or_else(|| first_existing(bundled_media_candidates(Some(app), file_name)))
        {
            let _ = slot.set(path);
        }
    }
    if PYTHON.get().is_none() {
        if let Some(path) =
            python_env_override().or_else(|| first_existing(bundled_python_candidates(Some(app))))
        {
            let _ = PYTHON.set(path);
        }
    }
}

/// 剪映适配器用的解释器：随包 `python.exe` 优先，不把 `py -3` 传给 embeddable 解释器。
pub(crate) fn python_program() -> OsString {
    if let Some(path) = resolved_python_path() {
        return path.into_os_string();
    }
    if cfg!(windows) {
        OsString::from("py")
    } else {
        OsString::from("python")
    }
}

/// 适配器子进程需要能直接找到 `ffmpeg` 与 `MediaInfo.dll`，因此把随包目录插到 PATH 前面。
pub(crate) fn apply_bundled_runtime_path(command: &mut Command) {
    let mut prepend = Vec::new();
    if let Some(path) = resolved_media_path("ffmpeg") {
        if let Some(dir) = path.parent() {
            prepend.push(dir.to_path_buf());
        }
    }
    if let Some(path) = resolved_python_path() {
        if let Some(dir) = path.parent() {
            prepend.push(dir.to_path_buf());
        }
    }
    if prepend.is_empty() {
        return;
    }
    let mut entries = prepend;
    if let Some(current) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&current));
    }
    if let Ok(joined) = std::env::join_paths(entries) {
        command.env("PATH", joined);
    }
}

fn resolve_program(program: impl AsRef<OsStr>) -> OsString {
    let program = program.as_ref();
    let Some(name) = program.to_str() else {
        return program.to_os_string();
    };
    let name = name
        .strip_suffix(".exe")
        .unwrap_or(name)
        .to_ascii_lowercase();
    let Some(file_name) = media_tool_file_name(&name) else {
        return program.to_os_string();
    };
    if let Some(path) = env_override(&name) {
        return path.into_os_string();
    }
    if let Some(path) = slot_for(&name).and_then(OnceLock::get) {
        return path.clone().into_os_string();
    }
    if let Some(path) = first_existing(bundled_media_candidates(None, file_name)) {
        return path.into_os_string();
    }
    program.to_os_string()
}

/// Builds an external command that never shows a console window from the GUI app.
pub(crate) fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(resolve_program(program));
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
    fn resolve_program_does_not_rewrite_python_names() {
        assert_eq!(resolve_program("python"), OsString::from("python"));
        assert_eq!(resolve_program("python.exe"), OsString::from("python.exe"));
        assert_eq!(resolve_program("py"), OsString::from("py"));
    }

    #[test]
    fn python_program_never_passes_dash_three() {
        let program = python_program();
        assert_ne!(program, OsString::from("-3"));
        let as_path = PathBuf::from(&program);
        assert_ne!(
            as_path.file_name().and_then(|name| name.to_str()),
            Some("-3")
        );
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
