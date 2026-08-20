use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use configuration::LayeredConfigLoader;
use serde::Deserialize;
use updater::{RELEASE_METADATA_FILE_NAME, load_release_metadata_from_directory};

#[path = "../src/config/path.rs"]
mod config_path;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const APP_NAME: &str = "bundle-config-test";

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ValueSettings {
    value: String,
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "nexora-bundle-config-{label}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn development_manifest(&self, value: &str) -> PathBuf {
        let manifest = self.root.join("workspace/apps/desktop");
        fs::create_dir_all(self.root.join("workspace/config")).unwrap();
        fs::create_dir_all(&manifest).unwrap();
        fs::write(
            self.root
                .join("workspace/config")
                .join(format!("{APP_NAME}.toml")),
            format!("value = \"{value}\"\n"),
        )
        .unwrap();
        manifest
    }

    fn release(&self, resource_directory: &Path) -> updater::LoadedApplicationReleaseMetadata {
        fs::create_dir_all(resource_directory).unwrap();
        fs::write(
            resource_directory.join(RELEASE_METADATA_FILE_NAME),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "app_key": "desktop",
                "app_id": "com.example.desktop",
                "display_name": "Desktop",
                "package": APP_NAME,
                "version": "1.2.3",
                "build_number": 7,
                "channel": "stable",
                "target": "aarch64-apple-darwin",
                "notes": null,
            }))
            .unwrap(),
        )
        .unwrap();
        load_release_metadata_from_directory(resource_directory)
            .unwrap()
            .unwrap()
    }

    fn bundle_config(&self, resource_directory: &Path, value: &str) -> PathBuf {
        let path = resource_directory
            .join("config")
            .join(format!("{APP_NAME}.toml"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("value = \"{value}\"\n")).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        _ = fs::remove_dir_all(&self.root);
    }
}

fn resolve(
    explicit_path: Option<PathBuf>,
    arguments: impl IntoIterator<Item = OsString>,
    manifest_directory: &Path,
    release: updater::LoadedApplicationReleaseMetadata,
) -> PathBuf {
    config_path::resolve_with_release_loader(
        explicit_path,
        arguments,
        APP_NAME,
        manifest_directory.to_str().unwrap(),
        || Ok(Some(release)),
    )
    .unwrap()
}

fn load(path: &Path) -> Result<ValueSettings, configuration::ConfigurationError> {
    LayeredConfigLoader::new()
        .with_required_file(path)
        .without_environment()
        .load()
}

#[test]
fn explicit_path_has_priority_over_bundle_configuration() {
    let fixture = Fixture::new("explicit");
    let manifest = fixture.development_manifest("development");
    let resources = fixture.root.join("Nexora.app/Contents/Resources");
    let release = fixture.release(&resources);
    fixture.bundle_config(&resources, "bundle");
    let explicit = fixture.root.join("explicit.toml");
    fs::write(&explicit, "value = \"explicit\"\n").unwrap();

    let selected = resolve(
        Some(explicit.clone()),
        [OsString::from("desktop")],
        &manifest,
        release,
    );

    assert_eq!(selected, explicit);
    assert_eq!(load(&selected).unwrap().value, "explicit");
}

#[test]
fn first_user_argument_has_priority_over_bundle_configuration() {
    let fixture = Fixture::new("argument");
    let manifest = fixture.development_manifest("development");
    let resources = fixture.root.join("Nexora.app/Contents/Resources");
    let release = fixture.release(&resources);
    fixture.bundle_config(&resources, "bundle");
    let positional = fixture.root.join("positional.toml");
    fs::write(&positional, "value = \"positional\"\n").unwrap();

    let selected = resolve(
        None,
        [
            OsString::from("desktop"),
            positional.clone().into_os_string(),
        ],
        &manifest,
        release,
    );

    assert_eq!(selected, positional);
    assert_eq!(load(&selected).unwrap().value, "positional");
}

#[test]
fn macos_bundle_uses_resources_config_directory() {
    let fixture = Fixture::new("macos");
    let manifest = fixture.development_manifest("development");
    let resources = fixture.root.join("Nexora.app/Contents/Resources");
    let release = fixture.release(&resources);
    let bundled = fixture.bundle_config(&resources, "stable");

    let selected = resolve(None, [OsString::from("desktop")], &manifest, release);

    assert_eq!(selected, bundled);
    assert_eq!(load(&selected).unwrap().value, "stable");
}

