use crate::PlanState;
use std::path::PathBuf;
use thiserror::Error;

/// Errors produced by planning, validation, approval, or local execution.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    /// The requested executable is outside the fixed allowlist.
    #[error("program '{0}' is not in the tool allowlist")]
    ProgramNotAllowed(String),

    /// A multi-step plan contains no steps.
    #[error("an execution plan must contain at least one step")]
    EmptyPlan,

    /// A plan is structurally invalid.
    #[error("invalid tool plan: {0}")]
    InvalidPlan(String),

    /// A path contains traversal, control characters, or another unsafe form.
    #[error("unsafe path '{path}': {reason}")]
    InvalidPath {
        /// Display form of the rejected path.
        path: String,
        /// Reason the path was rejected.
        reason: String,
    },

    /// A normalized path resolves outside the approved working directory.
    #[error("path '{}' escapes working directory '{}'", path.display(), workdir.display())]
    PathOutsideWorkdir {
        /// Normalized candidate path.
        path: PathBuf,
        /// Normalized working directory.
        workdir: PathBuf,
    },

    /// A session operation is incompatible with its current state.
    #[error("cannot {action} while plan is {state:?}")]
    InvalidState {
        /// Current session state.
        state: PlanState,
        /// Rejected operation.
        action: &'static str,
    },

    /// The process runner could not start or communicate with a process.
    #[error("process execution failed: {0}")]
    Process(String),

    /// A local tool exited unsuccessfully.
    #[error("tool exited with code {code}; stderr tail:\n{stderr_tail}")]
    ProcessFailed {
        /// Process exit code.
        code: i32,
        /// Last stderr lines retained for diagnosis.
        stderr_tail: String,
    },

    /// An ffmpeg progress value could not be parsed.
    #[error("invalid ffmpeg progress: {0}")]
    InvalidProgress(String),

    /// The LLM request failed at the application-provided boundary.
    #[error("LLM completion failed: {0}")]
    Llm(String),

    /// The LLM response was not a JSON execution plan of the expected shape.
    #[error("invalid LLM plan JSON: {0}")]
    InvalidPlanJson(String),

    /// Supplied ffprobe metadata was not valid JSON.
    #[error("invalid ffprobe metadata JSON: {0}")]
    InvalidMetadataJson(String),

    /// A retry limit of zero was requested.
    #[error("max_attempts must be at least 1")]
    InvalidRetryLimit,

    /// A preset parameter is outside its supported range.
    #[error("invalid preset parameter: {0}")]
    InvalidPreset(String),

    /// A filesystem operation needed for safe path normalization failed.
    #[error("filesystem validation failed: {0}")]
    Io(String),

    /// Serializing an internal prompt or plan failed.
    #[error("JSON serialization failed: {0}")]
    Serialization(String),
}

/// Result type used throughout this crate.
pub type Result<T> = std::result::Result<T, OrchestratorError>;
