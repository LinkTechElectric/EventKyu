use std::path::{Path, PathBuf};

use crate::download::Downloader;
use crate::eula::EulaStore;
use crate::health::{guided_check, inspect_at};
use crate::manifest::{validate_component_id, Component, Distribution};
use crate::{
    CheckStatus, ComponentCheck, DownloadOutcome, HashVerifier, RuntimeError, ShowtimeReport,
    Transport, VersionProbe,
};

/// Coordinates private installation, health checks, EULA state, and downloads.
///
/// The manager never installs into system locations. All files live beneath the
/// root supplied by the application.
pub struct RuntimeManager<'a> {
    root: PathBuf,
    transport: &'a dyn Transport,
    version_probe: &'a dyn VersionProbe,
    hash_verifier: &'a dyn HashVerifier,
    eulas: EulaStore,
}

impl<'a> RuntimeManager<'a> {
    /// Creates a manager rooted at an application-private directory.
    pub fn new(
        root: impl Into<PathBuf>,
        transport: &'a dyn Transport,
        version_probe: &'a dyn VersionProbe,
        hash_verifier: &'a dyn HashVerifier,
    ) -> Self {
        let root = root.into();
        let eulas = EulaStore::new(&root);
        Self {
            root,
            transport,
            version_probe,
            hash_verifier,
            eulas,
        }
    }

    /// Returns the application-private root used by this manager.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Records acceptance of one exact component version's EULA.
    pub fn accept_eula(&self, component_id: &str, version: &str) -> Result<(), RuntimeError> {
        validate_component_id(component_id)?;
        self.eulas.accept(component_id, version)
    }

    /// Returns whether the exact component version's EULA has been accepted.
    pub fn eula_accepted(&self, component_id: &str, version: &str) -> Result<bool, RuntimeError> {
        validate_component_id(component_id)?;
        self.eulas.is_accepted(component_id, version)
    }

    /// Returns the expected private installation path for a component asset.
    pub fn installed_path(
        &self,
        component: &Component,
        platform: &str,
    ) -> Result<PathBuf, RuntimeError> {
        Ok(self
            .component_dir(component, platform)?
            .join(component.artifact_name(platform)?))
    }

    /// Checks installation existence, optional version, size, and SHA-256.
    pub fn check(
        &self,
        component: &Component,
        platform: &str,
    ) -> Result<ComponentCheck, RuntimeError> {
        if component.distribution == Distribution::Guided {
            return Ok(guided_check(component));
        }
        let path = self.installed_path(component, platform)?;
        let accepted = self.eula_accepted(&component.id, &component.version)?;
        self.check_at_on_platform(component, platform, &path, accepted)
    }

    /// Builds a green/yellow/red showmode self-check report.
    pub fn showtime_report<'c>(
        &self,
        components: impl IntoIterator<Item = &'c Component>,
        platform: &str,
    ) -> Result<ShowtimeReport, RuntimeError> {
        let checks = components
            .into_iter()
            .map(|component| self.check(component, platform))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ShowtimeReport::from_checks(checks))
    }

    /// Downloads and installs an asset after enforcing its EULA gate.
    pub fn download(
        &self,
        component: &Component,
        platform: &str,
    ) -> Result<DownloadOutcome, RuntimeError> {
        let destination = self.component_dir(component, platform)?;
        self.download_to(component, platform, &destination)
    }

    pub(crate) fn check_at_on_platform(
        &self,
        component: &Component,
        platform: &str,
        path: &Path,
        eula_accepted: bool,
    ) -> Result<ComponentCheck, RuntimeError> {
        let asset = component.require_asset(platform)?;
        inspect_at(
            component,
            asset,
            path,
            eula_accepted,
            self.version_probe,
            self.hash_verifier,
        )
    }

    pub(crate) fn download_to(
        &self,
        component: &Component,
        platform: &str,
        destination: &Path,
    ) -> Result<DownloadOutcome, RuntimeError> {
        validate_component_id(&component.id)?;
        if component.eula_required && !self.eula_accepted(&component.id, &component.version)? {
            return Err(RuntimeError::EulaAcceptanceRequired {
                component_id: component.id.clone(),
                version: component.version.clone(),
            });
        }
        Downloader::new(self.transport, self.hash_verifier).download(
            component,
            platform,
            destination,
        )
    }

    fn component_dir(
        &self,
        component: &Component,
        platform: &str,
    ) -> Result<PathBuf, RuntimeError> {
        validate_component_id(&component.id)?;
        Ok(self
            .root
            .join("components")
            .join(&component.id)
            .join(platform))
    }
}

/// Converts a failed status into compact diagnostic text for updater errors.
pub(crate) fn check_failure(check: &ComponentCheck) -> String {
    if check.status == CheckStatus::Ready {
        return "ready".to_owned();
    }
    if check.details.is_empty() {
        format!("{:?}", check.status)
    } else {
        format!("{:?}: {}", check.status, check.details.join("; "))
    }
}
