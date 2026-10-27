use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::test_support::{MockTransport, MockVersionProbe, TestDir};
use crate::*;

const PLATFORM: &str = MACOS_AARCH64;
const PAYLOAD: &[u8] = b"verified runtime payload";

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn asset() -> PlatformAsset {
    PlatformAsset {
        url: "https://official.example/tool.bin".to_owned(),
        sha256: digest(PAYLOAD),
        size_bytes: PAYLOAD.len() as u64,
        mirrors: vec![
            "https://mirror-one.example/tool.bin".to_owned(),
            "https://mirror-two.example/tool.bin".to_owned(),
        ],
        version_args: Vec::new(),
        file_name: Some("tool.bin".to_owned()),
        note: None,
    }
}

fn component(distribution: Distribution) -> Component {
    Component {
        id: "test-tool".to_owned(),
        name: "Test Tool".to_owned(),
        version: "1.2.3".to_owned(),
        license: "MIT".to_owned(),
        distribution,
        eula_required: false,
        platforms: BTreeMap::from([(PLATFORM.to_owned(), asset())]),
        note: None,
    }
}

fn install(manager: &RuntimeManager<'_>, component: &Component, bytes: &[u8]) {
    let path = manager.installed_path(component, PLATFORM).expect("path");
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    fs::write(path, bytes).expect("write installed file");
}

#[test]
fn manifest_parses_complete_component() {
    let manifest = Manifest::from_json(
        r#"{
          "components": [{
            "id": "yt-dlp", "name": "yt-dlp", "version": "2026.10.21",
            "license": "Unlicense", "distribution": "bundled",
            "eula_required": false,
            "platforms": {
              "macos-aarch64": {
                "url": "https://example.test/yt-dlp", "sha256": "TO_PIN",
                "size_bytes": 123, "mirrors": ["https://mirror.test/yt-dlp"],
                "version_args": ["--version"]
              },
              "macos-x86_64": {
                "url": "https://example.test/yt-dlp", "sha256": "TO_PIN",
                "size_bytes": 123, "mirrors": [], "version_args": []
              },
              "windows-x86_64": {
                "url": "https://example.test/yt-dlp.exe", "sha256": "TO_PIN",
                "size_bytes": 123, "mirrors": [], "version_args": []
              }
            }
          }]
        }"#,
    )
    .expect("valid manifest");
    let component = manifest.component("yt-dlp").expect("component");
    assert_eq!(component.distribution, Distribution::Bundled);
    assert_eq!(component.platforms.len(), 3);
    assert_eq!(
        component.asset(PLATFORM).expect("asset").version_args,
        ["--version"]
    );
    assert_eq!(component.artifact_name(PLATFORM).expect("name"), "yt-dlp");
}

#[test]
fn manifest_defaults_optional_asset_fields() {
    let manifest = Manifest::from_json(
        r#"{"components":[{"id":"x","name":"X","version":"1","license":"MIT","distribution":"download","eula_required":false,"platforms":{"windows-x86_64":{"url":"https://example.test/x.exe","sha256":"TO_PIN","size_bytes":0}}}]}"#,
    )
    .expect("valid manifest");
    let asset = manifest.components[0].asset(WINDOWS_X86_64).expect("asset");
    assert!(asset.mirrors.is_empty());
    assert!(asset.version_args.is_empty());
    assert_eq!(asset.file_name, None);
}

#[test]
fn manifest_rejects_unknown_distribution() {
    let json = r#"{"components":[{"id":"x","name":"X","version":"1","license":"MIT","distribution":"sideload","eula_required":false,"platforms":{}}]}"#;
    assert!(Manifest::from_json(json).is_err());
}

#[test]
fn repository_manifest_parses_and_contains_all_components() {
    let manifest = Manifest::from_json(include_str!("../../../manifest/manifest.json"))
        .expect("repository manifest");
    assert_eq!(manifest.components.len(), 7);
    for id in [
        "yt-dlp",
        "ffmpeg",
        "deno",
        "ndi-runtime",
        "libreoffice",
        "blackhole",
        "vb-cable",
    ] {
        assert!(manifest.component(id).is_some(), "missing {id}");
    }
    assert!(
        manifest
            .component("ndi-runtime")
            .expect("ndi")
            .eula_required
    );
}

