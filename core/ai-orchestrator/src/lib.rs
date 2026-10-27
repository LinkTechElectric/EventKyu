#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Offline AI orchestration for SignalFit media tools.
//!
//! The crate turns natural-language requests into declarative [`ToolPlan`]
//! values, validates every executable and path, requires explicit user
//! approval, and delegates argv-based process execution to the application.
//! Media metadata is the only material included in LLM prompts; media files
//! themselves never leave the local execution boundary defined here.

mod engine;
mod error;
mod keystore;
mod llm;
mod model;
mod presets;
mod process;
mod progress;
mod session;
mod validation;

pub use engine::{
    AttemptLog, ExecutionEngine, ExecutionLog, ExecutionReport, ProgressEvent,
    DEFAULT_MAX_ATTEMPTS, DEFAULT_STDERR_TAIL_LINES,
};
pub use error::{OrchestratorError, Result};
pub use keystore::{InMemoryKeyStore, KeyStore};
pub use llm::{
    generate_plan, parse_plan_json, repair_plan, LlmProvider, PromptBuilder, PromptMessages,
    RetryContext, DEFAULT_MAX_METADATA_CHARS,
};
pub use model::{ExecutionPlan, ToolPlan};
pub use presets::{
    h264_1080p, loudnorm_two_pass, prores_422, superresolution_three_step,
    vertical_to_horizontal_blur, H264Encoder, H264Options, LoudnormMeasurement, Resolution,
    UpscaleEngine, UpscaleOptions,
};
pub use process::{ProcessExit, ProcessRunner};
pub use progress::{parse_progress_line, FfmpegProgress, ProgressStatus};
pub use session::{PlanSession, PlanState};
pub use validation::{
    normalize_confined_path, validate_execution_plan, validate_program, validate_tool_plan,
    ALLOWED_PROGRAMS,
};
