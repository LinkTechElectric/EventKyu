use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::RuntimeError;

/// Computes file digests for dependency checks and downloads.
pub trait HashVerifier: Send + Sync {
    /// Returns the lowercase hexadecimal SHA-256 digest of `path`.
    fn sha256(&self, path: &Path) -> Result<String, RuntimeError>;
}

/// Real streaming SHA-256 implementation backed by [`sha2`].
#[derive(Debug, Default, Clone, Copy)]
pub struct Sha256HashVerifier;

impl HashVerifier for Sha256HashVerifier {
    fn sha256(&self, path: &Path) -> Result<String, RuntimeError> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(hex::encode(hasher.finalize()))
    }
}