#[test]
fn health_reports_guided_install() {
    let temp = TestDir::new("guided");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let check = manager
        .check(&component(Distribution::Guided), PLATFORM)
        .expect("check");
    assert_eq!(check.status, CheckStatus::NeedsGuidedInstall);
    assert_eq!(check.status.level(), HealthLevel::Yellow);
}

#[test]
fn health_reports_missing_file() {
    let temp = TestDir::new("missing");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let check = manager
        .check(&component(Distribution::Download), PLATFORM)
        .expect("check");
    assert_eq!(check.status, CheckStatus::Missing);
    assert_eq!(check.status.level(), HealthLevel::Red);
}

#[test]
fn health_requires_current_eula_acceptance() {
    let temp = TestDir::new("health-eula");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Download);
    component.eula_required = true;
    install(&manager, &component, PAYLOAD);
    assert_eq!(
        manager.check(&component, PLATFORM).expect("check").status,
        CheckStatus::NeedsEulaAcceptance
    );
    manager.accept_eula(&component.id, "0.9").expect("old eula");
    assert_eq!(
        manager.check(&component, PLATFORM).expect("check").status,
        CheckStatus::NeedsEulaAcceptance
    );
    manager
        .accept_eula(&component.id, &component.version)
        .expect("accept current");
    assert_eq!(
        manager.check(&component, PLATFORM).expect("check").status,
        CheckStatus::Ready
    );
}

#[test]
fn health_ready_with_version_and_sha256() {
    let temp = TestDir::new("ready");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    probe.returns(Some("v1.2.3\n"));
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Bundled);
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .version_args = vec!["--version".to_owned()];
    install(&manager, &component, PAYLOAD);
    let check = manager.check(&component, PLATFORM).expect("check");
    assert_eq!(check.status, CheckStatus::Ready);
    assert_eq!(check.installed_version.as_deref(), Some("v1.2.3\n"));
    assert!(check.warnings.is_empty());
}

#[test]
fn health_reports_outdated_version() {
    let temp = TestDir::new("outdated");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    probe.returns(Some("1.2.2"));
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Bundled);
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .version_args = vec!["--version".to_owned()];
    install(&manager, &component, PAYLOAD);
    assert_eq!(
        manager.check(&component, PLATFORM).expect("check").status,
        CheckStatus::Outdated
    );
}

#[test]
fn health_reports_corrupted_hash() {
    let temp = TestDir::new("corrupted");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Bundled);
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .size_bytes = 0;
    install(&manager, &component, b"tampered");
    let check = manager.check(&component, PLATFORM).expect("check");
    assert_eq!(check.status, CheckStatus::Corrupted);
    assert!(check.details[0].contains("SHA-256 mismatch"));
}

#[test]
fn health_reports_failed_version_probe_as_corrupted() {
    let temp = TestDir::new("probe-failed");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    probe.fails("executable format error");
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Bundled);
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .version_args = vec!["--version".to_owned()];
    install(&manager, &component, PAYLOAD);
    assert_eq!(
        manager.check(&component, PLATFORM).expect("check").status,
        CheckStatus::Corrupted
    );
}

#[test]
fn to_pin_health_warns_and_makes_showtime_yellow() {
    let temp = TestDir::new("to-pin-health");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Bundled);
    component.platforms.get_mut(PLATFORM).expect("asset").sha256 = UNPINNED_SHA256.to_owned();
    install(&manager, &component, PAYLOAD);
    let check = manager.check(&component, PLATFORM).expect("check");
    assert_eq!(check.status, CheckStatus::Ready);
    assert_eq!(check.warnings.len(), 1);
    assert!(check.warnings[0].contains("TO_PIN"));
    let report = manager
        .showtime_report([&component], PLATFORM)
        .expect("report");
    assert_eq!(report.level, HealthLevel::Yellow);
}

#[test]
fn eula_gates_download_and_acceptance_persists() {
    let temp = TestDir::new("download-eula");
    let transport = MockTransport::default();
    transport.respond(HttpResponse::new(200, PAYLOAD));
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Download);
    component.eula_required = true;
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .mirrors
        .clear();
    assert!(matches!(
        manager.download(&component, PLATFORM),
        Err(RuntimeError::EulaAcceptanceRequired { .. })
    ));
    assert!(transport.gets().is_empty());
    manager
        .accept_eula(&component.id, &component.version)
        .expect("accept");
    manager
        .download(&component, PLATFORM)
        .expect("download after acceptance");
    let second = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    assert!(second
        .eula_accepted(&component.id, &component.version)
        .expect("persisted acceptance"));
}

