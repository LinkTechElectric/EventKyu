//! Direct argv process execution with confinement, streaming, and termination.

use ai_orchestrator::{OrchestratorError, ProcessExit, ProcessRunner, Result, ToolPlan};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// The process output stream associated with one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    /// Standard output, including ffmpeg `-progress pipe:1` updates.
    Stdout,
    /// Standard error, including normal ffmpeg diagnostics.
    Stderr,
}

/// One line observed while a child process is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Origin stream.
    pub stream: OutputStream,
    /// Line content without its trailing line ending.
    pub line: String,
}

/// Non-blocking observer invoked by the runner thread for every output line.
pub type OutputObserver = Arc<dyn Fn(ProcessOutput) + Send + Sync + 'static>;

/// A real `std::process::Command` runner. No shell is involved.
pub struct LocalProcessRunner {
    allowed_root: PathBuf,
    timeout: Duration,
    cancelled: Arc<AtomicBool>,
    observer: Option<OutputObserver>,
}

impl LocalProcessRunner {
    /// Creates a runner whose every step must stay beneath `allowed_root`.
    pub fn new(allowed_root: impl Into<PathBuf>) -> Self {
        Self {
            allowed_root: allowed_root.into(),
            timeout: DEFAULT_TIMEOUT,
            cancelled: Arc::new(AtomicBool::new(false)),
            observer: None,
        }
    }

    /// Overrides the per-process timeout for deterministic termination tests.
    #[cfg(test)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Installs a cooperative cancellation flag checked while polling.
    pub fn with_cancellation(mut self, cancelled: Arc<AtomicBool>) -> Self {
        self.cancelled = cancelled;
        self
    }

    /// Streams output to an application observer in addition to engine logs.
    pub fn with_observer(mut self, observer: OutputObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    fn terminate(&self, child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl ProcessRunner for LocalProcessRunner {
    fn run(
        &mut self,
        plan: &ToolPlan,
        on_stdout: &mut dyn FnMut(&str),
        on_stderr: &mut dyn FnMut(&str),
    ) -> Result<ProcessExit> {
        let workdir = confined_workdir(&self.allowed_root, &plan.workdir)?;
        let mut child = Command::new(&plan.program)
            .args(&plan.args)
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    OrchestratorError::Process(format!(
                        "工具 '{}' 未安装，请通过依赖管理安装",
                        plan.program
                    ))
                } else {
                    OrchestratorError::Process(format!("无法启动工具 '{}'：{error}", plan.program))
                }
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| OrchestratorError::Process("无法打开子进程 stdout 管道".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| OrchestratorError::Process("无法打开子进程 stderr 管道".into()))?;

        let (sender, receiver) = mpsc::channel();
        let stdout_reader = spawn_reader(stdout, OutputStream::Stdout, sender.clone());
        let stderr_reader = spawn_reader(stderr, OutputStream::Stderr, sender.clone());
        drop(sender);

        let started = Instant::now();
        let mut stdout_done = false;
        let mut stderr_done = false;
        let mut exit_status = None;

        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                self.terminate(&mut child);
                return Err(OrchestratorError::Process("执行已取消".into()));
            }
            if started.elapsed() >= self.timeout {
                self.terminate(&mut child);
                return Err(OrchestratorError::Process(format!(
                    "工具 '{}' 执行超过 {} 秒，已终止",
                    plan.program,
                    self.timeout.as_secs()
                )));
            }

            match child.try_wait() {
                Ok(Some(status)) => exit_status = Some(status),
                Ok(None) => {}
                Err(error) => {
                    self.terminate(&mut child);
                    return Err(OrchestratorError::Process(format!(
                        "无法读取子进程状态：{error}"
                    )));
                }
            }

            if exit_status.is_some() && stdout_done && stderr_done {
                break;
            }

            if stdout_done && stderr_done {
                thread::sleep(POLL_INTERVAL);
                continue;
            }

            match receiver.recv_timeout(POLL_INTERVAL) {
                Ok(ReaderEvent::Line(stream, line)) => {
                    if let Some(observer) = &self.observer {
                        observer(ProcessOutput {
                            stream,
                            line: line.clone(),
                        });
                    }
                    match stream {
                        OutputStream::Stdout => on_stdout(&line),
                        OutputStream::Stderr => on_stderr(&line),
                    }
                }
                Ok(ReaderEvent::Eof(stream)) => match stream {
                    OutputStream::Stdout => stdout_done = true,
                    OutputStream::Stderr => stderr_done = true,
                },
                Ok(ReaderEvent::Failed(stream, error)) => {
                    self.terminate(&mut child);
                    return Err(OrchestratorError::Process(format!(
                        "读取子进程 {stream:?} 失败：{error}"
                    )));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    stdout_done = true;
                    stderr_done = true;
                }
            }
        }

        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        let status = exit_status
            .ok_or_else(|| OrchestratorError::Process("子进程结束后未返回退出状态".into()))?;
        Ok(ProcessExit {
            code: status.code().unwrap_or(-1),
        })
    }
}

