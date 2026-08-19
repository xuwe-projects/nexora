#[allow(dead_code)]
#[path = "../src/tooling.rs"]
pub mod commands;

#[cfg(not(windows))]
use commands::inspect_inno_path_definition;
use commands::{
    inspect_app_selection, inspect_build_datetime_number, inspect_build_dependency_guidance,
    inspect_build_plans, inspect_build_plans_for_channel, inspect_channel_artifact_keys,
    inspect_create_windows_update_zip, inspect_credential_selection,
    inspect_effective_publish_target, inspect_freeze_release_notes, inspect_inno_setup_requirement,
    inspect_prepare_release_receipt, inspect_publish_object_layout, inspect_release_artifacts,
    inspect_release_artifacts_for_channel, inspect_release_resources, inspect_release_selection,
    inspect_select_inno_setup_candidate, inspect_signing_key, inspect_windows_binary_link_args,
    inspect_windows_installer_sources, inspect_windows_resource_scripts, validate_display_name,
    write_bundle_icon, write_bundle_info, write_sha256_sidecar,
};
#[cfg(windows)]
use commands::{
    inspect_compile_windows_installer, inspect_compile_windows_resource_executables,
    inspect_inno_setup_compiler_version,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
static ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

struct EnvironmentGuard {
    original: Vec<(&'static str, Option<OsString>)>,
}

impl EnvironmentGuard {
    fn clear(names: &[&'static str]) -> Self {
        let original = names
            .iter()
            .map(|name| (*name, env::var_os(name)))
            .collect::<Vec<_>>();
        for name in names {
            unsafe { env::remove_var(name) };
        }
        Self { original }
    }

    fn set(name: &str, value: &str) {
        unsafe { env::set_var(name, value) };
    }

    fn unset(name: &str) {
        unsafe { env::remove_var(name) };
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        for (name, value) in &self.original {
            match value {
                Some(value) => unsafe { env::set_var(name, value) },
                None => unsafe { env::remove_var(name) },
            }
        }
    }
}

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
            fs::create_dir_all(root.join("config")).unwrap();
            fs::write(
                root.join("config").join(format!("{package}.toml")),
                "value = \"test\"\n",
            )
            .unwrap();
        }
        for app_key in apps.lines().filter_map(|line| {
            line.strip_prefix("[apps.")
                .and_then(|line| line.strip_suffix(']'))
                .filter(|line| !line.contains('.'))
        }) {
            write_brand_assets(&root, app_key);
            fs::create_dir_all(root.join("docs/releases")).unwrap();
            fs::write(
                root.join("docs/releases").join(format!("{app_key}.md")),
                format!("# {app_key} 更新日志\n"),
            )
            .unwrap();
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
notes = "docs/releases/{key}.md"

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

fn multi_channel_app_config(key: &str, package: &str, display_name: &str) -> String {
    app_config(key, package, display_name)
        .replace(
            &format!(
                "[apps.{key}.release]\nchannel = \"stable\"\nversion = \"1.2.3\"\nbuild_number = 7\nminimum_supported_version = \"0.0.0\"\nnotes = \"docs/releases/{key}.md\""
            ),
            &format!(
                "[apps.{key}.release]\ndefault_channel = \"nightly\"\nversion = \"1.2.3\"\nbuild_number = 7\nminimum_supported_version = \"0.0.0\"\nnotes = \"docs/releases/{key}.md\"\n\n[apps.{key}.release.channels.nightly]\n\n[apps.{key}.release.channels.beta]\nbuild_number = 8\nminimum_supported_version = \"1.0.0\"\nruntime_config = \"config/{package}-beta.toml\"\n\n[apps.{key}.release.channels.stable]"
            ),
        )
        .replace(
            &format!(
                "feed_url = \"http://127.0.0.1:9000/releases/e2e/{key}/stable/latest.json\"\nchannels = [\"stable\"]"
            ),
            "channels = [\"nightly\", \"beta\", \"stable\"]",
        )
}

fn runtime_config_sha256(fixture: &Fixture, package: &str) -> String {
    use sha2::{Digest as _, Sha256};
    format!(
        "{:x}",
        Sha256::digest(
            fs::read(fixture.root.join("config").join(format!("{package}.toml"))).unwrap()
        )
    )
}

fn write_artifacts(fixture: &Fixture, target: &str, zip: bool, dmg: bool) {
    use sha2::{Digest as _, Sha256};
    let runtime_hash = runtime_config_sha256(fixture, "package-one");
    let directory = fixture.root.join("dist/one/stable/1.2.3/7").join(target);
    fs::create_dir_all(&directory).unwrap();
    let arch = if target.starts_with("aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let mut entries = Vec::new();
    if zip {
        let name = format!("应用一-{arch}.app.zip");
        fs::write(directory.join(&name), b"zip").unwrap();
        entries.push(serde_json::json!({
            "kind": "macos_app_zip",
            "file_name": name,
            "sha256": format!("{:x}", Sha256::digest(b"zip")),
            "size": 3
        }));
    }
    if dmg {
        let name = format!("应用一-{arch}.dmg");
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
            "schema_version": 3,
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
            },
            "runtime_config_source": "config/package-one.toml",
            "runtime_config_sha256": runtime_hash,
            "updater_feed": "http://127.0.0.1:9000/releases/e2e/one/stable/latest.json",
            "notes_source": "docs/releases/one.md"
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
    let zip_name = format!("应用一-{arch}.app.zip");
    let dmg_name = format!("应用一-{arch}.dmg");
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

fn write_windows_artifacts(
    fixture: &Fixture,
    display_name: &str,
    include_setup: bool,
    include_zip: bool,
) {
    use sha2::{Digest as _, Sha256};
    let runtime_hash = runtime_config_sha256(fixture, "package-one");
    let target = "x86_64-pc-windows-msvc";
    let directory = fixture.root.join("dist/one/stable/1.2.3/7").join(target);
    fs::create_dir_all(&directory).unwrap();
    let mut entries = Vec::new();
    for (include, kind, name, bytes) in [
        (
            include_setup,
            "windows_setup_exe",
            format!("{display_name}-x86_64.exe"),
            b"setup".as_slice(),
        ),
        (
            include_zip,
            "windows_update_zip",
            format!("{display_name}-x86_64.windows.zip"),
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
            "schema_version": 3,
            "app_key": "one",
            "package": "package-one",
            "channel": "stable",
            "version": "1.2.3",
            "build_number": 7,
            "version_source": "literal",
            "build_number_source": "literal",
            "created_at": 1,
            "targets": [target],
            "runtime_config_source": "config/package-one.toml",
            "runtime_config_sha256": runtime_hash,
            "updater_feed": "http://127.0.0.1:9000/releases/e2e/one/stable/latest.json",
            "notes_source": "docs/releases/one.md"
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
publisher = "Nexora Test Publisher"
signing = "authenticode"
signing_thumbprint = "00112233445566778899AABBCCDDEEFF00112233"
timestamp_url = "http://timestamp.example.test"
desktop_shortcut_default = false
start_menu_shortcut_default = true
launch_after_install_default = true
minimum_windows_build = 15063"#,
        )
}

fn with_unsigned_windows_signing(config: String) -> String {
    with_windows_target(config).replace(
        "signing = \"authenticode\"\nsigning_thumbprint = \"00112233445566778899AABBCCDDEEFF00112233\"\ntimestamp_url = \"http://timestamp.example.test\"",
        "signing = \"none\"",
    )
}

#[cfg(windows)]
fn windows_host_config(config: String) -> String {
    let output = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let host = stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap();
    config.replace("x86_64-pc-windows-msvc", host)
}

#[cfg(windows)]
fn windows_version_info(path: &Path) -> serde_json::Value {
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$i=(Get-Item -LiteralPath $env:NEXORA_PE_SMOKE_PATH).VersionInfo; $i | Select-Object FileDescription,ProductName,InternalName,OriginalFilename | ConvertTo-Json -Compress",
        ])
        .env("NEXORA_PE_SMOKE_PATH", path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "PowerShell VersionInfo 读取失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
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
fn distribution_artifact_names_use_display_name_and_arch() {
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
            .ends_with("产品名称-aarch64.app.zip")
    );
    assert!(
        plan["dmg_path"]
            .as_str()
            .unwrap()
            .ends_with("产品名称-aarch64.dmg")
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
fn build_datetime_uses_local_24_hour_time_and_is_strictly_increasing() {
    let current = inspect_build_datetime_number(1_785_765_975, 8 * 60 * 60, None).unwrap();
    assert_eq!(current, 260_803_220_615);
    assert_eq!(current.to_string().len(), 12);
    assert_eq!(
        inspect_build_datetime_number(1_785_765_975, 8 * 60 * 60, Some(current)).unwrap(),
        current + 1
    );
    assert_eq!(
        inspect_build_datetime_number(1_785_765_974, 8 * 60 * 60, Some(current + 10)).unwrap(),
        current + 11
    );
    assert_eq!(
        inspect_build_datetime_number(1_785_765_975, 0, None).unwrap(),
        260_803_140_615
    );
    assert!(inspect_build_datetime_number(1_785_765_975, 8 * 60 * 60, Some(u64::MAX)).is_err());
}

#[test]
fn checksum_writes_standard_sha256_sidecar_for_each_artifact_name() {
    let fixture = Fixture::new(
        "sha256-sidecar",
        &app_config("one", "package-one", "应用一"),
    );

    for file_name in ["package-one.app.zip", "package-one.dmg"] {
        let artifact = fixture.root.join(file_name);
        fs::write(&artifact, b"nexora").unwrap();

        let checksum = write_sha256_sidecar(&artifact).unwrap();

        assert_eq!(checksum, fixture.root.join(format!("{file_name}.sha256")));
        assert_eq!(
            fs::read_to_string(checksum).unwrap(),
            format!(
                "6684bd7ca5b118220b0b7f9996bc71c75359fec3242a3c8ce8a53e889081bf55  {file_name}\n"
            )
        );
    }
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
fn runtime_config_change_invalidates_existing_receipt() {
    let fixture = Fixture::new(
        "runtime-config-receipt",
        &app_config("one", "package-one", "应用一"),
    );

    let first = inspect_prepare_release_receipt(fixture.config(), "one").unwrap();
    fs::write(
        fixture.root.join("config/package-one.toml"),
        "value = \"changed\"\n",
    )
    .unwrap();
    let next = inspect_prepare_release_receipt(fixture.config(), "one").unwrap();
    let persisted: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.root.join("dist/one/stable/release.json")).unwrap(),
    )
    .unwrap();

    assert_eq!(
        persisted["runtime_config_sha256"],
        next["runtime_config_sha256"]
    );
    assert_ne!(
        persisted["runtime_config_sha256"],
        first["runtime_config_sha256"]
    );
    assert_eq!(
        next["updater_feed"],
        "http://127.0.0.1:9000/releases/e2e/one/stable/latest.json"
    );
}

#[test]
fn windows_build_plan_uses_branded_setup_exe_and_update_zip() {
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
            .ends_with("Application One-x86_64.windows.zip")
    );
    assert!(
        plan["setup_path"]
            .as_str()
            .unwrap()
            .ends_with("Application One-x86_64.exe")
    );
    assert!(plan.get("msi_path").is_none());
}

#[test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn missing_targets_configuration_uses_rustc_host() {
    let config = with_windows_target(app_config("one", "package-one", "Application One")).replace(
        "[apps.one.targets]\nrequired = [\"x86_64-pc-windows-msvc\"]\n\n",
        "",
    );
    let fixture = Fixture::new("automatic-host-target", &config);
    let plans = inspect_build_plans(fixture.config(), "one").unwrap();
    let output = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let host = stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap();

    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0]["target"], host);
}

#[test]
fn windows_arm64_build_plan_uses_aarch64_artifact_suffix() {
    let config = with_windows_target(app_config("one", "package-one", "Application One")).replace(
        "required = [\"x86_64-pc-windows-msvc\"]",
        "required = [\"aarch64-pc-windows-msvc\"]",
    );
    let fixture = Fixture::new("windows-arm64-plan", &config);
    let plans = inspect_build_plans(fixture.config(), "one").unwrap();

    assert_eq!(plans[0]["target"], "aarch64-pc-windows-msvc");
    assert!(
        plans[0]["setup_path"]
            .as_str()
            .unwrap()
            .ends_with("Application One-aarch64.exe")
    );
}

#[test]
fn windows_binary_uses_gui_subsystem_with_rust_main_entrypoint() {
    assert_eq!(
        inspect_windows_binary_link_args(Path::new("nexora-icon.res")),
        vec![
            "link-arg=nexora-icon.res",
            "link-arg=/SUBSYSTEM:WINDOWS",
            "link-arg=/ENTRY:mainCRTStartup",
        ]
    );
}

#[test]
fn windows_installer_source_defines_chinese_inno_flow() {
    let fixture = Fixture::new(
        "windows installer 中文",
        &with_windows_target(app_config("one", "package-one", "Application One")),
    );
    let sources = inspect_windows_installer_sources(fixture.config(), "one").unwrap();
    let script = sources["iss"].as_str().unwrap();
    let updater_config = &sources["updater_config"];

    assert_eq!(sources["file_version"], "1.2.3.7");
    assert!(script.contains("WizardStyle=modern"));
    assert!(script.contains("PrivilegesRequired=lowest"));
    assert!(
        script.contains("DefaultDirName={localappdata}\\Programs\\{#AppPublisher}\\{#AppName}")
    );
    assert!(!script.contains("DefaultDirName={localappdata}\\Programs\\{#AppName}"));
    assert!(!script.contains("DefaultDirName={localappdata}\\Programs\\{#AppId}"));
    assert!(script.contains("DisableDirPage=no"));
    assert!(script.contains("#define LanguageFile \""));
    assert!(script.contains("ChineseSimplified.isl"));
    assert!(script.contains("MessagesFile: \"{#LanguageFile}\""));
    assert!(script.contains("CloseApplications=force"));
    assert!(script.contains("Source: \"*\""));
    assert!(script.contains("recursesubdirs createallsubdirs"));
    assert!(script.contains("skipifsilent"));
    assert!(script.starts_with("#define AppId \"com.example.one\""));
    assert!(script.contains("#define AppName \"Application One\""));
    assert!(script.contains("#define ArchitectureAllowed \"x64compatible and not arm64\""));
    assert!(script.contains("#define MinimumWindowsBuild 15063"));
    assert!(script.contains("#define SourceDir \""));
    assert!(script.contains("windows installer 中文"));
    assert!(!script.contains("/DAppId"));
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
fn inno_definitions_escape_braces_and_select_arm64_architecture() {
    let config = with_windows_target(app_config("one", "package-one", "Application One"))
        .replace(
            "publisher = \"Nexora Test Publisher\"",
            r#"publisher = "Nexora {Quoted} 发布者""#,
        )
        .replace(
            "required = [\"x86_64-pc-windows-msvc\"]",
            "required = [\"aarch64-pc-windows-msvc\"]",
        );
    let fixture = Fixture::new("inno quoted 中文", &config);
    let sources = inspect_windows_installer_sources(fixture.config(), "one").unwrap();
    let script = sources["iss"].as_str().unwrap();

    assert!(script.contains("#define AppPublisher \"Nexora {{Quoted} 发布者\""));
    assert!(script.contains("#define ArchitectureAllowed \"arm64\""));
    assert!(script.contains("#define ArchitectureInstallMode \"arm64\""));
}

#[test]
fn invalid_windows_publishers_are_rejected_before_installer_generation() {
    for publisher in [
        ".",
        "..",
        "bad/name",
        "bad\\name",
        "bad:name",
        "bad\"name",
        "CON",
        "lpt1.txt",
        "trailing. ",
    ] {
        let config = with_windows_target(app_config("one", "package-one", "Application One"))
            .replace(
                "publisher = \"Nexora Test Publisher\"",
                &format!("publisher = {publisher:?}"),
            );
        let fixture = Fixture::new("invalid-windows-publisher", &config);
        let error = inspect_windows_installer_sources(fixture.config(), "one")
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("platforms.windows.publisher"),
            "publisher {publisher:?} returned: {error}"
        );
        assert!(
            error.contains("安装目录"),
            "publisher {publisher:?} returned: {error}"
        );
    }
}

#[test]
#[cfg(not(windows))]
fn inno_definitions_reject_windows_invalid_path_characters() {
    let error = inspect_inno_path_definition(Path::new("inno-invalid?path"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Windows 路径不允许的字符"));
}

#[test]
fn inno_definitions_reject_app_ids_longer_than_inno_supports() {
    let app_id = "a".repeat(128);
    let config = with_windows_target(app_config("one", "package-one", "Application One")).replace(
        "app_id = \"com.example.one\"",
        &format!("app_id = \"{app_id}\""),
    );
    let fixture = Fixture::new("inno-app-id-limit", &config);
    let error = inspect_windows_installer_sources(fixture.config(), "one")
        .unwrap_err()
        .to_string();

    assert!(error.contains("AppId"));
    assert!(error.contains("127"));
}

#[test]
fn removed_windows_installer_and_scope_fields_are_rejected() {
    for field in ["installer = \"wix\"", "install_scope = \"user\""] {
        let config = with_windows_target(app_config("one", "package-one", "Application One"))
            .replace(
                "publisher = \"Nexora Test Publisher\"",
                &format!("{field}\npublisher = \"Nexora Test Publisher\""),
            );
        let fixture = Fixture::new("removed-windows-field", &config);
        let error = inspect_windows_installer_sources(fixture.config(), "one")
            .unwrap_err()
            .to_string();

        assert!(error.contains(field.split_once(' ').unwrap().0));
        assert!(error.contains("unknown field") || error.contains("未知字段"));
    }
}

#[test]
fn windows_update_zip_uses_protocol_paths_and_round_trips_through_updater() {
    let fixture = Fixture::new(
        "windows-update-zip",
        &with_windows_target(app_config("one", "package-one", "Application One")),
    );
    let staging = fixture.root.join("windows-payload");
    fs::create_dir_all(staging.join("config")).unwrap();
    fs::write(staging.join("main.exe"), b"main").unwrap();
    fs::write(staging.join("main-updater.exe"), b"updater").unwrap();
    fs::write(staging.join("nexora-updater.json"), b"{}").unwrap();
    fs::write(staging.join("config/application.toml"), b"value = true\n").unwrap();
    let archive_path = fixture.root.join("update.windows.zip");

    inspect_create_windows_update_zip(&staging, &archive_path).unwrap();

    let archive_file = fs::File::open(&archive_path).unwrap();
    let mut archive = zip::ZipArchive::new(archive_file).unwrap();
    let mut names = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        vec![
            "config/application.toml",
            "main-updater.exe",
            "main.exe",
            "nexora-updater.json",
        ]
    );
    assert!(names.iter().all(|name| !name.contains('\\')));
    drop(archive);

    let extracted = fixture.root.join("extracted");
    updater::extract_windows_update_zip(&archive_path, &extracted, "main.exe", "main-updater.exe")
        .unwrap();
    assert_eq!(
        fs::read(extracted.join("config/application.toml")).unwrap(),
        b"value = true\n"
    );
}

#[test]
fn unsigned_windows_updater_config_omits_authenticode_identity() {
    let config = with_unsigned_windows_signing(app_config("one", "package-one", "Application One"));
    let fixture = Fixture::new("windows-unsigned-updater", &config);

    let sources = inspect_windows_installer_sources(fixture.config(), "one").unwrap();
    let updater_config = &sources["updater_config"];

    assert!(updater_config["expected_windows_signer_thumbprint"].is_null());
    assert!(updater_config["expected_windows_publisher"].is_null());
}

#[test]
fn unsigned_windows_signing_rejects_authenticode_only_fields() {
    for (field, setting) in [
        (
            "signing_thumbprint",
            "signing_thumbprint = \"00112233445566778899AABBCCDDEEFF00112233\"",
        ),
        (
            "expected_publisher",
            "expected_publisher = \"Nexora Test Publisher\"",
        ),
        (
            "timestamp_url",
            "timestamp_url = \"http://timestamp.example.test\"",
        ),
    ] {
        let config =
            with_unsigned_windows_signing(app_config("one", "package-one", "Application One"))
                .replace(
                    "signing = \"none\"",
                    &format!("signing = \"none\"\n{setting}"),
                );
        let fixture = Fixture::new(&format!("windows-unsigned-{field}"), &config);

        let error = inspect_windows_installer_sources(fixture.config(), "one")
            .unwrap_err()
            .to_string();

        assert!(error.contains(field), "unexpected error: {error}");
        assert!(
            error.contains("signing = \"none\""),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn windows_authenticode_configuration_is_validated_before_packaging() {
    let invalid_thumbprint =
        with_windows_target(app_config("one", "package-one", "Application One")).replace(
            "signing_thumbprint = \"00112233445566778899AABBCCDDEEFF00112233\"",
            "signing_thumbprint = \"not-a-thumbprint\"",
        );
    let fixture = Fixture::new("windows-invalid-thumbprint", &invalid_thumbprint);
    let error = inspect_windows_installer_sources(fixture.config(), "one")
        .unwrap_err()
        .to_string();
    assert!(error.contains("40 位 SHA-1"), "unexpected error: {error}");

    let missing_timestamp =
        with_windows_target(app_config("one", "package-one", "Application One")).replace(
            "timestamp_url = \"http://timestamp.example.test\"",
            "timestamp_url = \"\"",
        );
    let fixture = Fixture::new("windows-missing-timestamp", &missing_timestamp);
    let error = inspect_windows_installer_sources(fixture.config(), "one")
        .unwrap_err()
        .to_string();
    assert!(error.contains("timestamp_url"), "unexpected error: {error}");
}

#[test]
fn windows_file_version_accepts_large_release_build_number() {
    let fixture = Fixture::new(
        "windows-large-build-number",
        &with_windows_target(
            app_config("one", "package-one", "Application One")
                .replace("build_number = 7", "build_number = 260803122140"),
        ),
    );
    let sources = inspect_windows_installer_sources(fixture.config(), "one").unwrap();

    assert_eq!(sources["file_version"], "1.2.3.54236");
    assert!(
        sources["iss"]
            .as_str()
            .unwrap()
            .contains("#define FileVersion \"1.2.3.54236\"")
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
        "bad:name",
        "bad*name",
        "CON",
        "lpt1.txt",
        "trailing. ",
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
fn multi_channel_selection_uses_default_repeats_and_cartesian_all() {
    let apps = format!(
        "{}\n{}",
        multi_channel_app_config("one", "package-one", "应用一"),
        multi_channel_app_config("two", "package-two", "应用二")
    );
    let fixture = Fixture::new("multi-channel-selection", &apps);

    assert_eq!(
        inspect_release_selection(fixture.config(), &["one"], false, &[], false).unwrap(),
        vec![serde_json::json!({"app": "one", "channel": "nightly"})]
    );
    assert_eq!(
        inspect_release_selection(fixture.config(), &["one"], false, &["beta"], false).unwrap(),
        vec![serde_json::json!({"app": "one", "channel": "beta"})]
    );
    assert_eq!(
        inspect_release_selection(fixture.config(), &[], true, &[], true).unwrap(),
        vec![
            serde_json::json!({"app": "one", "channel": "beta"}),
            serde_json::json!({"app": "one", "channel": "nightly"}),
            serde_json::json!({"app": "one", "channel": "stable"}),
            serde_json::json!({"app": "two", "channel": "beta"}),
            serde_json::json!({"app": "two", "channel": "nightly"}),
            serde_json::json!({"app": "two", "channel": "stable"}),
        ]
    );
    assert!(
        inspect_release_selection(fixture.config(), &["one"], false, &["edge"], false)
            .unwrap_err()
            .to_string()
            .contains("channel")
    );
}

#[test]
fn multi_channel_merges_overrides_and_generates_channel_feed() {
    let fixture = Fixture::new(
        "multi-channel-merge",
        &multi_channel_app_config("one", "package-one", "应用一"),
    );
    fs::write(
        fixture.root.join("config/package-one-beta.toml"),
        "value = \"beta\"\n",
    )
    .unwrap();

    let beta = inspect_build_plans_for_channel(fixture.config(), "one", "beta")
        .unwrap()
        .remove(0);
    assert_eq!(beta["channel"], "beta");
    assert_eq!(beta["build_number"], 8);
    assert_eq!(
        beta["runtime_config_source"],
        "config/package-one-beta.toml"
    );
    assert_eq!(
        beta["updater_feed"],
        "http://127.0.0.1:9000/releases/e2e/one/beta/latest.json"
    );

    let nightly = inspect_build_plans_for_channel(fixture.config(), "one", "nightly")
        .unwrap()
        .remove(0);
    assert_eq!(nightly["runtime_config_source"], "config/package-one.toml");
    assert_eq!(
        nightly["updater_feed"],
        "http://127.0.0.1:9000/releases/e2e/one/nightly/latest.json"
    );
}

#[test]
fn multi_channel_rejects_invalid_default_and_static_feed() {
    let invalid = multi_channel_app_config("one", "package-one", "应用一").replace(
        "default_channel = \"nightly\"",
        "default_channel = \"edge\"",
    );
    let fixture = Fixture::new("invalid-default-channel", &invalid);
    assert!(
        inspect_build_plans(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("default_channel")
    );

    let invalid = multi_channel_app_config("one", "package-one", "应用一").replace(
        "channels = [\"nightly\", \"beta\", \"stable\"]",
        "feed_url = \"https://example.com/static/latest.json\"\nchannels = [\"nightly\", \"beta\", \"stable\"]",
    );
    let fixture = Fixture::new("static-feed", &invalid);
    assert!(
        inspect_build_plans(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("feed_url")
    );
}

#[test]
fn release_configuration_is_required_and_validated() {
    let missing = app_config("one", "package-one", "应用一").replace(
        "[apps.one.release]\nchannel = \"stable\"\nversion = \"1.2.3\"\nbuild_number = 7\nminimum_supported_version = \"0.0.0\"\nnotes = \"docs/releases/one.md\"\n\n",
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
fn release_notes_are_required_safe_utf8_bounded_and_frozen() {
    let missing = app_config("one", "package-one", "应用一")
        .replace("notes = \"docs/releases/one.md\"\n", "");
    let fixture = Fixture::new("notes-missing", &missing);
    assert!(
        inspect_freeze_release_notes(fixture.config(), "one", "stable")
            .unwrap_err()
            .to_string()
            .contains("release.notes")
    );

    let escaped = app_config("one", "package-one", "应用一").replace(
        "notes = \"docs/releases/one.md\"",
        "notes = \"../outside.md\"",
    );
    let fixture = Fixture::new("notes-escaped", &escaped);
    assert!(
        inspect_freeze_release_notes(fixture.config(), "one", "stable")
            .unwrap_err()
            .to_string()
            .contains("相对路径")
    );

    let fixture = Fixture::new(
        "notes-invalid-utf8",
        &app_config("one", "package-one", "应用一"),
    );
    fs::write(fixture.root.join("docs/releases/one.md"), [0xff, 0xfe]).unwrap();
    assert!(
        inspect_freeze_release_notes(fixture.config(), "one", "stable")
            .unwrap_err()
            .to_string()
            .contains("UTF-8")
    );

    let fixture = Fixture::new(
        "notes-too-large",
        &app_config("one", "package-one", "应用一"),
    );
    fs::write(
        fixture.root.join("docs/releases/one.md"),
        vec![b'a'; updater::MAX_RELEASE_NOTES_BYTES as usize + 1],
    )
    .unwrap();
    assert!(
        inspect_freeze_release_notes(fixture.config(), "one", "stable")
            .unwrap_err()
            .to_string()
            .contains("1..=")
    );

    let fixture = Fixture::new("notes-frozen", &app_config("one", "package-one", "应用一"));
    let first = inspect_freeze_release_notes(fixture.config(), "one", "stable")
        .unwrap()
        .unwrap();
    let frozen_path = PathBuf::from(first["path"].as_str().unwrap());
    let frozen = fs::read(&frozen_path).unwrap();
    fs::write(
        fixture.root.join("docs/releases/one.md"),
        "# 已修改源文件\n",
    )
    .unwrap();
    let second = inspect_freeze_release_notes(fixture.config(), "one", "stable")
        .unwrap()
        .unwrap();
    assert_eq!(first["sha256"], second["sha256"]);
    assert_eq!(fs::read(frozen_path).unwrap(), frozen);
}

#[test]
fn release_channel_can_override_notes_source() {
    let config = multi_channel_app_config("one", "package-one", "应用一").replace(
        "[apps.one.release.channels.beta]\nbuild_number = 8",
        "[apps.one.release.channels.beta]\nnotes = \"docs/releases/beta.md\"\nbuild_number = 8",
    );
    let fixture = Fixture::new("notes-channel-override", &config);
    fs::write(fixture.root.join("docs/releases/beta.md"), "# Beta\n").unwrap();
    fs::write(
        fixture.root.join("config/package-one-beta.toml"),
        "value = \"beta\"\n",
    )
    .unwrap();
    let plan = inspect_build_plans_for_channel(fixture.config(), "one", "beta").unwrap();
    assert_eq!(plan[0]["notes_source"], "docs/releases/beta.md");
}

#[test]
fn macos_and_windows_payloads_carry_receipt_identity_and_identical_notes() {
    let mac = Fixture::new(
        "mac-release-resources",
        &app_config("one", "package-one", "应用一"),
    );
    let receipt = inspect_prepare_release_receipt(mac.config(), "one").unwrap();
    let resources = inspect_release_resources(mac.config(), "one", "aarch64-apple-darwin").unwrap();
    assert_eq!(resources["metadata"]["version"], receipt["version"]);
    assert_eq!(
        resources["metadata"]["build_number"],
        receipt["build_number"]
    );
    assert_eq!(
        resources["metadata"]["notes"]["sha256"],
        resources["notes_sha256"]
    );
    assert!(
        resources["directory"]
            .as_str()
            .unwrap()
            .ends_with(".app/Contents/Resources")
    );

    let windows = Fixture::new(
        "windows-release-resources",
        &with_windows_target(app_config("one", "package-one", "Application One")),
    );
    let resources =
        inspect_release_resources(windows.config(), "one", "x86_64-pc-windows-msvc").unwrap();
    assert_eq!(resources["metadata"]["target"], "x86_64-pc-windows-msvc");
    assert_eq!(
        resources["metadata"]["notes"]["sha256"],
        resources["notes_sha256"]
    );
    assert!(
        resources["directory"]
            .as_str()
            .unwrap()
            .ends_with("payload")
    );
}

#[test]
fn updater_disabled_release_still_writes_build_identity_without_notes() {
    let config = app_config("one", "package-one", "应用一")
        .replace("enabled = true", "enabled = false")
        .replace("notes = \"docs/releases/one.md\"\n", "");
    let fixture = Fixture::new("release-without-updater", &config);
    let resources =
        inspect_release_resources(fixture.config(), "one", "aarch64-apple-darwin").unwrap();
    assert_eq!(resources["metadata"]["build_number"], 7);
    assert!(resources["metadata"]["notes"].is_null());
    assert!(resources["notes_sha256"].is_null());
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
    write_windows_artifacts(&fixture, "Application One", true, true);
    assert_eq!(
        inspect_release_artifacts(fixture.config(), "one").unwrap(),
        vec!["windows_setup_exe", "windows_update_zip"]
    );

    write_windows_artifacts(&fixture, "Application One", true, false);
    assert!(
        inspect_release_artifacts(fixture.config(), "one")
            .unwrap_err()
            .to_string()
            .contains("windows_update_zip")
    );
}

#[test]
fn publish_artifacts_are_read_from_the_exact_selected_channel() {
    let fixture = Fixture::new(
        "channel-artifact-selection",
        &multi_channel_app_config("one", "package-one", "应用一"),
    );
    fs::write(
        fixture.root.join("config/package-one-beta.toml"),
        "value = \"beta\"\n",
    )
    .unwrap();
    write_artifacts(&fixture, "aarch64-apple-darwin", true, true);

    assert_eq!(
        inspect_release_artifacts_for_channel(fixture.config(), "one", "stable").unwrap(),
        vec!["macos_app_zip", "macos_dmg"]
    );
    let error = inspect_release_artifacts_for_channel(fixture.config(), "one", "beta")
        .unwrap_err()
        .to_string();
    assert!(error.contains("dist"));
    assert!(error.contains("beta"));
}

#[test]
fn channel_root_uses_branded_macos_names_and_checksums() {
    let fixture = Fixture::new("single-alias", &app_config("one", "package-one", "应用一"));
    write_artifacts(&fixture, "aarch64-apple-darwin", true, true);
    assert_eq!(
        inspect_channel_artifact_keys(fixture.config(), "one").unwrap(),
        vec![
            "e2e/one/stable/应用一-aarch64.app.zip",
            "e2e/one/stable/应用一-aarch64.app.zip.sha256",
            "e2e/one/stable/应用一-aarch64.dmg",
            "e2e/one/stable/应用一-aarch64.dmg.sha256",
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
        inspect_channel_artifact_keys(fixture.config(), "one").unwrap(),
        vec![
            "e2e/one/stable/应用一-aarch64.app.zip",
            "e2e/one/stable/应用一-aarch64.app.zip.sha256",
            "e2e/one/stable/应用一-aarch64.dmg",
            "e2e/one/stable/应用一-aarch64.dmg.sha256",
            "e2e/one/stable/应用一-x86_64.app.zip",
            "e2e/one/stable/应用一-x86_64.app.zip.sha256",
            "e2e/one/stable/应用一-x86_64.dmg",
            "e2e/one/stable/应用一-x86_64.dmg.sha256",
        ]
    );
}

#[test]
fn channel_root_uses_branded_windows_names_without_latest_aliases() {
    let fixture = Fixture::new(
        "windows-latest-aliases",
        &with_windows_target(app_config("one", "package-one", "应用一")),
    );
    write_windows_artifacts(&fixture, "应用一", true, true);

    assert_eq!(
        inspect_channel_artifact_keys(fixture.config(), "one").unwrap(),
        vec![
            "e2e/one/stable/应用一-x86_64.exe",
            "e2e/one/stable/应用一-x86_64.exe.sha256",
            "e2e/one/stable/应用一-x86_64.windows.zip",
            "e2e/one/stable/应用一-x86_64.windows.zip.sha256",
        ]
    );
}

#[test]
fn empty_object_prefix_uses_app_key_root_without_double_slashes() {
    let config = app_config("one", "package-one", "iMES")
        .replace("object_prefix = \"e2e\"", "object_prefix = \"\"")
        .replace("/releases/e2e/one/", "/releases/one/");
    let fixture = Fixture::new("empty-object-prefix", &config);
    inspect_prepare_release_receipt(fixture.config(), "one").unwrap();
    let layout = inspect_publish_object_layout(
        fixture.config(),
        "one",
        "aarch64-apple-darwin",
        "iMES-aarch64.dmg",
    )
    .unwrap();

    assert_eq!(layout["latest_key"], "one/stable/latest.json");
    assert_eq!(layout["sequence_key"], "one/stable/manifests/42.json");
    assert_eq!(layout["channel_key"], "one/stable/iMES-aarch64.dmg");
    assert_eq!(
        layout["versioned_key"],
        "one/stable/1.2.3/7/aarch64/iMES-aarch64.dmg"
    );
    assert!(!layout.to_string().contains("//one"));
    assert!(
        !layout["versioned_key"]
            .as_str()
            .unwrap()
            .contains("releases")
    );
}

#[test]
fn app_key_keeps_feed_identity_stable_when_display_name_changes() {
    let fixture = Fixture::new(
        "stable-app-key",
        &app_config("one", "package-one", "旧名称"),
    );
    inspect_prepare_release_receipt(fixture.config(), "one").unwrap();
    let before = inspect_publish_object_layout(
        fixture.config(),
        "one",
        "aarch64-apple-darwin",
        "旧名称-aarch64.dmg",
    )
    .unwrap();
    let config = fs::read_to_string(fixture.config())
        .unwrap()
        .replace("display_name = \"旧名称\"", "display_name = \"新名称\"");
    fs::write(fixture.config(), config).unwrap();
    let after = inspect_publish_object_layout(
        fixture.config(),
        "one",
        "aarch64-apple-darwin",
        "新名称-aarch64.dmg",
    )
    .unwrap();

    assert_eq!(before["latest_key"], after["latest_key"]);
    assert_eq!(before["sequence_key"], after["sequence_key"]);
    assert!(after["channel_key"].as_str().unwrap().contains("新名称"));
}

#[test]
fn channel_publish_target_overrides_merge_by_field_and_revalidate_http() {
    let fixture = Fixture::new("channel-target", &app_config("one", "package-one", "iMES"));
    let config = fs::read_to_string(fixture.config())
        .unwrap()
        .replace("http://127.0.0.1:9000", "https://s3.example.com")
        .replace(
            "http://127.0.0.1:9000/releases",
            "https://downloads.example.com",
        )
        .replace("allow_insecure_http = true", "allow_insecure_http = false")
        .replace(
            "\n[apps.one]",
            r#"
[publish.targets.rustfs.channels.nightly]
provider = "aliyun_oss"
endpoint = "http://192.168.0.250:9000"
public_base_url = "http://192.168.0.250:9000/releases"
allow_insecure_http = true

[apps.one]"#,
        );
    fs::write(fixture.config(), config).unwrap();
    let nightly = inspect_effective_publish_target(fixture.config(), "one", "nightly").unwrap();
    assert_eq!(nightly["provider"], "aliyun_oss");
    assert_eq!(nightly["endpoint"], "http://192.168.0.250:9000");
    assert_eq!(nightly["bucket"], "releases");
    assert_eq!(nightly["region"], "us-east-1");
    assert_eq!(nightly["force_path_style"], true);
    assert_eq!(nightly["allow_insecure_http"], true);
    assert!(nightly.get("credential_env_prefix").is_none());
    let stable = inspect_effective_publish_target(fixture.config(), "one", "stable").unwrap();
    assert_eq!(stable["provider"], "s3");
    assert_eq!(stable["endpoint"], "https://s3.example.com");
    assert!(stable.get("credential_env_prefix").is_none());

    let invalid = fs::read_to_string(fixture.config()).unwrap().replace(
        "public_base_url = \"http://192.168.0.250:9000/releases\"\nallow_insecure_http = true",
        "public_base_url = \"http://192.168.0.250:9000/releases\"\nallow_insecure_http = false",
    );
    fs::write(fixture.config(), invalid).unwrap();
    assert!(
        inspect_effective_publish_target(fixture.config(), "one", "nightly")
            .unwrap_err()
            .to_string()
            .contains("allow_insecure_http")
    );
}

#[test]
fn publish_target_rejects_removed_credential_env_prefix() {
    let fixture = Fixture::new(
        "removed-credential-prefix",
        &app_config("one", "package-one", "iMES"),
    );
    let config = fs::read_to_string(fixture.config()).unwrap().replace(
        "allow_insecure_http = true",
        "allow_insecure_http = true\ncredential_env_prefix = \"\"",
    );
    fs::write(fixture.config(), config).unwrap();

    let error = inspect_effective_publish_target(fixture.config(), "one", "stable")
        .unwrap_err()
        .to_string();
    assert!(error.contains("credential_env_prefix"));
    assert!(error.contains("unknown field") || error.contains("未知字段"));
}

#[test]
fn publish_credentials_fall_back_independently_by_channel_and_field() {
    let _lock = ENVIRONMENT_LOCK.lock().unwrap();
    let names = [
        "NEXORA_PUBLISH_ACCESS_KEY_ID",
        "NEXORA_PUBLISH_SECRET_ACCESS_KEY",
        "NEXORA_PUBLISH_SESSION_TOKEN",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "NEXORA_PUBLISH_BETA_ACCESS_KEY_ID",
        "NEXORA_PUBLISH_BETA_SECRET_ACCESS_KEY",
        "NEXORA_PUBLISH_BETA_SESSION_TOKEN",
        "RUSTFS_ACCESS_KEY_ID",
        "RUSTFS_SECRET_ACCESS_KEY",
    ];
    let _environment = EnvironmentGuard::clear(&names);

    EnvironmentGuard::set("NEXORA_PUBLISH_ACCESS_KEY_ID", "nexora-access");
    EnvironmentGuard::set("NEXORA_PUBLISH_SECRET_ACCESS_KEY", "nexora-secret");
    EnvironmentGuard::set("NEXORA_PUBLISH_SESSION_TOKEN", "nexora-session");
    EnvironmentGuard::set("AWS_ACCESS_KEY_ID", "aws-access");
    EnvironmentGuard::set("AWS_SECRET_ACCESS_KEY", "aws-secret");
    EnvironmentGuard::set("AWS_SESSION_TOKEN", "aws-session");
    let selected = inspect_credential_selection("stable").unwrap();
    assert_eq!(
        selected["access_key_source"],
        "NEXORA_PUBLISH_ACCESS_KEY_ID"
    );
    assert_eq!(
        selected["secret_key_source"],
        "NEXORA_PUBLISH_SECRET_ACCESS_KEY"
    );
    assert_eq!(
        selected["session_token_source"],
        "NEXORA_PUBLISH_SESSION_TOKEN"
    );
    assert_eq!(selected["has_session_token"], true);

    EnvironmentGuard::set("NEXORA_PUBLISH_BETA_ACCESS_KEY_ID", "beta-access");
    EnvironmentGuard::set("NEXORA_PUBLISH_BETA_SECRET_ACCESS_KEY", "");
    EnvironmentGuard::unset("NEXORA_PUBLISH_SESSION_TOKEN");
    let selected = inspect_credential_selection("beta").unwrap();
    assert_eq!(
        selected["access_key_source"],
        "NEXORA_PUBLISH_BETA_ACCESS_KEY_ID"
    );
    assert_eq!(
        selected["secret_key_source"],
        "NEXORA_PUBLISH_SECRET_ACCESS_KEY"
    );
    assert_eq!(selected["session_token_source"], "AWS_SESSION_TOKEN");
    assert_eq!(selected["has_session_token"], true);

    EnvironmentGuard::set("NEXORA_PUBLISH_SECRET_ACCESS_KEY", "");
    let selected = inspect_credential_selection("beta").unwrap();
    assert_eq!(selected["secret_key_source"], "AWS_SECRET_ACCESS_KEY");

    EnvironmentGuard::unset("NEXORA_PUBLISH_BETA_ACCESS_KEY_ID");
    EnvironmentGuard::unset("NEXORA_PUBLISH_ACCESS_KEY_ID");
    EnvironmentGuard::unset("AWS_ACCESS_KEY_ID");
    EnvironmentGuard::set("RUSTFS_ACCESS_KEY_ID", "legacy-access");
    EnvironmentGuard::set("RUSTFS_SECRET_ACCESS_KEY", "legacy-secret");
    let error = inspect_credential_selection("beta")
        .unwrap_err()
        .to_string();
    assert!(error.contains("ACCESS_KEY_ID"));
    assert!(error.contains("NEXORA_PUBLISH_BETA_ACCESS_KEY_ID"));
    assert!(!error.contains("RUSTFS"));
}

#[test]
fn build_dependency_guidance_is_manual_and_read_only() {
    let macos = inspect_build_dependency_guidance("macos").unwrap();
    assert_eq!(macos["xcode_command_line_tools"], "xcode-select --install");
    assert_eq!(macos["cargo_bundle"], "cargo install cargo-bundle");
    assert_eq!(macos["create_dmg"], "brew install create-dmg");
    assert!(
        macos["homebrew"]
            .as_str()
            .unwrap()
            .contains("Homebrew/install/HEAD/install.sh")
    );
    assert_eq!(macos["automatic_install"], false);

    let windows = inspect_build_dependency_guidance("windows").unwrap();
    assert_eq!(windows["inno_setup_supported"], ">=6.7.3, <8.0.0");
    assert_eq!(windows["inno_setup_recommended"], "7.0.2");
    assert!(
        windows["inno_setup_install"]
            .as_str()
            .unwrap()
            .contains(
                "winget install --source winget --exact --id JRSoftware.InnoSetup.7 --version 7.0.2 --scope user --silent --force"
            )
    );
    assert!(
        windows["windows_sdk"]
            .as_str()
            .unwrap()
            .contains("Microsoft.WindowsSDK.10.0.26100")
    );
    assert_eq!(windows["automatic_install"], false);
}

#[test]
fn inno_dependency_accepts_only_the_supported_range() {
    for version in ["6.7.3", "6.8.1", "7.0.0", "7.0.2", "7.9.9"] {
        assert_eq!(
            inspect_inno_setup_requirement(Some(&format!(
                "Compiler engine version: Inno Setup {version}"
            )))
            .unwrap(),
            "ready"
        );
    }

    for version in ["6.7.2", "8.0.0", "8.1.0"] {
        let error = inspect_inno_setup_requirement(Some(&format!(
            "Compiler engine version: Inno Setup {version}"
        )))
        .unwrap_err()
        .to_string();
        assert!(error.contains(">= 6.7.3, < 8.0.0"));
        assert!(error.contains("JRSoftware.InnoSetup.7"));
    }

    for output in [None, Some("Inno Setup 7 Command-Line Compiler")] {
        let error = inspect_inno_setup_requirement(output)
            .unwrap_err()
            .to_string();
        assert!(error.contains(">= 6.7.3, < 8.0.0"));
        assert!(error.contains("Nexora 不会自动执行"));
    }
}

#[test]
fn inno_candidate_selection_uses_the_highest_valid_version() {
    let selected = inspect_select_inno_setup_candidate(&[
        (
            "C:/broken/Inno Setup 7/ISCC.exe",
            Some("Inno Setup 7 Command-Line Compiler"),
        ),
        (
            "C:/Inno Setup 6/ISCC.exe",
            Some("Compiler engine version: Inno Setup 6.7.3"),
        ),
        (
            "C:/Inno Setup 7.0.0/ISCC.exe",
            Some("Compiler engine version: Inno Setup 7.0.0"),
        ),
        (
            "C:/Inno Setup 7.0.2/ISCC.exe",
            Some("Compiler engine version: Inno Setup 7.0.2"),
        ),
        (
            "C:/unsupported/Inno Setup 8/ISCC.exe",
            Some("Compiler engine version: Inno Setup 8.0.0"),
        ),
    ])
    .unwrap();
    assert_eq!(selected["path"], "C:/Inno Setup 7.0.2/ISCC.exe");
    assert_eq!(selected["version"], "7.0.2");

    let fallback = inspect_select_inno_setup_candidate(&[
        ("C:/broken/ISCC.exe", None),
        (
            "C:/valid/ISCC.exe",
            Some("Compiler engine version: Inno Setup 6.8.0"),
        ),
    ])
    .unwrap();
    assert_eq!(fallback["path"], "C:/valid/ISCC.exe");
    assert_eq!(fallback["version"], "6.8.0");

    let error = inspect_select_inno_setup_candidate(&[
        (
            "C:/old/ISCC.exe",
            Some("Compiler engine version: Inno Setup 6.7.2"),
        ),
        (
            "C:/future/ISCC.exe",
            Some("Compiler engine version: Inno Setup 8.0.0"),
        ),
    ])
    .unwrap_err()
    .to_string();
    assert!(error.contains("C:/old/ISCC.exe (6.7.2)"));
    assert!(error.contains("C:/future/ISCC.exe (8.0.0)"));
}

#[cfg(windows)]
#[test]
#[ignore = "需要 NEXORA_TEST_ISCC 指向人工或 CI 预装的兼容 ISCC.exe"]
fn installed_inno_setup_reports_supported_engine_version() {
    let path = env::var_os("NEXORA_TEST_ISCC")
        .map(PathBuf::from)
        .expect("NEXORA_TEST_ISCC must point to an installed ISCC.exe");

    assert!(
        path.is_file(),
        "missing installed ISCC.exe: {}",
        path.display()
    );
    assert_eq!(inspect_inno_setup_compiler_version(path).unwrap(), "7.0.2");
}

#[cfg(windows)]
#[test]
#[ignore = "需要 NEXORA_TEST_ISCC 指向人工或 CI 预装的兼容 ISCC.exe"]
fn installed_inno_setup_compiles_generated_installer_source() {
    let compiler = env::var_os("NEXORA_TEST_ISCC")
        .map(PathBuf::from)
        .expect("NEXORA_TEST_ISCC must point to an installed ISCC.exe");
    let config = with_windows_target(app_config("one", "package-one", "中文 iMES One")).replace(
        "publisher = \"Nexora Test Publisher\"",
        r#"publisher = "Nexora {Quoted} 发布者""#,
    );
    let fixture = Fixture::new("inno smoke 中文 path", &config);
    let setup = inspect_compile_windows_installer(
        fixture.config(),
        "one",
        compiler,
        fixture.root.join("inno smoke 输出"),
    )
    .unwrap();

    assert!(
        setup.is_file(),
        "missing compiled setup: {}",
        setup.display()
    );
    assert!(fs::metadata(setup).unwrap().len() > 0);
}

#[test]
fn windows_resources_use_utf8_unicode_table_and_distinct_process_identity() {
    let fixture = Fixture::new(
        "windows-resource",
        &with_windows_target(app_config("one", "package-one", "中文 iMES")),
    );
    let scripts = inspect_windows_resource_scripts(fixture.config(), "one").unwrap();
    let main = scripts["main"].as_str().unwrap();
    let updater = scripts["updater"].as_str().unwrap();

    for script in [main, updater] {
        assert!(script.starts_with("#pragma code_page(65001)"));
        assert!(script.contains("BLOCK \"080404B0\""));
        assert!(script.contains("VALUE \"Translation\", 0x0804, 1200"));
        assert!(script.contains("VALUE \"ProductName\", \"中文 iMES\\0\""));
        assert!(!script.contains("080403A8"));
        assert!(!script.contains("0x0804, 936"));
    }
    assert!(main.contains("VALUE \"FileDescription\", \"中文 iMES\\0\""));
    assert!(main.contains("VALUE \"InternalName\", \"package-one\\0\""));
    assert!(main.contains("VALUE \"OriginalFilename\", \"package-one.exe\\0\""));
    assert!(updater.contains("VALUE \"FileDescription\", \"中文 iMES 更新程序\\0\""));
    assert!(updater.contains("VALUE \"InternalName\", \"package-one-updater\\0\""));
    assert!(updater.contains("VALUE \"OriginalFilename\", \"package-one-updater.exe\\0\""));
}

#[test]
#[cfg(windows)]
#[ignore = "需要当前 Windows 宿主人工或 CI 预装 rc.exe 与 MSVC linker"]
fn windows_runner_reads_distinct_unicode_version_info_from_actual_pe_files() {
    let config = windows_host_config(with_windows_target(app_config(
        "one",
        "package-one",
        "中文 iMES",
    )));
    let fixture = Fixture::new("windows-pe-version-info", &config);
    let (main, updater) = inspect_compile_windows_resource_executables(
        fixture.config(),
        "one",
        fixture.root.join("pe-smoke"),
    )
    .unwrap();
    let main = windows_version_info(&main);
    let updater = windows_version_info(&updater);

    assert_eq!(main["FileDescription"], "中文 iMES");
    assert_eq!(main["ProductName"], "中文 iMES");
    assert_eq!(main["InternalName"], "package-one");
    assert_eq!(main["OriginalFilename"], "package-one.exe");
    assert_eq!(updater["FileDescription"], "中文 iMES 更新程序");
    assert_eq!(updater["ProductName"], "中文 iMES");
    assert_eq!(updater["InternalName"], "package-one-updater");
    assert_eq!(updater["OriginalFilename"], "package-one-updater.exe");
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
