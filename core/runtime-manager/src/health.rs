use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::manifest::{Component, Distribution, PlatformAsset, UNPINNED_SHA256};
use crate::{HashVerifier, RuntimeError};

/// Probes an installed executable without tying this crate to a process API.
pub trait VersionProbe: Send + Sync {
    /// Runs a version probe with `args` and returns a normalized version string.
    ///
    /// `Ok(None)` means that the executable ran but no version could be determined.
    /// Implementations should return only the version token, not a multi-line banner.
    fn probe(&self, executable: &Path, args: &[String]) -> Result<Option<String>, RuntimeError>;
}

/// Installation health of one component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStatus {
    /// Installed and all requested checks passed.
    Ready,
    /// No private installation exists.
    Missing,
    /// Installed, but the probed version differs from the manifest.
    Outdated,
    /// The file size, executable probe, or pinned digest failed.
    Corrupted,
    /// The user must perform a guided installation.
    NeedsGuidedInstall,
    /// The current component version's EULA has not been accepted.
    NeedsEulaAcceptance,
}

impl CheckStatus {
    /// Maps this status to a showtime severity.
    pub const fn level(self) -> HealthLevel {
        match self {
            Self::Ready => HealthLevel::Green,
            Self::Outdated | Self::NeedsGuidedInstall | Self::NeedsEulaAcceptance => {
                HealthLevel::Yellow
            }
            Self::Missing | Self::Corrupted => HealthLevel::Red,
        }
    }
}

/// Detailed result of checking one component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentCheck {
    /// Component identifier.
    pub component_id: String,
    /// Overall installation status.
    pub status: CheckStatus,
    /// Private file that was inspected, when applicable.
    pub path: Option<PathBuf>,
    /// Version returned by the probe, when available.
    pub installed_version: Option<String>,
    /// Non-fatal warnings, including unpinned SHA-256 sentinels.
    pub warnings: Vec<String>,
    /// Diagnostic details explaining a non-ready status.
    pub details: Vec<String>,
}

impl ComponentCheck {
    pub(crate) fn new(component_id: &str, status: CheckStatus, path: Option<PathBuf>) -> Self {
        Self {
            component_id: component_id.to_owned(),
            status,
            path,
            installed_version: None,
            warnings: Vec::new(),
            details: Vec::new(),
        }
    }

    /// Returns true only when the status is [`CheckStatus::Ready`].
    pub const fn is_ready(&self) -> bool {
        matches!(self.status, CheckStatus::Ready)
    }
}

/// Green/yellow/red severity for showtime self-checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthLevel {
    /// All checks are ready and no warnings were emitted.
    Green,
    /// Operator attention is recommended, but no hard corruption or absence was found.
    Yellow,
    /// At least one component is missing or corrupted.
    Red,
}

/// Aggregated showmode self-check report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShowtimeReport {
    /// Worst severity across all checks; any warning raises green to yellow.
    pub level: HealthLevel,
    /// Individual component results.
    pub checks: Vec<ComponentCheck>,
}

impl ShowtimeReport {
    /// Builds a report and computes its overall severity.
    pub fn from_checks(checks: Vec<ComponentCheck>) -> Self {
        let mut level = HealthLevel::Green;
        for check in &checks {
            level = level.max(check.status.level());
            if !check.warnings.is_empty() {
                level = level.max(HealthLevel::Yellow);
            }
        }
        Self { level, checks }
    }
}

impl HealthLevel {
    fn max(self, other: Self) -> Self {
        match (self, other) {
            (Self::Red, _) | (_, Self::Red) => Self::Red,
            (Self::Yellow, _) | (_, Self::Yellow) => Self::Yellow,
            _ => Self::Green,
        }
    }
}

pub(crate) fn guided_check(component: &Component) -> ComponentCheck {
    let mut check = ComponentCheck::new(&component.id, CheckStatus::NeedsGuidedInstall, None);
    check
        .details
        .push("follow the upstream guided installation instructions".to_owned());
    check
}

pub(crate) fn inspect_at(
    component: &Component,
    asset: &PlatformAsset,
    path: &Path,
    eula_accepted: bool,
    probe: &dyn VersionProbe,
    verifier: &dyn HashVerifier,
) -> Result<ComponentCheck, RuntimeError> {
    if component.distribution == Distribution::Guided {
        return Ok(guided_check(component));
    }
    let mut check = ComponentCheck::new(
        &component.id,
        CheckStatus::Missing,
        Some(path.to_path_buf()),
    );
    if component.eula_required && !eula_accepted {
        check.status = CheckStatus::NeedsEulaAcceptance;
        check.details.push(format!(
            "accept the EULA for version {} before installation",
            component.version
        ));
        return Ok(check);
    }
    if !path.is_file() {
        return Ok(check);
    }

    if asset.size_bytes > 0 {
        let actual_size = path.metadata()?.len();
        if actual_size != asset.size_bytes {
            check.status = CheckStatus::Corrupted;
            check.details.push(format!(
                "size mismatch: expected {} bytes, got {actual_size}",
                asset.size_bytes
            ));
            return Ok(check);
        }
    }

    if !asset.version_args.is_empty() {
        match probe.probe(path, &asset.version_args) {
            Ok(Some(version)) => {
                check.installed_version = Some(version.clone());
                if normalize_version(&version) != normalize_version(&component.version) {
                    check.status = CheckStatus::Outdated;
                    check.details.push(format!(
                        "version mismatch: expected {}, got {version}",
                        component.version
                    ));
                    return Ok(check);
                }
            }
            Ok(None) => {
                check.warnings.push(format!(
                    "{}: version probe returned no parseable version",
                    component.id
                ));
            }
            Err(error) => {
                check.status = CheckStatus::Corrupted;
                check.details.push(error.to_string());
                return Ok(check);
            }
        }
    }

    if asset.sha256 == UNPINNED_SHA256 {
        check.warnings.push(format!(
            "{}: SHA-256 is TO_PIN; integrity verification was not performed",
            component.id
        ));
    } else {
        let expected = normalize_sha256(&asset.sha256)?;
        let actual = verifier.sha256(path)?;
        if actual != expected {
            check.status = CheckStatus::Corrupted;
            check.details.push(format!(
                "SHA-256 mismatch: expected {expected}, got {actual}"
            ));
            return Ok(check);
        }
    }

    check.status = CheckStatus::Ready;
    Ok(check)
}

fn normalize_version(version: &str) -> &str {
    let trimmed = version.trim();
    trimmed.strip_prefix('v').unwrap_or(trimmed)
}

pub(crate) fn normalize_sha256(value: &str) -> Result<String, RuntimeError> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(RuntimeError::InvalidSha256(value.to_owned()));
    }
    Ok(normalized)
}
