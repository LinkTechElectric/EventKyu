use std::fs;
use std::path::PathBuf;

use crate::manager::check_failure;
use crate::{Component, ComponentCheck, RuntimeError, RuntimeManager};

/// Injectable query for the latest upstream yt-dlp version.
pub trait LatestVersionQuery {
    /// Returns the latest normalized upstream version string.
    fn latest_version(&self) -> Result<String, RuntimeError>;
}

/// One of the two independent yt-dlp installation slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YtDlpSlot {
    /// Installer-provided fallback copy.
    Bundled,
    /// Copy replaced by the built-in updater.
    Updated,
}

/// A validated yt-dlp executable selected for use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveYtDlp {
    /// Executable path in application-private storage.
    pub path: PathBuf,
    /// Slot that supplied the executable.
    pub slot: YtDlpSlot,
    /// Expected or recorded version for the selected slot.
    pub version: String,
    /// Non-fatal validation warnings.
    pub warnings: Vec<String>,
}

/// Action taken by an update attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YtDlpUpdateAction {
    /// A new version was downloaded into the updated slot.
    Updated,
    /// A validated slot already had the requested latest version.
    AlreadyCurrent,
    /// Update or latest-version lookup failed and bundled was selected.
    FellBackToBundled,
}

/// Result of checking and applying a yt-dlp update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YtDlpUpdateOutcome {
    /// Executable that should be used after the attempt.
    pub active: ActiveYtDlp,
    /// Latest version returned by the query, when the query succeeded.
    pub latest_version: Option<String>,
    /// Update action taken.
    pub action: YtDlpUpdateAction,
    /// Update and fallback warnings.
    pub warnings: Vec<String>,
}

/// Updates yt-dlp while retaining independent bundled and updated slots.
pub struct YtDlpUpdater<'m, 't> {
    manager: &'m RuntimeManager<'t>,
    component: Component,
    platform: String,
}

impl<'m, 't> YtDlpUpdater<'m, 't> {
    /// Creates an updater from the yt-dlp manifest component.
    pub fn new(
        manager: &'m RuntimeManager<'t>,
        component: &Component,
        platform: impl Into<String>,
    ) -> Self {
        Self {
            manager,
            component: component.clone(),
            platform: platform.into(),
        }
    }

    /// Returns the bundled slot's executable path.
    pub fn bundled_path(&self) -> Result<PathBuf, RuntimeError> {
        Ok(self
            .slot_dir(YtDlpSlot::Bundled)?
            .join(self.component.artifact_name(&self.platform)?))
    }

    /// Returns the updated slot's executable path.
    pub fn updated_path(&self) -> Result<PathBuf, RuntimeError> {
        Ok(self
            .slot_dir(YtDlpSlot::Updated)?
            .join(self.component.artifact_name(&self.platform)?))
    }

    /// Selects a valid updated copy, automatically falling back to bundled.
    pub fn active_binary(&self) -> Result<ActiveYtDlp, RuntimeError> {
        let updated_component = self.updated_component()?;
        let updated_path = self.updated_path()?;
        let updated_check = self.manager.check_at_on_platform(
            &updated_component,
            &self.platform,
            &updated_path,
            true,
        )?;
        if updated_check.is_ready() {
            return Ok(active_from_check(
                updated_path,
                YtDlpSlot::Updated,
                updated_component.version,
                updated_check,
            ));
        }

        let bundled_path = self.bundled_path()?;
        let bundled_check = self.check_bundled(&bundled_path)?;
        if bundled_check.is_ready() {
            return Ok(active_from_check(
                bundled_path,
                YtDlpSlot::Bundled,
                self.component.version.clone(),
                bundled_check,
            ));
        }
        Err(RuntimeError::UpdateFailed {
            message: format!(
                "updated slot: {}; bundled slot: {}",
                check_failure(&updated_check),
                check_failure(&bundled_check)
            ),
        })
    }

