use runtime_manager::{
    current_platform_key, HttpResponse, Manifest, RangeRequest, RuntimeError, RuntimeManager,
    Sha256HashVerifier, ShowtimeReport, Transport, VersionProbe,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

const MANIFEST_JSON: &[u8] = include_bytes!("../../../../manifest/manifest.json");
const NETWORK_STATUS: &str = "未接入网络：占位 Transport 不会发起 HTTP 请求";

struct OfflineTransport;

impl OfflineTransport {
    fn unavailable<T>(url: &str) -> Result<T, RuntimeError> {
        Err(RuntimeError::Transport {
            url: url.to_owned(),
            message: NETWORK_STATUS.to_owned(),
        })
    }
}

impl Transport for OfflineTransport {
    fn get(&self, url: &str, _range: Option<RangeRequest>) -> Result<HttpResponse, RuntimeError> {
        Self::unavailable(url)
    }

    fn head(&self, url: &str) -> Result<HttpResponse, RuntimeError> {
        Self::unavailable(url)
    }
}

struct OfflineVersionProbe;

impl VersionProbe for OfflineVersionProbe {
    fn probe(&self, executable: &Path, _args: &[String]) -> Result<Option<String>, RuntimeError> {
        Err(RuntimeError::VersionProbe {
            path: executable.to_path_buf(),
            message: "版本探测进程尚未接入 Windows 壳".to_owned(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelfcheck {
    pub platform: String,
    pub network: String,
    pub runtime_root: String,
    pub skipped_components: Vec<String>,
    pub report: ShowtimeReport,
}

fn selfcheck_at(runtime_root: PathBuf) -> Result<RuntimeSelfcheck, String> {
    let manifest = Manifest::from_slice(MANIFEST_JSON).map_err(|error| error.to_string())?;
    let platform = current_platform_key()
        .ok_or_else(|| "current compilation target has no runtime manifest key".to_owned())?;
    let transport = OfflineTransport;
    let version_probe = OfflineVersionProbe;
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(&runtime_root, &transport, &version_probe, &verifier);

    let components = manifest
        .components
        .iter()
        .filter(|component| component.asset(platform).is_some())
        .collect::<Vec<_>>();
    let skipped_components = manifest
        .components
        .iter()
        .filter(|component| component.asset(platform).is_none())
        .map(|component| component.id.clone())
        .collect();
    let report = manager
        .showtime_report(components, platform)
        .map_err(|error| error.to_string())?;

    Ok(RuntimeSelfcheck {
        platform: platform.to_owned(),
        network: NETWORK_STATUS.to_owned(),
        runtime_root: runtime_root.to_string_lossy().into_owned(),
        skipped_components,
        report,
    })
}

#[tauri::command]
pub fn runtime_selfcheck() -> Result<RuntimeSelfcheck, String> {
    let runtime_root = std::env::var_os("EVENTKYU_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("eventkyu-win").join("runtime"));
    selfcheck_at(runtime_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_transport_reports_network_not_connected() {
        let error = OfflineTransport
            .head("https://example.com/runtime")
            .unwrap_err();
        assert!(error.to_string().contains("未接入网络"));
    }

    #[test]
    fn real_manifest_parses_and_contains_windows_ytdlp() {
        let manifest = Manifest::from_slice(MANIFEST_JSON).unwrap();
        let ytdlp = manifest.component("yt-dlp").unwrap();
        assert!(ytdlp.asset(runtime_manager::WINDOWS_X86_64).is_some());
    }

    #[test]
    fn selfcheck_filters_components_without_host_assets() {
        let root = std::env::temp_dir().join(format!(
            "eventkyu-win-selfcheck-test-{}",
            std::process::id()
        ));
        let report = selfcheck_at(root).unwrap();
        assert_eq!(report.network, NETWORK_STATUS);
        assert!(!report.report.checks.is_empty());
        assert!(report
            .report
            .checks
            .iter()
            .all(|check| check.component_id != "blackhole" || cfg!(target_os = "macos")));
    }
}
