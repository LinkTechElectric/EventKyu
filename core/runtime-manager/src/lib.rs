#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! External dependency and runtime management for EventKyu.
//!
//! The crate owns manifest parsing, private-install health checks, EULA state,
//! resumable downloads with mirror fallback, and yt-dlp slot updates. All HTTP,
//! executable probing, and latest-version lookups are injected by the application.

mod download;
mod error;
mod eula;
mod hash;
mod health;
mod manager;
mod manifest;
mod transport;
mod update;

pub use download::DownloadOutcome;
pub use error::{RuntimeError, SourceFailure};
pub use hash::{HashVerifier, Sha256HashVerifier};
pub use health::{CheckStatus, ComponentCheck, HealthLevel, ShowtimeReport, VersionProbe};
pub use manager::RuntimeManager;
pub use manifest::{
    current_platform_key, Component, Distribution, Manifest, PlatformAsset, MACOS_AARCH64,
    MACOS_X86_64, UNPINNED_SHA256, WINDOWS_X86_64,
};
pub use transport::{HttpResponse, RangeRequest, Transport};
pub use update::{
    ActiveYtDlp, LatestVersionQuery, YtDlpSlot, YtDlpUpdateAction, YtDlpUpdateOutcome, YtDlpUpdater,
};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
