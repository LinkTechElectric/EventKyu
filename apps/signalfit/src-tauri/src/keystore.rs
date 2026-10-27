//! Operating-system protected BYOK storage.

use ai_orchestrator::{KeyStore, OrchestratorError, Result};

/// Keychain service name shared by every SignalFit provider account.
pub const KEYCHAIN_SERVICE: &str = "fit.eventkyu.signalfit";

/// Platform key store backed by macOS Keychain on supported hosts.
#[derive(Default)]
pub struct PlatformKeyStore;

impl PlatformKeyStore {
    /// Creates a platform store without reading any credentials.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
impl KeyStore for PlatformKeyStore {
    fn get(&self, provider: &str) -> Result<Option<String>> {
        validate_provider(provider)?;
        let output = std::process::Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-a",
                provider,
                "-s",
                KEYCHAIN_SERVICE,
                "-w",
            ])
            .output()
            .map_err(|_| keychain_error("无法调用 macOS Keychain"))?;
        if output.status.success() {
            let value = String::from_utf8(output.stdout)
                .map_err(|_| keychain_error("Keychain 返回了无法解码的数据"))?;
            return Ok(Some(value.trim_end_matches(['\r', '\n']).to_owned()));
        }
        if is_not_found(&output) {
            Ok(None)
        } else {
            // Do not forward security output: diagnostics must never echo a key.
            Err(keychain_error("读取 Keychain 凭据失败"))
        }
    }

    fn set(&mut self, provider: &str, key: &str) -> Result<()> {
        validate_provider(provider)?;
        if key.is_empty() {
            return Err(keychain_error("API Key 不能为空"));
        }
        let status = std::process::Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-a",
                provider,
                "-s",
                KEYCHAIN_SERVICE,
                "-w",
                key,
                "-U",
            ])
            .status()
            .map_err(|_| keychain_error("无法调用 macOS Keychain"))?;
        if status.success() {
            Ok(())
        } else {
            Err(keychain_error("写入 Keychain 凭据失败"))
        }
    }

    fn delete(&mut self, provider: &str) -> Result<bool> {
        validate_provider(provider)?;
        let output = std::process::Command::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-a",
                provider,
                "-s",
                KEYCHAIN_SERVICE,
            ])
            .output()
            .map_err(|_| keychain_error("无法调用 macOS Keychain"))?;
        if output.status.success() {
            Ok(true)
        } else if is_not_found(&output) {
            Ok(false)
        } else {
            Err(keychain_error("删除 Keychain 凭据失败"))
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl KeyStore for PlatformKeyStore {
    fn get(&self, provider: &str) -> Result<Option<String>> {
        validate_provider(provider)?;
        Err(unsupported())
    }

    fn set(&mut self, provider: &str, _key: &str) -> Result<()> {
        validate_provider(provider)?;
        Err(unsupported())
    }

    fn delete(&mut self, provider: &str) -> Result<bool> {
        validate_provider(provider)?;
        Err(unsupported())
    }
}

fn validate_provider(provider: &str) -> Result<()> {
    let valid = !provider.is_empty()
        && provider.len() <= 64
        && provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(keychain_error(
            "provider 仅允许 1–64 位字母、数字、点、横线或下划线",
        ))
    }
}

#[cfg(target_os = "macos")]
fn is_not_found(output: &std::process::Output) -> bool {
    output.status.code() == Some(44)
        || String::from_utf8_lossy(&output.stderr).contains("could not be found")
}

fn keychain_error(message: &str) -> OrchestratorError {
    OrchestratorError::Io(message.to_owned())
}

#[cfg(not(target_os = "macos"))]
fn unsupported() -> OrchestratorError {
    keychain_error("当前平台暂不支持系统 Keychain，SignalFit 不会退化为明文文件存储")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_names_are_bounded_argv_tokens() {
        assert!(validate_provider("openai-compatible_1").is_ok());
        assert!(validate_provider("").is_err());
        assert!(validate_provider("bad provider").is_err());
        assert!(validate_provider("bad;provider").is_err());
        assert!(validate_provider(&"x".repeat(65)).is_err());
    }
}