#[test]
fn downloader_tries_official_then_mirrors_in_order() {
    let temp = TestDir::new("mirror-order");
    let transport = MockTransport::default();
    transport.fail("official unavailable");
    transport.fail("first mirror unavailable");
    transport.respond(HttpResponse::new(200, PAYLOAD));
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let outcome = manager
        .download(&component(Distribution::Download), PLATFORM)
        .expect("mirror succeeds");
    assert_eq!(outcome.source_url, "https://mirror-two.example/tool.bin");
    let urls = transport
        .gets()
        .into_iter()
        .map(|request| request.url)
        .collect::<Vec<_>>();
    assert_eq!(
        urls,
        [
            "https://official.example/tool.bin",
            "https://mirror-one.example/tool.bin",
            "https://mirror-two.example/tool.bin"
        ]
    );
}

#[test]
fn downloader_resumes_partial_file_with_range() {
    let temp = TestDir::new("resume");
    let transport = MockTransport::default();
    transport.respond(HttpResponse::new(206, b"world".as_slice()));
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Download);
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .mirrors
        .clear();
    let final_path = manager.installed_path(&component, PLATFORM).expect("path");
    fs::create_dir_all(final_path.parent().expect("parent")).expect("parent");
    fs::write(format!("{}.part", final_path.display()), b"hello ").expect("partial");
    let full = b"hello world";
    let asset = component.platforms.get_mut(PLATFORM).expect("asset");
    asset.sha256 = digest(full);
    asset.size_bytes = full.len() as u64;
    let outcome = manager.download(&component, PLATFORM).expect("resumed");
    assert!(outcome.resumed);
    assert_eq!(transport.gets()[0].range, Some(RangeRequest::from(6)));
    assert_eq!(
        transport.gets()[0].range.expect("range").header_value(),
        "bytes=6-"
    );
    assert_eq!(fs::read(final_path).expect("final"), full);
}

#[test]
fn downloader_restarts_when_server_ignores_range() {
    let temp = TestDir::new("range-ignored");
    let transport = MockTransport::default();
    transport.respond(HttpResponse::new(200, PAYLOAD));
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Download);
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .mirrors
        .clear();
    let final_path = manager.installed_path(&component, PLATFORM).expect("path");
    fs::create_dir_all(final_path.parent().expect("parent")).expect("parent");
    fs::write(format!("{}.part", final_path.display()), b"stale").expect("partial");
    let outcome = manager.download(&component, PLATFORM).expect("restarted");
    assert!(!outcome.resumed);
    assert_eq!(fs::read(final_path).expect("final"), PAYLOAD);
}

#[test]
fn hash_mismatch_deletes_partial_and_switches_mirror_without_resume() {
    let temp = TestDir::new("hash-mirror");
    let transport = MockTransport::default();
    transport.respond(HttpResponse::new(200, b"bad bytes".as_slice()));
    transport.respond(HttpResponse::new(200, PAYLOAD));
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let component = component(Distribution::Download);
    let outcome = manager.download(&component, PLATFORM).expect("good mirror");
    assert_eq!(outcome.source_url, "https://mirror-one.example/tool.bin");
    let requests = transport.gets();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].range, None);
    assert_eq!(requests[1].range, None);
    assert!(!Path::new(&format!("{}.part", outcome.path.display())).exists());
    assert_eq!(fs::read(outcome.path).expect("final"), PAYLOAD);
}

