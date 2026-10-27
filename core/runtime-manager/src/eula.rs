use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::RuntimeError;

#[derive(Debug, Default, Serialize, Deserialize)]
struct EulaState {
    #[serde(default)]
    accepted_versions: BTreeMap<String, String>,
}

pub(crate) struct EulaStore {
    path: PathBuf,
}

impl EulaStore {
    pub(crate) fn new(root: &Path) -> Self {
        Self {
            path: root.join("eula-acceptances.json"),
        }
    }

    pub(crate) fn accepted_version(
        &self,
        component_id: &str,
    ) -> Result<Option<String>, RuntimeError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&self.path)?;
        let state: EulaState = serde_json::from_slice(&bytes)?;
        Ok(state.accepted_versions.get(component_id).cloned())
    }

    pub(crate) fn is_accepted(
        &self,
        component_id: &str,
        version: &str,
    ) -> Result<bool, RuntimeError> {
        Ok(self
            .accepted_version(component_id)?
            .is_some_and(|accepted| accepted == version))
    }

    pub(crate) fn accept(&self, component_id: &str, version: &str) -> Result<(), RuntimeError> {
        let mut state = if self.path.exists() {
            serde_json::from_slice::<EulaState>(&fs::read(&self.path)?)?
        } else {
            EulaState::default()
        };
        state
            .accepted_versions
            .insert(component_id.to_owned(), version.to_owned());
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&state)?)?;
        // Windows does not replace an existing destination with `rename`.
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        fs::rename(temporary, &self.path)?;
        Ok(())
    }
}