#[test]
fn windows_bundle_uses_executable_sibling_config_directory() {
    let fixture = Fixture::new("windows");
    let manifest = fixture.development_manifest("development");
    let install_directory = fixture.root.join("installed");
    let release = fixture.release(&install_directory);
    let bundled = fixture.bundle_config(&install_directory, "stable");

    let selected = resolve(None, [OsString::from("desktop.exe")], &manifest, release);

    assert_eq!(selected, bundled);
    assert_eq!(load(&selected).unwrap().value, "stable");
}

#[test]
fn formal_bundle_precedes_development_workspace_configuration() {
    let fixture = Fixture::new("bundle-priority");
    let manifest = fixture.development_manifest("nightly-development");
    let resources = fixture.root.join("Stable.app/Contents/Resources");
    let release = fixture.release(&resources);
    let bundled = fixture.bundle_config(&resources, "stable-bundle");

    let selected = resolve(None, [OsString::from("desktop")], &manifest, release);

    assert_eq!(selected, bundled);
    assert_eq!(load(&selected).unwrap().value, "stable-bundle");
}

#[test]
fn missing_or_invalid_formal_bundle_config_fails_without_development_fallback() {
    let fixture = Fixture::new("bundle-failure");
    let manifest = fixture.development_manifest("valid-development");

    let missing_resources = fixture.root.join("Missing.app/Contents/Resources");
    let missing_release = fixture.release(&missing_resources);
    let selected = resolve(
        None,
        [OsString::from("desktop")],
        &manifest,
        missing_release,
    );
    assert_eq!(
        selected,
        missing_resources
            .join("config")
            .join(format!("{APP_NAME}.toml"))
    );
    assert!(load(&selected).is_err());

    let invalid_resources = fixture.root.join("Invalid.app/Contents/Resources");
    let invalid_release = fixture.release(&invalid_resources);
    let invalid = fixture.bundle_config(&invalid_resources, "temporary");
    fs::write(&invalid, "value = [\n").unwrap();
    let selected = resolve(
        None,
        [OsString::from("desktop")],
        &manifest,
        invalid_release,
    );
    assert_eq!(selected, invalid);
    assert!(load(&selected).is_err());
}

#[test]
fn updater_health_arguments_are_ignored_before_loading_bundle_configuration() {
    let fixture = Fixture::new("health");
    let manifest = fixture.development_manifest("development");
    let resources = fixture.root.join("Nexora.app/Contents/Resources");
    let release = fixture.release(&resources);
    let bundled = fixture.bundle_config(&resources, "bundle");
    let arguments = [
        OsString::from("desktop"),
        OsString::from("--nexora-updater-health-session"),
        OsString::from("session"),
        OsString::from("--nexora-updater-health-file"),
        OsString::from("health.json"),
    ];

    let selected = resolve(None, arguments, &manifest, release);

    assert_eq!(selected, bundled);
    assert_eq!(load(&selected).unwrap().value, "bundle");
}

#[test]
fn development_run_without_release_metadata_uses_workspace_config() {
    let fixture = Fixture::new("development");
    let manifest = fixture.development_manifest("development");

    let selected = config_path::resolve_with_release_loader(
        None,
        [OsString::from("desktop")],
        APP_NAME,
        manifest.to_str().unwrap(),
        || Ok(None),
    )
    .unwrap();

    assert_eq!(
        selected,
        fixture
            .root
            .join("workspace/config")
            .join(format!("{APP_NAME}.toml"))
    );
    assert_eq!(load(&selected).unwrap().value, "development");
}

#[test]
fn invalid_release_metadata_fails_before_development_fallback() {
    let fixture = Fixture::new("invalid-release");
    let manifest = fixture.development_manifest("development");
    let resources = fixture.root.join("Nexora.app/Contents/Resources");
    fs::create_dir_all(&resources).unwrap();
    fs::write(resources.join(RELEASE_METADATA_FILE_NAME), b"not json").unwrap();

    let error = config_path::resolve_with_release_loader(
        None,
        [OsString::from("desktop")],
        APP_NAME,
        manifest.to_str().unwrap(),
        || load_release_metadata_from_directory(&resources),
    )
    .unwrap_err();

    assert!(error.to_string().contains("元数据无效"));
}
