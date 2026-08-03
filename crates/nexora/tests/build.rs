#![cfg(feature = "cli")]

#[allow(dead_code)]
#[path = "../src/bin/nexora/tooling.rs"]
pub mod commands;

use commands::{
    inspect_app_selection, inspect_build_datetime_number, inspect_build_plans,
    inspect_latest_dmg_aliases, inspect_prepare_release_receipt, inspect_release_artifacts,
    inspect_signing_key, inspect_windows_installer_sources, validate_display_name,
    write_bundle_icon, write_bundle_info,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str, apps: &str) -> Self {
        let root = env::temp_dir().join(format!(
            "nexora-build-{name}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("nexora.toml"),
            format!(
                r#"schema_version = 1

[publish.targets.rustfs]
provider = "s3"
endpoint = "http://127.0.0.1:9000"
bucket = "releases"
region = "us-east-1"
force_path_style = true
public_base_url = "http://127.0.0.1:9000/releases"
allow_insecure_http = true

{apps}
"#
            ),
        )
        .unwrap();
        let packages = apps
            .lines()
            .filter_map(|line| {
                line.strip_prefix("package = \"")
                    .and_then(|value| value.strip_suffix('"'))
            })
            .collect::<Vec<_>>();
        fs::write(
            root.join("Cargo.toml"),
            r#"[workspace]
resolver = "3"
members = ["packages/*"]

[workspace.package]
version = "9.8.7"
"#,
        )
        .unwrap();
        for package in packages {
            write_test_package(&root, package, "version = \"1.2.3\"");
        }
        for app_key in apps.lines().filter_map(|line| {
            line.strip_prefix("[apps.")
                .and_then(|line| line.strip_suffix(']'))
                .filter(|line| !line.contains('.'))
        }) {
            write_brand_assets(&root, app_key);
        }
        Self { root }
    }

    fn config(&self) -> PathBuf {
        self.root.join("nexora.toml")
    }
}

fn write_test_package(root: &Path, package: &str, version: &str) {
    let directory = root.join("packages").join(package);
    fs::create_dir_all(directory.join("src")).unwrap();
    fs::write(
        directory.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{package}\"\n{version}\nedition = \"2024\"\n\n[lib]\npath = \"src/lib.rs\"\n"
        ),
    )
    .unwrap();
    fs::write(directory.join("src/lib.rs"), "").unwrap();
}