    /// Checks upstream, installs a new copy, or falls back to the bundled slot.
    pub fn update(
        &self,
        latest_query: &dyn LatestVersionQuery,
    ) -> Result<YtDlpUpdateOutcome, RuntimeError> {
        let latest = match latest_query.latest_version() {
            Ok(latest) if !latest.trim().is_empty() => latest.trim().to_owned(),
            Ok(_) => {
                return self.fallback(
                    None,
                    "latest version query returned an empty version".to_owned(),
                )
            }
            Err(error) => return self.fallback(None, error.to_string()),
        };

        let mut candidate = self.component.clone();
        candidate.version = latest.clone();
        let updated_path = self.updated_path()?;
        let existing =
            self.manager
                .check_at_on_platform(&candidate, &self.platform, &updated_path, true)?;
        if existing.is_ready() {
            return Ok(YtDlpUpdateOutcome {
                active: active_from_check(
                    updated_path,
                    YtDlpSlot::Updated,
                    latest.clone(),
                    existing,
                ),
                latest_version: Some(latest),
                action: YtDlpUpdateAction::AlreadyCurrent,
                warnings: Vec::new(),
            });
        }

        if latest == self.component.version {
            let bundled_path = self.bundled_path()?;
            let bundled = self.check_bundled(&bundled_path)?;
            if bundled.is_ready() {
                return Ok(YtDlpUpdateOutcome {
                    active: active_from_check(
                        bundled_path,
                        YtDlpSlot::Bundled,
                        latest.clone(),
                        bundled,
                    ),
                    latest_version: Some(latest),
                    action: YtDlpUpdateAction::AlreadyCurrent,
                    warnings: Vec::new(),
                });
            }
        }

        let slot_dir = self.slot_dir(YtDlpSlot::Updated)?;
        match self
            .manager
            .download_to(&candidate, &self.platform, &slot_dir)
            .and_then(|download| {
                fs::write(slot_dir.join("version.txt"), &latest)?;
                Ok(download)
            }) {
            Ok(download) => {
                let active = ActiveYtDlp {
                    path: download.path,
                    slot: YtDlpSlot::Updated,
                    version: latest.clone(),
                    warnings: download.warnings.clone(),
                };
                Ok(YtDlpUpdateOutcome {
                    active,
                    latest_version: Some(latest),
                    action: YtDlpUpdateAction::Updated,
                    warnings: download.warnings,
                })
            }
            Err(error) => self.fallback(Some(latest), error.to_string()),
        }
    }

    fn fallback(
        &self,
        latest: Option<String>,
        reason: String,
    ) -> Result<YtDlpUpdateOutcome, RuntimeError> {
        let bundled_path = self.bundled_path()?;
        let bundled = self.check_bundled(&bundled_path)?;
        if !bundled.is_ready() {
            return Err(RuntimeError::UpdateFailed {
                message: format!("{reason}; bundled slot: {}", check_failure(&bundled)),
            });
        }
        let warning = format!("yt-dlp update unavailable; using bundled copy: {reason}");
        let mut warnings = vec![warning];
        warnings.extend(bundled.warnings.iter().cloned());
        Ok(YtDlpUpdateOutcome {
            active: active_from_check(
                bundled_path,
                YtDlpSlot::Bundled,
                self.component.version.clone(),
                bundled,
            ),
            latest_version: latest,
            action: YtDlpUpdateAction::FellBackToBundled,
            warnings,
        })
    }

    fn check_bundled(&self, path: &std::path::Path) -> Result<ComponentCheck, RuntimeError> {
        self.manager
            .check_at_on_platform(&self.component, &self.platform, path, true)
    }

    fn updated_component(&self) -> Result<Component, RuntimeError> {
        let mut component = self.component.clone();
        let version_path = self.slot_dir(YtDlpSlot::Updated)?.join("version.txt");
        if version_path.is_file() {
            component.version = fs::read_to_string(version_path)?.trim().to_owned();
        }
        Ok(component)
    }

    fn slot_dir(&self, slot: YtDlpSlot) -> Result<PathBuf, RuntimeError> {
        crate::manifest::validate_component_id(&self.component.id)?;
        let name = match slot {
            YtDlpSlot::Bundled => "bundled",
            YtDlpSlot::Updated => "updated",
        };
        Ok(self
            .manager
            .root()
            .join("slots")
            .join(&self.component.id)
            .join(name))
    }
}

fn active_from_check(
    path: PathBuf,
    slot: YtDlpSlot,
    version: String,
    check: ComponentCheck,
) -> ActiveYtDlp {
    ActiveYtDlp {
        path,
        slot,
        version,
        warnings: check.warnings,
    }
}
