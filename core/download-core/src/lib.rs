#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Media resolution and download orchestration for EventKyu.
//!
//! The crate plans downloads but deliberately does not perform network I/O or
//! spawn processes itself. Applications provide a [`ProcessRunner`]; tests can
//! use a mock. Third-party resolvers run before the default yt-dlp resolver,
//! and resolver failures fall back to yt-dlp automatically.

mod engine;
mod error;
mod job;
mod plan;
mod platform;
mod process;
mod progress;
mod registry;
mod resolver;
mod security;
mod ytdlp;

pub use engine::DownloadEngine;
pub use error::{DownloadError, DownloadResult};
pub use job::{DownloadEvent, DownloadJob, JobState};
pub use plan::{
    CookieBrowser, DirectStream, DownloadPlan, HttpHeader, MediaFormat, ResolveContext,
    ResolvePlan, StreamCookie,
};
pub use platform::{identify_platform, Platform};
pub use process::{ProcessExit, ProcessRunner};
pub use progress::{parse_progress_line, DownloadProgress};
pub use registry::ResolverRegistry;
pub use resolver::{MediaResolver, Resolution};
pub use security::{resolve_output_path, validate_url, MAX_OUTPUT_NAME_LENGTH, MAX_URL_LENGTH};
pub use ytdlp::YtDlpResolver;
