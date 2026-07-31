#![cfg(feature = "cli")]

#[allow(dead_code)]
#[path = "../src/bin/nexora/tooling.rs"]
pub mod commands;

use commands::{
    inspect_app_selection, inspect_build_plans, inspect_latest_dmg_aliases,
    inspect_release_artifacts, inspect_signing_key, validate_display_name, write_bundle_info,
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
        Self { root }
    }

    fn config(&self) -> PathBuf {
        self.root.join("nexora.toml")
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
signing = "ad_hoc"
notarize = false
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