fn confined_workdir(root: &Path, candidate: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root).map_err(|error| {
        OrchestratorError::Io(format!(
            "无法打开用户工作目录 '{}'：{error}",
            root.display()
        ))
    })?;
    let candidate = fs::canonicalize(candidate).map_err(|error| {
        OrchestratorError::Io(format!(
            "无法打开步骤工作目录 '{}'：{error}",
            candidate.display()
        ))
    })?;
    if !candidate.is_dir() {
        return Err(OrchestratorError::InvalidPath {
            path: candidate.display().to_string(),
            reason: "working directory is not a directory".into(),
        });
    }
    if !candidate.starts_with(&root) {
        return Err(OrchestratorError::PathOutsideWorkdir {
            path: candidate,
            workdir: root,
        });
    }
    Ok(candidate)
}

enum ReaderEvent {
    Line(OutputStream, String),
    Eof(OutputStream),
    Failed(OutputStream, String),
}

fn spawn_reader<R>(
    reader: R,
    stream: OutputStream,
    sender: mpsc::Sender<ReaderEvent>,
) -> JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) => {
                    let _ = sender.send(ReaderEvent::Eof(stream));
                    break;
                }
                Ok(_) => {
                    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
                        bytes.pop();
                    }
                    let line = String::from_utf8_lossy(&bytes).into_owned();
                    if sender.send(ReaderEvent::Line(stream, line)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(ReaderEvent::Failed(stream, error.to_string()));
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "signalfit-process-test-{}-{id}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn plan(program: &str, args: &[&str], workdir: &Path) -> ToolPlan {
        ToolPlan::new(
            program,
            args.iter().map(|arg| (*arg).to_owned()).collect(),
            "runner test",
            workdir,
        )
    }

    #[test]
    fn echo_streams_stdout_and_returns_real_exit_code() {
        let root = TestDir::new();
        let mut runner = LocalProcessRunner::new(&root.0);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = runner
            .run(
                &plan("/bin/echo", &["hello signalfit"], &root.0),
                &mut |line| stdout.push(line.to_owned()),
                &mut |line| stderr.push(line.to_owned()),
            )
            .unwrap();
        assert!(exit.success());
        assert_eq!(stdout, ["hello signalfit"]);
        assert!(stderr.is_empty());
    }

    #[test]
    fn ls_failure_preserves_stderr_and_nonzero_status() {
        let root = TestDir::new();
        let mut runner = LocalProcessRunner::new(&root.0);
        let mut stderr = Vec::new();
        let exit = runner
            .run(
                &plan("/bin/ls", &["definitely-missing"], &root.0),
                &mut |_| {},
                &mut |line| stderr.push(line.to_owned()),
            )
            .unwrap();
        assert!(!exit.success());
        assert!(stderr.join("\n").contains("definitely-missing"));
    }

    #[test]
    fn arguments_are_not_interpreted_by_a_shell() {
        let root = TestDir::new();
        let marker = root.0.join("should-not-exist");
        let injected = format!("; touch {}", marker.display());
        let mut runner = LocalProcessRunner::new(&root.0);
        let mut stdout = Vec::new();
        runner
            .run(
                &plan("/bin/echo", &[&injected], &root.0),
                &mut |line| stdout.push(line.to_owned()),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(stdout, [injected]);
        assert!(!marker.exists());
    }

    #[test]
    fn missing_tool_has_dependency_manager_guidance() {
        let root = TestDir::new();
        let mut runner = LocalProcessRunner::new(&root.0);
        let error = runner
            .run(
                &plan("signalfit-tool-that-does-not-exist", &[], &root.0),
                &mut |_| {},
                &mut |_| {},
            )
            .unwrap_err();
        assert!(error.to_string().contains("未安装，请通过依赖管理安装"));
    }

    #[test]
    fn workdir_cannot_escape_the_selected_root() {
        let root = TestDir::new();
        let mut runner = LocalProcessRunner::new(&root.0);
        let error = runner
            .run(
                &plan("/bin/echo", &["unsafe"], Path::new("/")),
                &mut |_| {},
                &mut |_| {},
            )
            .unwrap_err();
        assert!(matches!(
            error,
            OrchestratorError::PathOutsideWorkdir { .. }
        ));
    }

    #[test]
    fn timeout_kills_a_running_process() {
        let root = TestDir::new();
        let mut runner = LocalProcessRunner::new(&root.0).with_timeout(Duration::from_millis(50));
        let started = Instant::now();
        let error = runner
            .run(
                &plan("/bin/sleep", &["10"], &root.0),
                &mut |_| {},
                &mut |_| {},
            )
            .unwrap_err();
        assert!(error.to_string().contains("已终止"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn cancellation_kills_a_running_process() {
        let root = TestDir::new();
        let cancelled = Arc::new(AtomicBool::new(true));
        let mut runner = LocalProcessRunner::new(&root.0).with_cancellation(cancelled);
        let error = runner
            .run(
                &plan("/bin/sleep", &["10"], &root.0),
                &mut |_| {},
                &mut |_| {},
            )
            .unwrap_err();
        assert!(error.to_string().contains("执行已取消"));
    }
}
