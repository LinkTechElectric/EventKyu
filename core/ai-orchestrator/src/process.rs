use crate::{Result, ToolPlan};

/// Exit status returned by an application-provided process runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExit {
    /// Process exit code; zero indicates success.
    pub code: i32,
}

impl ProcessExit {
    /// Returns whether this exit code represents success.
    pub fn success(self) -> bool {
        self.code == 0
    }
}

/// Application boundary for direct, argv-based local process execution.
///
/// Implementations must invoke [`ToolPlan::program`] with [`ToolPlan::args`]
/// directly (for example through `std::process::Command`), set its current
/// directory to [`ToolPlan::workdir`], and never route the command through a
/// shell. Standard output and standard error must be forwarded independently
/// as lines so ffmpeg progress and diagnostics cannot be confused.
pub trait ProcessRunner {
    /// Runs one validated tool plan and returns its exit status.
    fn run(
        &mut self,
        plan: &ToolPlan,
        on_stdout: &mut dyn FnMut(&str),
        on_stderr: &mut dyn FnMut(&str),
    ) -> Result<ProcessExit>;
}
