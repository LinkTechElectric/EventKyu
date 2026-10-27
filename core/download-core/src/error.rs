use crate::JobState;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// Errors produced while validating input, resolving media, or running a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    /// The URL failed scheme, length, host, or control-character validation.
    InvalidUrl(String),
    /// The output name is empty, absolute, or contains forbidden components.
    InvalidOutputName(String),
    /// The normalized output path escapes the normalized working directory.
    OutputOutsideWorkDir {
        /// Normalized working directory.
        work_dir: PathBuf,
        /// Resolved output path.
        output: PathBuf,
    },
    /// A cookie browser name is outside the supported enumeration.
    InvalidBrowser(String),
    /// A job state transition is not allowed.
    InvalidState {
        /// State in which the operation was attempted.
        state: JobState,
        /// Operation that was rejected.
        action: &'static str,
    },
    /// A progress value is malformed or outside its valid range.
    InvalidProgress(String),
    /// A resolver failed to produce a plan.
    Resolver(String),
    /// A resolver produced an invalid or unsafe plan.
    InvalidPlan(String),
    /// A plugin with the same identifier is already registered.
    PluginAlreadyRegistered(String),
    /// No plugin with the requested identifier is registered.
    PluginNotFound(String),
    /// A plugin did not return within its allotted time.
    PluginTimeout {
        /// Identifier of the timed-out plugin.
        plugin_id: String,
        /// Timeout applied to the plugin call.
        timeout: Duration,
    },
    /// The injected process runner failed to execute a plan.
    Process(String),
    /// The download process exited unsuccessfully.
    ProcessFailed(i32),
    /// A filesystem operation required for path validation failed.
    Io(String),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(message) => write!(f, "invalid URL: {message}"),
            Self::InvalidOutputName(message) => write!(f, "invalid output name: {message}"),
            Self::OutputOutsideWorkDir { work_dir, output } => write!(
                f,
                "output path '{}' escapes working directory '{}'",
                output.display(),
                work_dir.display()
            ),
            Self::InvalidBrowser(browser) => write!(f, "unsupported cookie browser: {browser}"),
            Self::InvalidState { state, action } => {
                write!(f, "cannot {action} while job is {state:?}")
            }
            Self::InvalidProgress(message) => write!(f, "invalid progress: {message}"),
            Self::Resolver(message) => write!(f, "resolver failed: {message}"),
            Self::InvalidPlan(message) => write!(f, "invalid resolve plan: {message}"),
            Self::PluginAlreadyRegistered(id) => write!(f, "plugin already registered: {id}"),
            Self::PluginNotFound(id) => write!(f, "plugin not found: {id}"),
            Self::PluginTimeout { plugin_id, timeout } => write!(
                f,
                "plugin '{plugin_id}' timed out after {} ms",
                timeout.as_millis()
            ),
            Self::Process(message) => write!(f, "process execution failed: {message}"),
            Self::ProcessFailed(code) => write!(f, "download process exited with code {code}"),
            Self::Io(message) => write!(f, "filesystem operation failed: {message}"),
        }
    }
}

impl Error for DownloadError {}

/// Result type used by this crate.
pub type DownloadResult<T> = Result<T, DownloadError>;