#[test]
fn downloader_reports_all_failed_sources() {
    let temp = TestDir::new("all-failed");
    let transport = MockTransport::default();
    transport.fail("one");
    transport.fail("two");
    transport.fail("three");
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let error = manager
        .download(&component(Distribution::Download), PLATFORM)
        .expect_err("all sources fail");
    match error {
        RuntimeError::AllSourcesFailed { attempts, .. } => assert_eq!(attempts.len(), 3),
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn to_pin_download_returns_explicit_warning() {
    let temp = TestDir::new("to-pin-download");
    let transport = MockTransport::default();
    transport.respond(HttpResponse::new(200, PAYLOAD));
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Download);
    component.platforms.get_mut(PLATFORM).expect("asset").sha256 = UNPINNED_SHA256.to_owned();
    let outcome = manager.download(&component, PLATFORM).expect("download");
    assert_eq!(outcome.warnings.len(), 1);
    assert!(outcome.warnings[0].contains("TO_PIN"));
}

struct StaticLatest(&'static str);

impl LatestVersionQuery for StaticLatest {
    fn latest_version(&self) -> Result<String, RuntimeError> {
        Ok(self.0.to_owned())
    }
}

struct FailingLatest;

impl LatestVersionQuery for FailingLatest {
    fn latest_version(&self) -> Result<String, RuntimeError> {
        Err(RuntimeError::LatestVersion("offline".to_owned()))
    }
}

fn yt_dlp_component() -> Component {
    let mut component = component(Distribution::Bundled);
    component.id = "yt-dlp".to_owned();
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .mirrors
        .clear();
    component
}

fn seed_slot(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("slot parent")).expect("create slot");
    fs::write(path, bytes).expect("seed slot");
}

#[test]
fn yt_dlp_update_download_failure_falls_back_to_bundled() {
    let temp = TestDir::new("update-fallback");
    let transport = MockTransport::default();
    transport.fail("network offline");
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let component = yt_dlp_component();
    let updater = YtDlpUpdater::new(&manager, &component, PLATFORM);
    seed_slot(&updater.bundled_path().expect("bundled path"), PAYLOAD);
    let outcome = updater.update(&StaticLatest("2.0.0")).expect("fallback");
    assert_eq!(outcome.action, YtDlpUpdateAction::FellBackToBundled);
    assert_eq!(outcome.active.slot, YtDlpSlot::Bundled);
    assert_eq!(outcome.active.version, "1.2.3");
    assert!(outcome.warnings[0].contains("bundled"));
}

#[test]
fn yt_dlp_latest_query_failure_falls_back_to_bundled() {
    let temp = TestDir::new("query-fallback");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let component = yt_dlp_component();
    let updater = YtDlpUpdater::new(&manager, &component, PLATFORM);
    seed_slot(&updater.bundled_path().expect("bundled path"), PAYLOAD);
    let outcome = updater.update(&FailingLatest).expect("fallback");
    assert_eq!(outcome.action, YtDlpUpdateAction::FellBackToBundled);
    assert_eq!(outcome.active.slot, YtDlpSlot::Bundled);
    assert!(transport.gets().is_empty());
}

#[test]
fn yt_dlp_successful_update_uses_updated_slot() {
    let temp = TestDir::new("update-success");
    let transport = MockTransport::default();
    transport.respond(HttpResponse::new(200, b"new yt-dlp".as_slice()));
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = yt_dlp_component();
    component.platforms.get_mut(PLATFORM).expect("asset").sha256 = UNPINNED_SHA256.to_owned();
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .size_bytes = 0;
    let updater = YtDlpUpdater::new(&manager, &component, PLATFORM);
    seed_slot(&updater.bundled_path().expect("bundled path"), PAYLOAD);
    let outcome = updater.update(&StaticLatest("2.0.0")).expect("update");
    assert_eq!(outcome.action, YtDlpUpdateAction::Updated);
    assert_eq!(outcome.active.slot, YtDlpSlot::Updated);
    assert_eq!(outcome.active.version, "2.0.0");
    assert_eq!(
        fs::read(outcome.active.path).expect("updated"),
        b"new yt-dlp"
    );
}

#[test]
fn corrupted_updated_slot_automatically_falls_back_to_bundled() {
    let temp = TestDir::new("corrupted-updated");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = yt_dlp_component();
    component
        .platforms
        .get_mut(PLATFORM)
        .expect("asset")
        .size_bytes = 0;
    let updater = YtDlpUpdater::new(&manager, &component, PLATFORM);
    seed_slot(&updater.bundled_path().expect("bundled path"), PAYLOAD);
    let updated = updater.updated_path().expect("updated path");
    seed_slot(&updated, b"tampered");
    let active = updater.active_binary().expect("bundled fallback");
    assert_eq!(active.slot, YtDlpSlot::Bundled);
    assert_eq!(active.path, updater.bundled_path().expect("bundled path"));
}

#[test]
fn unsafe_component_ids_cannot_escape_private_root() {
    let temp = TestDir::new("unsafe-id");
    let transport = MockTransport::default();
    let probe = MockVersionProbe::default();
    let verifier = Sha256HashVerifier;
    let manager = RuntimeManager::new(temp.path(), &transport, &probe, &verifier);
    let mut component = component(Distribution::Download);
    component.id = "../escape".to_owned();
    assert!(matches!(
        manager.installed_path(&component, PLATFORM),
        Err(RuntimeError::InvalidComponentId(_))
    ));
}
