use crate::{DownloadPlan, DownloadResult};

/// Exit status returned by an injected process runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExit {
    /// Process exit code; zero indicates success.
    pub code: i32,
}

impl ProcessExit {
    /// Returns whether the exit code indicates success.
    pub fn success(self) -> bool {
        self.code == 0
    }
}

/// Application-provided external process execution boundary.
///
/// Implementations must invoke [`DownloadPlan::program`] with
/// [`DownloadPlan::args`] directly, without a shell. Output lines should be
/// forwarded to `on_line`; yt-dlp progress lines are parsed by the engine.
/// No production implementation is included in this crate.
pub trait ProcessRunner {
    /// Executes a download plan and returns its exit status.
    fn run(
        &mut self,
        plan: &DownloadPlan,
        on_line: &mut dyn FnMut(&str),
    ) -> DownloadResult<ProcessExit>;
}