fn write_brand_assets(root: &Path, app_key: &str) {
    let directory = root.join("assets/logos").join(app_key);
    fs::create_dir_all(&directory).unwrap();
    let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/scaffold/assets/logos");
    for name in [
        "logo-icon-16.png",
        "logo-icon-24.png",
        "logo-icon-32.png",
        "logo-icon-48.png",
        "logo-icon-64.png",
        "logo-icon-128.png",
        "logo-icon-256.png",
        "logo-icon-512.png",
        "logo-icon-1024.png",
        "logo-icon-source.png",
        "logo-icon.icns",
        "logo-icon.ico",
    ] {
        fs::copy(template.join(name), directory.join(name)).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn app_config(key: &str, package: &str, display_name: &str) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::SigningKey;
    let public_key = STANDARD.encode(
        SigningKey::from_bytes(&[7_u8; 32])
            .verifying_key()
            .to_bytes(),
    );
    format!(
        r#"[apps.{key}]
package = "{package}"
app_id = "com.example.{key}"
display_name = "{display_name}"
publish_target = "rustfs"
object_prefix = "e2e"

[apps.{key}.branding]
application_logo = "assets/logos/{key}/logo-icon-128.png"
icon_source = "assets/logos/{key}/logo-icon-source.png"
managed = true

[apps.{key}.release]
channel = "stable"
version = "1.2.3"
build_number = 7
minimum_supported_version = "0.0.0"

[apps.{key}.updater]
enabled = true
feed_url = "http://127.0.0.1:9000/releases/e2e/{key}/stable/latest.json"
channels = ["stable"]
trusted_public_keys = ["main:ed25519:{public_key}"]
signing_key_env = "TEST_SIGNING_KEY"
check_interval = "15m"
check_jitter = "1m"
offline_grace_period = "24h"
mandatory_restart_delay = "60s"
health_timeout = "20s"

[apps.{key}.targets]
required = ["aarch64-apple-darwin"]

[apps.{key}.platforms.macos]
icon = "assets/logos/{key}/logo-icon.icns"
signing = "ad_hoc"
notarize = false

[apps.{key}.platforms.windows]
icon = "assets/logos/{key}/logo-icon.ico"

[apps.{key}.platforms.linux]
icons = [
    "assets/logos/{key}/logo-icon-16.png",
    "assets/logos/{key}/logo-icon-24.png",
    "assets/logos/{key}/logo-icon-32.png",
    "assets/logos/{key}/logo-icon-48.png",
    "assets/logos/{key}/logo-icon-64.png",
    "assets/logos/{key}/logo-icon-128.png",
    "assets/logos/{key}/logo-icon-256.png",
    "assets/logos/{key}/logo-icon-512.png",
    "assets/logos/{key}/logo-icon-1024.png",
]
"#
    )
}

fn write_artifacts(fixture: &Fixture, target: &str, zip: bool, dmg: bool) {
    use sha2::{Digest as _, Sha256};
    let directory = fixture.root.join("dist/one/stable/1.2.3/7").join(target);
    fs::create_dir_all(&directory).unwrap();
    let arch = if target.starts_with("aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let mut entries = Vec::new();
    if zip {
        let name = format!("package-one-1.2.3-7-{arch}.app.zip");
        fs::write(directory.join(&name), b"zip").unwrap();
        entries.push(serde_json::json!({
            "kind": "macos_app_zip",
            "file_name": name,
            "sha256": format!("{:x}", Sha256::digest(b"zip")),
            "size": 3
        }));
    }
    if dmg {
        let name = format!("package-one-1.2.3-7-{arch}.dmg");
        fs::write(directory.join(&name), b"dmg").unwrap();
        entries.push(serde_json::json!({
            "kind": "macos_dmg",
            "file_name": name,
            "sha256": format!("{:x}", Sha256::digest(b"dmg")),
            "size": 3
        }));
    }
    fs::write(
        directory.join("artifact.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "app_id": "com.example.one",
            "channel": "stable",
            "version": "1.2.3",
            "build_number": 7,
            "target": target,
            "artifacts": entries
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        fixture.root.join("dist/one/stable/release.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "app_key": "one",
            "package": "package-one",
            "channel": "stable",
            "version": "1.2.3",
            "build_number": 7,
            "version_source": "literal",
            "build_number_source": "literal",
            "created_at": 1,
            "targets": if target.starts_with("x86_64") {
                vec!["aarch64-apple-darwin", "x86_64-apple-darwin"]
            } else {
                vec!["aarch64-apple-darwin"]
            }
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_complete_artifacts_for_identity(
    fixture: &Fixture,
    target: &str,
    version: &str,
    build_number: u64,
) {
    use sha2::{Digest as _, Sha256};
    let directory = fixture
        .root
        .join("dist/one/stable")
        .join(version)
        .join(build_number.to_string())
        .join(target);
    fs::create_dir_all(&directory).unwrap();
    let arch = if target.starts_with("aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let zip_name = format!("package-one-{version}-{build_number}-{arch}.app.zip");
    let dmg_name = format!("package-one-{version}-{build_number}-{arch}.dmg");
    fs::write(directory.join(&zip_name), b"zip").unwrap();
    fs::write(directory.join(&dmg_name), b"dmg").unwrap();
    fs::write(
        directory.join("artifact.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "app_id": "com.example.one",
            "channel": "stable",
            "version": version,
            "build_number": build_number,
            "target": target,
            "artifacts": [
                {
                    "kind": "macos_app_zip",
                    "file_name": zip_name,
                    "sha256": format!("{:x}", Sha256::digest(b"zip")),
                    "size": 3
                },
                {
                    "kind": "macos_dmg",
                    "file_name": dmg_name,
                    "sha256": format!("{:x}", Sha256::digest(b"dmg")),
                    "size": 3
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_windows_artifacts(fixture: &Fixture, include_setup: bool, include_zip: bool) {
    use sha2::{Digest as _, Sha256};
    let target = "x86_64-pc-windows-msvc";
    let directory = fixture.root.join("dist/one/stable/1.2.3/7").join(target);
    fs::create_dir_all(&directory).unwrap();
    let mut entries = Vec::new();
    for (include, kind, name, bytes) in [
        (
            include_setup,
            "windows_setup_exe",
            "package-one-1.2.3-7-x86_64.setup.exe".to_owned(),
            b"setup".as_slice(),
        ),
        (
            include_zip,
            "windows_update_zip",
            "package-one-1.2.3-7-x86_64.windows.zip".to_owned(),
            b"zip".as_slice(),
        ),
    ] {
        if include {
            fs::write(directory.join(&name), bytes).unwrap();
            entries.push(serde_json::json!({
                "kind": kind,
                "file_name": name,
                "sha256": format!("{:x}", Sha256::digest(bytes)),
                "size": bytes.len()
            }));
        }
    }
    fs::write(
        directory.join("artifact.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "app_id": "com.example.one",
            "channel": "stable",
            "version": "1.2.3",
            "build_number": 7,
            "target": target,
            "artifacts": entries
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        fixture.root.join("dist/one/stable/release.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "app_key": "one",
            "package": "package-one",
            "channel": "stable",
            "version": "1.2.3",
            "build_number": 7,
            "version_source": "literal",
            "build_number_source": "literal",
            "created_at": 1,
            "targets": [target]
        }))
        .unwrap(),
    )
    .unwrap();
}

fn with_windows_target(config: String) -> String {
    config
        .replace(
            "required = [\"aarch64-apple-darwin\"]",
            "required = [\"x86_64-pc-windows-msvc\"]",
        )
        .replace(
            "icon = \"assets/logos/one/logo-icon.ico\"",
            r#"icon = "assets/logos/one/logo-icon.ico"
installer = "nsis"
install_scope = "user"
publisher = "Nexora Test Publisher"
signing = "authenticode"
signing_thumbprint = "00112233445566778899AABBCCDDEEFF00112233"
timestamp_url = "http://timestamp.example.test"
desktop_shortcut_default = false
launch_after_install_default = true
minimum_windows_build = 19045"#,
        )
}

#[test]
fn package_and_display_name_are_separate() {
    let fixture = Fixture::new(
        "identity",
        &app_config("desktop", "technical-package", "产品名称"),
    );
    let plans = inspect_build_plans(fixture.config(), "desktop").unwrap();
    let plan = &plans[0];

    assert_eq!(plan["package"], "technical-package");
    assert_eq!(plan["display_name"], "产品名称");
    assert!(
        plan["app_path"]
            .as_str()
            .unwrap()
            .ends_with("target/aarch64-apple-darwin/release/bundle/osx/technical-package.app")
    );
    assert!(!plan["app_path"].as_str().unwrap().contains("产品名称"));
}

#[test]
fn technical_artifact_names_include_package_version_build_and_arch() {
    let fixture = Fixture::new(
        "artifacts",
        &app_config("desktop", "technical-package", "产品名称"),
    );
    let plan = inspect_build_plans(fixture.config(), "desktop")
        .unwrap()
        .remove(0);

    assert!(
        plan["app_zip_path"]
            .as_str()
            .unwrap()
            .ends_with("technical-package-1.2.3-7-aarch64.app.zip")
    );
    assert!(
        plan["dmg_path"]
            .as_str()
            .unwrap()
            .ends_with("technical-package-1.2.3-7-aarch64.dmg")
    );
}

#[test]
fn cargo_package_version_expression_uses_selected_package() {
    let apps = format!(
        "{}\n{}",
        app_config("one", "package-one", "应用一")
            .replace("version = \"1.2.3\"", "version = \"${CARGO_PKG_VERSION}\""),
        app_config("two", "package-two", "应用二")
            .replace("version = \"1.2.3\"", "version = \"${CARGO_PKG_VERSION}\"")
    );
    let fixture = Fixture::new("selected-package-version", &apps);
    write_test_package(&fixture.root, "package-two", "version = \"2.3.4\"");

    let plan = inspect_build_plans(fixture.config(), "two")
        .unwrap()
        .remove(0);

    assert_eq!(plan["version"], "2.3.4");
    assert_eq!(plan["version_source"], "cargo_pkg_version");
}

#[test]
fn cargo_package_version_expression_supports_workspace_version() {
    let app = app_config("one", "package-one", "应用一")
        .replace("version = \"1.2.3\"", "version = \"${CARGO_PKG_VERSION}\"");
    let fixture = Fixture::new("workspace-package-version", &app);
    write_test_package(&fixture.root, "package-one", "version.workspace = true");

    let plan = inspect_build_plans(fixture.config(), "one")
        .unwrap()
        .remove(0);

    assert_eq!(plan["version"], "9.8.7");
    assert_ne!(plan["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn unknown_release_expressions_are_rejected() {
    for invalid in [
        app_config("one", "package-one", "应用一")
            .replace("version = \"1.2.3\"", "version = \"${CARGO_VERSION}\""),
        app_config("one", "package-one", "应用一").replace(
            "version = \"1.2.3\"",
            "version = \"1.${CARGO_PKG_VERSION}\"",
        ),
        app_config("one", "package-one", "应用一")
            .replace("build_number = 7", "build_number = \"${UNKNOWN}\""),
    ] {
        let fixture = Fixture::new("unknown-expression", &invalid);
        assert!(inspect_build_plans(fixture.config(), "one").is_err());
    }
}

#[test]
fn build_datetime_is_twelve_digit_utc_and_strictly_increasing() {
    let current = inspect_build_datetime_number(1_785_765_975, None).unwrap();
    assert_eq!(current, 260_803_140_615);
    assert_eq!(current.to_string().len(), 12);
    assert_eq!(
        inspect_build_datetime_number(1_785_765_975, Some(current)).unwrap(),
        current + 1
    );
    assert_eq!(
        inspect_build_datetime_number(1_785_765_974, Some(current + 10)).unwrap(),
        current + 11
    );
    assert!(inspect_build_datetime_number(1_785_765_975, Some(u64::MAX)).is_err());
}

#[test]
fn dynamic_receipt_is_reused_until_all_targets_are_complete() {
    let app = app_config("one", "package-one", "应用一")
        .replace("version = \"1.2.3\"", "version = \"${CARGO_PKG_VERSION}\"")
        .replace("build_number = 7", "build_number = \"${BUILD_DATETIME}\"")
        .replace(
            "required = [\"aarch64-apple-darwin\"]",
            "required = [\"aarch64-apple-darwin\", \"x86_64-apple-darwin\"]",
        );
    let fixture = Fixture::new("dynamic-receipt", &app);

    let first = inspect_prepare_release_receipt(fixture.config(), "one").unwrap();
    let retry = inspect_prepare_release_receipt(fixture.config(), "one").unwrap();
    assert_eq!(first["build_number"], retry["build_number"]);
    assert_eq!(
        first["targets"],
        serde_json::json!(["aarch64-apple-darwin", "x86_64-apple-darwin"])
    );

    let build_number = first["build_number"].as_u64().unwrap();
    write_complete_artifacts_for_identity(&fixture, "aarch64-apple-darwin", "1.2.3", build_number);
    let partial_retry = inspect_prepare_release_receipt(fixture.config(), "one").unwrap();
    assert_eq!(first["build_number"], partial_retry["build_number"]);

    write_complete_artifacts_for_identity(&fixture, "x86_64-apple-darwin", "1.2.3", build_number);
    let next = inspect_prepare_release_receipt(fixture.config(), "one").unwrap();
    assert!(next["build_number"].as_u64().unwrap() > build_number);
    assert_eq!(next["version_source"], "cargo_pkg_version");
    assert_eq!(next["build_number_source"], "build_datetime");
}

#[test]
fn corrupt_release_receipt_fails_before_identity_is_replaced() {
    let app = app_config("one", "package-one", "应用一")
        .replace("build_number = 7", "build_number = \"${BUILD_DATETIME}\"");
    let fixture = Fixture::new("corrupt-receipt", &app);
    fs::create_dir_all(fixture.root.join("dist/one/stable")).unwrap();
    let receipt = fixture.root.join("dist/one/stable/release.json");
    fs::write(&receipt, "{broken").unwrap();

    let error = inspect_prepare_release_receipt(fixture.config(), "one").unwrap_err();

    assert!(error.to_string().contains("release receipt"));
    assert_eq!(fs::read_to_string(receipt).unwrap(), "{broken");
}

#[test]
fn windows_build_plan_uses_setup_and_update_zip_without_msi() {
    let fixture = Fixture::new(
        "windows-plan",
        &with_windows_target(app_config("one", "package-one", "Application One")),
    );
    let plans = inspect_build_plans(fixture.config(), "one").unwrap();
    let plan = &plans[0];

    assert_eq!(plan["platform"], "Windows");
    assert!(
        plan["app_zip_path"]
            .as_str()
            .unwrap()
            .ends_with("package-one-1.2.3-7-x86_64.windows.zip")
    );
    assert!(
        plan["setup_path"]
            .as_str()
            .unwrap()
            .ends_with("package-one-1.2.3-7-x86_64.setup.exe")
    );
    assert!(plan.get("msi_path").is_none());
}

#[test]
fn windows_installer_sources_keep_nsis_boundaries() {
    let fixture = Fixture::new(
        "windows-installer-source",
        &with_windows_target(app_config("one", "package-one", "Application One")),
    );
    let sources = inspect_windows_installer_sources(fixture.config(), "one").unwrap();
    let script = sources["nsis_script"].as_str().unwrap();
    let updater_config = &sources["updater_config"];

    assert_eq!(sources["file_version"], "1.2.3.7");
    assert!(script.contains("RequestExecutionLevel user"));
    assert!(script.contains("$LOCALAPPDATA\\Programs\\com.example.one"));
    assert!(script.contains("WriteRegStr SHCTX"));
    assert!(script.contains("ReadRegDWORD $1"));
    assert!(script.contains("package-one-updater.exe"));
    assert!(script.contains("nexora-updater.json"));
    assert!(!script.contains(".windows.zip"));
    assert!(!script.contains("MsiPackage"));
    assert!(!script.contains("WixToolset"));
    assert_eq!(
        updater_config["expected_windows_signer_thumbprint"],
        "00112233445566778899AABBCCDDEEFF00112233"
    );
    assert_eq!(
        updater_config["expected_windows_publisher"],
        "Nexora Test Publisher"
    );
}

#[test]
fn invalid_display_names_are_rejected() {
    for value in [
        "",
        ".",
        "..",
        "bad/name",
        "bad\\name",
        "bad\0name",
        "bad\nname",
    ] {
        assert!(validate_display_name(value).is_err(), "{value:?}");
    }
    validate_display_name("macOS 更新程序示例").unwrap();
}

#[test]
fn single_app_is_selected_automatically() {
    let fixture = Fixture::new("single", &app_config("one", "package-one", "应用一"));
    assert_eq!(
        inspect_app_selection(fixture.config(), None, false).unwrap(),
        vec!["one"]
    );
}

#[test]
fn multiple_apps_are_not_guessed_non_interactively() {
    let apps = format!(
        "{}\n{}",
        app_config("one", "package-one", "应用一"),
        app_config("two", "package-two", "应用二")
    );
    let fixture = Fixture::new("multiple", &apps);
    let error = inspect_app_selection(fixture.config(), None, false).unwrap_err();

    assert!(error.to_string().contains("--app"));
    assert!(error.to_string().contains("--all"));
}

#[test]
fn explicit_app_and_all_skip_guessing() {
    let apps = format!(
        "{}\n{}",
        app_config("one", "package-one", "应用一"),
        app_config("two", "package-two", "应用二")
    );
    let fixture = Fixture::new("explicit", &apps);
    assert_eq!(
        inspect_app_selection(fixture.config(), Some("two"), false).unwrap(),
        vec!["two"]
    );
    assert_eq!(
        inspect_app_selection(fixture.config(), None, true).unwrap(),
        vec!["one", "two"]
    );
}

#[test]
fn release_configuration_is_required_and_validated() {
    let missing = app_config("one", "package-one", "应用一").replace(
        "[apps.one.release]\nchannel = \"stable\"\nversion = \"1.2.3\"\nbuild_number = 7\nminimum_supported_version = \"0.0.0\"\n\n",
        "",
    );
    let fixture = Fixture::new("missing-release", &missing);
    assert!(
        inspect_build_plans(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("release")
    );

    let invalid = app_config("one", "package-one", "应用一")
        .replace("version = \"1.2.3\"", "version = \"not-semver\"");
    let fixture = Fixture::new("invalid-version", &invalid);
    assert!(
        inspect_build_plans(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("SemVer")
    );

    let invalid =
        app_config("one", "package-one", "应用一").replace("build_number = 7", "build_number = 0");
    let fixture = Fixture::new("invalid-build", &invalid);
    assert!(
        inspect_build_plans(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("大于 0")
    );
}

#[test]
fn release_channel_must_be_declared_by_updater() {
    let invalid = app_config("one", "package-one", "应用一")
        .replace("channel = \"stable\"", "channel = \"beta\"");
    let fixture = Fixture::new("channel", &invalid);
    assert!(
        inspect_build_plans(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("updater.channels")
    );
}

#[test]
fn info_plist_contains_identity_display_name_version_and_build() {
    let fixture = Fixture::new("plist", &app_config("one", "package-one", "应用一"));
    let app = fixture.root.join("Package.app");
    let contents = app.join("Contents");
    fs::create_dir_all(&contents).unwrap();
    fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>old</string>
<key>CFBundleDisplayName</key><string>old</string>
<key>CFBundleName</key><string>old</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
</dict></plist>"#,
    )
    .unwrap();

    write_bundle_info(&app, "com.example.one", "macOS 更新程序示例", "1.2.3", 7).unwrap();
    let plist = fs::read_to_string(contents.join("Info.plist")).unwrap();
    for expected in ["com.example.one", "macOS 更新程序示例", "1.2.3", "7"] {
        assert!(plist.contains(expected));
    }
}

#[test]
fn macos_bundle_installs_configured_icns_and_updates_info_plist() {
    let fixture = Fixture::new("bundle-icon", &app_config("one", "package-one", "应用一"));
    let app = fixture.root.join("Package.app");
    let contents = app.join("Contents");
    fs::create_dir_all(&contents).unwrap();
    fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleIconFile</key><string>old.icns</string></dict></plist>"#,
    )
    .unwrap();
    let icon = fixture.root.join("assets/logos/one/logo-icon.icns");

    write_bundle_icon(&app, &icon).unwrap();

    assert_eq!(
        fs::read(contents.join("Resources/logo-icon.icns")).unwrap(),
        fs::read(icon).unwrap()
    );
    assert!(
        fs::read_to_string(contents.join("Info.plist"))
            .unwrap()
            .contains("logo-icon.icns")
    );
}

#[test]
fn missing_selected_app_icon_fails_before_building() {
    let fixture = Fixture::new("missing-icon", &app_config("one", "package-one", "应用一"));
    fs::remove_file(fixture.root.join("assets/logos/one/logo-icon.icns")).unwrap();

    let error = inspect_build_plans(fixture.config(), "one").unwrap_err();

    assert!(error.to_string().contains("macOS ICNS 不存在"));
}

#[test]
fn artifact_path_is_isolated_by_app_channel_version_build_and_target() {
    let fixture = Fixture::new("layout", &app_config("one", "package-one", "应用一"));
    let plan = inspect_build_plans(fixture.config(), "one")
        .unwrap()
        .remove(0);
    let expected = Path::new("dist/one/stable/1.2.3/7/aarch64-apple-darwin/artifact.json");
    assert!(
        plan["artifact_path"]
            .as_str()
            .unwrap()
            .ends_with(expected.to_str().unwrap())
    );
}

#[test]
fn publish_artifact_validation_requires_zip_and_dmg() {
    let fixture = Fixture::new(
        "artifact-validation",
        &app_config("one", "package-one", "应用一"),
    );
    write_artifacts(&fixture, "aarch64-apple-darwin", true, true);
    assert_eq!(
        inspect_release_artifacts(fixture.config(), "one").unwrap(),
        vec!["macos_app_zip", "macos_dmg"]
    );

    write_artifacts(&fixture, "aarch64-apple-darwin", true, false);
    assert!(
        inspect_release_artifacts(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("macos_dmg")
    );

    write_artifacts(&fixture, "aarch64-apple-darwin", false, true);
    assert!(
        inspect_release_artifacts(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("macos_app_zip")
    );
}

#[test]
fn publish_artifact_validation_accepts_windows_setup_and_update_zip() {
    let fixture = Fixture::new(
        "windows-artifact-validation",
        &with_windows_target(app_config("one", "package-one", "Application One")),
    );
    write_windows_artifacts(&fixture, true, true);
    assert_eq!(
        inspect_release_artifacts(fixture.config(), "one").unwrap(),
        vec!["windows_setup_exe", "windows_update_zip"]
    );

    write_windows_artifacts(&fixture, true, false);
    assert!(
        inspect_release_artifacts(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("windows_update_zip")
    );
}

#[test]
fn latest_dmg_aliases_are_unambiguous() {
    let fixture = Fixture::new("single-alias", &app_config("one", "package-one", "应用一"));
    write_artifacts(&fixture, "aarch64-apple-darwin", true, true);
    assert_eq!(
        inspect_latest_dmg_aliases(fixture.config(), "one").unwrap(),
        vec![
            "e2e/one/stable/latest-aarch64.dmg",
            "e2e/one/stable/latest.dmg"
        ]
    );

    let apps = app_config("one", "package-one", "应用一").replace(
        "required = [\"aarch64-apple-darwin\"]",
        "required = [\"aarch64-apple-darwin\", \"x86_64-apple-darwin\"]",
    );
    let fixture = Fixture::new("multi-alias", &apps);
    write_artifacts(&fixture, "aarch64-apple-darwin", true, true);
    write_artifacts(&fixture, "x86_64-apple-darwin", true, true);
    assert_eq!(
        inspect_latest_dmg_aliases(fixture.config(), "one").unwrap(),
        vec![
            "e2e/one/stable/latest-aarch64.dmg",
            "e2e/one/stable/latest-x86_64.dmg"
        ]
    );
}

#[test]
fn signing_key_file_is_relative_and_must_match_trusted_key() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let apps = app_config("one", "package-one", "应用一").replace(
        "minimum_supported_version = \"0.0.0\"",
        "minimum_supported_version = \"0.0.0\"\nsigning_key_file = \".secrets/update.key\"",
    );
    let fixture = Fixture::new("signing-file", &apps);
    fs::create_dir_all(fixture.root.join(".secrets")).unwrap();
    fs::write(
        fixture.root.join(".secrets/update.key"),
        format!("main:ed25519:{}\n", STANDARD.encode([7_u8; 32])),
    )
    .unwrap();
    assert_eq!(
        inspect_signing_key(fixture.config(), "one").unwrap(),
        "main"
    );

    fs::write(
        fixture.root.join(".secrets/update.key"),
        format!("main:ed25519:{}\n", STANDARD.encode([8_u8; 32])),
    )
    .unwrap();
    assert!(
        inspect_signing_key(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("不匹配")
    );
}
