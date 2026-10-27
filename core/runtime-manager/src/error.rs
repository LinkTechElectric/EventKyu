use std::path::PathBuf;

use thiserror::Error;

/// Describes one failed download source in an [`RuntimeError::AllSourcesFailed`] error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFailure {
    /// URL that was attempted.
    pub url: String,
    /// Human-readable failure reason.
    pub error: String,
}

/// Errors produced by runtime dependency management.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// A filesystem operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Manifest or state JSON could not be decoded or encoded.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// An injected transport failed.
    #[error("transport error for {url}: {message}")]
    Transport {
        /// URL being requested.
        url: String,
        /// Transport-specific error text.
        message: String,
    },
    /// The requested component is not present in a manifest.
    #[error("component not found: {0}")]
    ComponentNotFound(String),
    /// The component has no asset for the requested platform.
    #[error("component {component_id} has no asset for platform {platform}")]
    UnsupportedPlatform {
        /// Component identifier.
        component_id: String,
        /// Requested platform key.
        platform: String,
    },
    /// A component identifier cannot safely be used as a directory name.
    #[error("invalid component id: {0}")]
    InvalidComponentId(String),
    /// An asset does not contain enough information to choose an install file name.
    #[error("invalid asset for {component_id}: {reason}")]
    InvalidAsset {
        /// Component identifier.
        component_id: String,
        /// Validation failure.
        reason: String,
    },
    /// A download requiring a EULA was attempted without accepting the current version.
    #[error("EULA acceptance is required for {component_id} version {version}")]
    EulaAcceptanceRequired {
        /// Component identifier.
        component_id: String,
        /// Component version that must be accepted.
        version: String,
    },
    /// Every official and mirror URL failed.
    #[error("all download sources failed for {component_id}: {attempts:?}")]
    AllSourcesFailed {
        /// Component identifier.
        component_id: String,
        /// Failures in attempt order.
        attempts: Vec<SourceFailure>,
    },
    /// A pinned SHA-256 value is malformed.
    #[error("invalid SHA-256 value: {0}")]
    InvalidSha256(String),
    /// A completed file does not have the expected digest.
    #[error("SHA-256 mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        /// File that failed verification.
        path: PathBuf,
        /// Expected lowercase hexadecimal digest.
        expected: String,
        /// Actual lowercase hexadecimal digest.
        actual: String,
    },
    /// An injected version process probe failed.
    #[error("version probe failed for {path}: {message}")]
    VersionProbe {
        /// Executable that was probed.
        path: PathBuf,
        /// Probe-specific error text.
        message: String,
    },
    /// An injected latest-version query failed.
    #[error("latest version query failed: {0}")]
    LatestVersion(String),
    /// Both a yt-dlp update and the bundled fallback were unusable.
    #[error("yt-dlp update failed and bundled fallback is unavailable: {message}")]
    UpdateFailed {
        /// Combined update and fallback failure details.
        message: String,
    },
}
