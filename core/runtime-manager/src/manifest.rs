use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::RuntimeError;

/// Platform key for Apple Silicon Macs.
pub const MACOS_AARCH64: &str = "macos-aarch64";
/// Platform key for Intel Macs.
pub const MACOS_X86_64: &str = "macos-x86_64";
/// Platform key for 64-bit Windows.
pub const WINDOWS_X86_64: &str = "windows-x86_64";
/// Sentinel used when an asset digest has not yet been pinned.
pub const UNPINNED_SHA256: &str = "TO_PIN";

/// How a component is delivered to users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Distribution {
    /// Included in the application installer and copied into private storage.
    Bundled,
    /// Downloaded on demand by the built-in downloader.
    Download,
    /// Requires guided installation by the user.
    Guided,
}

/// A platform-specific downloadable asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformAsset {
    /// Official source URL, always attempted before mirrors.
    pub url: String,
    /// Expected SHA-256, or [`UNPINNED_SHA256`] to warn instead of verifying.
    pub sha256: String,
    /// Expected byte length; zero means that the length is not pinned.
    pub size_bytes: u64,
    /// Fallback URLs, attempted in list order.
    #[serde(default)]
    pub mirrors: Vec<String>,
    /// Optional arguments used to ask an installed executable for its version.
    #[serde(default)]
    pub version_args: Vec<String>,
    /// Optional stable local file name, useful when the URL has no file segment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// Optional operational note for packagers and maintainers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One external runtime component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    /// Stable identifier, also used as a private-storage directory name.
    pub id: String,
    /// Human-readable component name.
    pub name: String,
    /// Manifest or bundled version.
    pub version: String,
    /// SPDX identifier or upstream license description.
    pub license: String,
    /// Delivery strategy.
    pub distribution: Distribution,
    /// Whether the current version requires explicit EULA acceptance.
    pub eula_required: bool,
    /// Assets indexed by platform key.
    pub platforms: BTreeMap<String, PlatformAsset>,
    /// Optional packager or download notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Component {
    /// Returns the asset for `platform`, if one exists.
    pub fn asset(&self, platform: &str) -> Option<&PlatformAsset> {
        self.platforms.get(platform)
    }

    /// Returns the asset for `platform` or a descriptive error.
    pub fn require_asset(&self, platform: &str) -> Result<&PlatformAsset, RuntimeError> {
        self.asset(platform)
            .ok_or_else(|| RuntimeError::UnsupportedPlatform {
                component_id: self.id.clone(),
                platform: platform.to_owned(),
            })
    }

    /// Chooses a safe local artifact name for a platform asset.
    pub fn artifact_name(&self, platform: &str) -> Result<String, RuntimeError> {
        let asset = self.require_asset(platform)?;
        let candidate = asset.file_name.clone().unwrap_or_else(|| {
            let without_query = asset
                .url
                .split(['?', '#'])
                .next()
                .unwrap_or(asset.url.as_str());
            without_query
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned()
        });
        if candidate.is_empty()
            || candidate == "."
            || candidate == ".."
            || candidate.contains('/')
            || candidate.contains('\\')
        {
            return Err(RuntimeError::InvalidAsset {
                component_id: self.id.clone(),
                reason: format!("unsafe or empty artifact name: {candidate}"),
            });
        }
        Ok(candidate)
    }
}

/// The complete external component manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Components in stable display order.
    pub components: Vec<Component>,
}

impl Manifest {
    /// Parses a manifest from JSON text.
    pub fn from_json(json: &str) -> Result<Self, RuntimeError> {
        Ok(serde_json::from_str(json)?)
    }

    /// Parses a manifest from JSON bytes.
    pub fn from_slice(json: &[u8]) -> Result<Self, RuntimeError> {
        Ok(serde_json::from_slice(json)?)
    }

    /// Serializes the manifest as pretty JSON.
    pub fn to_json_pretty(&self) -> Result<String, RuntimeError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Finds a component by its stable identifier.
    pub fn component(&self, id: &str) -> Option<&Component> {
        self.components.iter().find(|component| component.id == id)
    }
}

/// Returns the platform key for the compilation target when it is supported.
pub const fn current_platform_key() -> Option<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return Some(MACOS_AARCH64);
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return Some(MACOS_X86_64);
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Some(WINDOWS_X86_64);
    }
    #[allow(unreachable_code)]
    None
}

pub(crate) fn validate_component_id(id: &str) -> Result<(), RuntimeError> {
    let valid = !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(RuntimeError::InvalidComponentId(id.to_owned()))
    }
}
