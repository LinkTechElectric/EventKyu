use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::health::normalize_sha256;
use crate::manifest::{Component, UNPINNED_SHA256};
use crate::{HashVerifier, RangeRequest, RuntimeError, SourceFailure, Transport};

/// Successful download and verification result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    /// Final path inside application-private storage.
    pub path: PathBuf,
    /// Official or mirror URL that supplied the accepted bytes.
    pub source_url: String,
    /// Number of bytes received in the successful response.
    pub bytes_written: u64,
    /// Whether the successful response resumed an existing partial file.
    pub resumed: bool,
    /// Non-fatal integrity warnings, notably for `TO_PIN` assets.
    pub warnings: Vec<String>,
}

pub(crate) struct Downloader<'a> {
    transport: &'a dyn Transport,
    verifier: &'a dyn HashVerifier,
}

impl<'a> Downloader<'a> {
    pub(crate) fn new(transport: &'a dyn Transport, verifier: &'a dyn HashVerifier) -> Self {
        Self {
            transport,
            verifier,
        }
    }

    pub(crate) fn download(
        &self,
        component: &Component,
        platform: &str,
        destination_dir: &Path,
    ) -> Result<DownloadOutcome, RuntimeError> {
        let asset = component.require_asset(platform)?;
        let artifact_name = component.artifact_name(platform)?;
        let expected_sha256 = if asset.sha256 == UNPINNED_SHA256 {
            None
        } else {
            Some(normalize_sha256(&asset.sha256)?)
        };
        fs::create_dir_all(destination_dir)?;
        let final_path = destination_dir.join(artifact_name);
        let partial_path = partial_path(&final_path);
        let sources = std::iter::once(asset.url.as_str())
            .chain(asset.mirrors.iter().map(String::as_str))
            .collect::<Vec<_>>();
        let mut failures = Vec::new();

        for url in sources {
            match self.try_source(
                url,
                &final_path,
                &partial_path,
                asset.size_bytes,
                expected_sha256.as_deref(),
            ) {
                Ok((bytes_written, resumed)) => {
                    let warnings = if expected_sha256.is_none() {
                        vec![format!(
                            "{}: SHA-256 is TO_PIN; downloaded bytes were not digest-verified",
                            component.id
                        )]
                    } else {
                        Vec::new()
                    };
                    return Ok(DownloadOutcome {
                        path: final_path,
                        source_url: url.to_owned(),
                        bytes_written,
                        resumed,
                        warnings,
                    });
                }
                Err(error) => failures.push(SourceFailure {
                    url: url.to_owned(),
                    error: error.to_string(),
                }),
            }
        }

        Err(RuntimeError::AllSourcesFailed {
            component_id: component.id.clone(),
            attempts: failures,
        })
    }

    fn try_source(
        &self,
        url: &str,
        final_path: &Path,
        partial_path: &Path,
        expected_size: u64,
        expected_sha256: Option<&str>,
    ) -> Result<(u64, bool), RuntimeError> {
        let existing_bytes = match fs::metadata(partial_path) {
            Ok(metadata) if metadata.is_file() => metadata.len(),
            Ok(_) => {
                fs::remove_file(partial_path)?;
                0
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        let range = (existing_bytes > 0).then_some(RangeRequest::from(existing_bytes));
        let response = self.transport.get(url, range)?;
        if !matches!(response.status, 200 | 206) || (response.status == 206 && range.is_none()) {
            return Err(RuntimeError::Transport {
                url: url.to_owned(),
                message: format!("unexpected HTTP status {}", response.status),
            });
        }
        let resumed = response.status == 206 && range.is_some();
        let mut options = OpenOptions::new();
        options.create(true).write(true);
        if resumed {
            options.append(true);
        } else {
            options.truncate(true);
        }
        let mut file = options.open(partial_path)?;
        file.write_all(&response.body)?;
        file.sync_all()?;
        let bytes_written = response.body.len() as u64;

        if expected_size > 0 {
            let actual_size = fs::metadata(partial_path)?.len();
            if actual_size != expected_size {
                fs::remove_file(partial_path)?;
                return Err(RuntimeError::Transport {
                    url: url.to_owned(),
                    message: format!(
                        "size mismatch: expected {expected_size} bytes, got {actual_size}"
                    ),
                });
            }
        }

        if let Some(expected) = expected_sha256 {
            let actual = self.verifier.sha256(partial_path)?;
            if actual != expected {
                fs::remove_file(partial_path)?;
                return Err(RuntimeError::HashMismatch {
                    path: partial_path.to_path_buf(),
                    expected: expected.to_owned(),
                    actual,
                });
            }
        }

        if final_path.exists() {
            fs::remove_file(final_path)?;
        }
        fs::rename(partial_path, final_path)?;
        Ok((bytes_written, resumed))
    }
}

pub(crate) fn partial_path(final_path: &Path) -> PathBuf {
    let mut name = OsString::from(final_path.as_os_str());
    name.push(".part");
    PathBuf::from(name)
}
