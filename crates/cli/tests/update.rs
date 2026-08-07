#[allow(dead_code)]
#[path = "../src/update.rs"]
mod cli_update;

use sha2::{Digest as _, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{env, fs, path::PathBuf};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const RELEASE_WORKFLOW: &str = include_str!("../../../.github/workflows/release.yml");

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = env::temp_dir().join(format!(
            "nexora-cli-update-{name}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn manifest(version: &str, target: &str, url_scheme: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "release": version,
        "draft": false,
        "assets": {
            (target): {
                "name": format!("nexora-{target}"),
                "url": format!("{url_scheme}://example.test/nexora-{target}"),
                "size": 3,
                "sha256": format!("{:x}", Sha256::digest(b"new")),
            }
        }
    }))
    .unwrap()
}

#[test]
fn update_manifest_accepts_newer_release_and_detects_current_version() {
    let target = "x86_64-unknown-linux-gnu";
    assert_eq!(
        cli_update::inspect_update_decision(&manifest("2.0.0", target, "https"), "1.0.0", target)
            .unwrap(),
        "download:nexora-x86_64-unknown-linux-gnu"
    );
    assert_eq!(
        cli_update::inspect_update_decision(&manifest("2.0.0", target, "https"), "2.0.0", target)
            .unwrap(),
        "current:2.0.0"
    );
}

#[test]
fn update_manifest_rejects_invalid_schema_draft_non_https_and_downgrade() {
    let target = "x86_64-unknown-linux-gnu";
    let valid = manifest("2.0.0", target, "https");
    let mut invalid_schema: serde_json::Value = serde_json::from_slice(&valid).unwrap();
    invalid_schema["schema_version"] = 2.into();
    assert!(
        cli_update::inspect_update_decision(
            &serde_json::to_vec(&invalid_schema).unwrap(),
            "1.0.0",
            target,
        )
        .unwrap_err()
        .contains("schema_version")
    );

    let mut draft: serde_json::Value = serde_json::from_slice(&valid).unwrap();
    draft["draft"] = true.into();
    assert!(
        cli_update::inspect_update_decision(&serde_json::to_vec(&draft).unwrap(), "1.0.0", target,)
            .unwrap_err()
            .contains("draft")
    );
    assert!(
        cli_update::inspect_update_decision(&manifest("2.0.0", target, "http"), "1.0.0", target)
            .unwrap_err()
            .contains("HTTPS")
    );
    assert!(
        cli_update::inspect_update_decision(&valid, "3.0.0", target)
            .unwrap_err()
            .contains("降级")
    );
}

#[test]
fn update_manifest_missing_target_reports_manual_cli_install_without_fallback() {
    let error = cli_update::inspect_update_decision(
        &manifest("2.0.0", "aarch64-apple-darwin", "https"),
        "1.0.0",
        "x86_64-unknown-linux-gnu",
    )
    .unwrap_err();
    assert!(error.contains("cargo install --git"));
    assert!(error.contains("cli --locked --bin nexora"));
    assert!(!error.contains("cargo build"));
}

#[test]
fn verified_download_checks_size_and_sha256_before_installation() {
    let fixture = Fixture::new("download");
    let digest = format!("{:x}", Sha256::digest(b"new"));
    let valid = fixture.root.join("valid");
    cli_update::inspect_verified_download(b"new", 3, &digest, &valid).unwrap();
    assert_eq!(fs::read(valid).unwrap(), b"new");

    let wrong_size = fixture.root.join("wrong-size");
    assert!(
        cli_update::inspect_verified_download(b"new", 4, &digest, &wrong_size)
            .unwrap_err()
            .contains("大小不匹配")
    );
    let wrong_sha = fixture.root.join("wrong-sha");
    assert!(
        cli_update::inspect_verified_download(b"new", 3, &"0".repeat(64), &wrong_sha)
            .unwrap_err()
            .contains("SHA-256")
    );
}

#[test]
fn windows_helper_contract_uses_hidden_mode_and_verified_replacement_fields() {
    assert_eq!(
        cli_update::inspect_windows_helper_arguments(
            42,
            std::path::Path::new("C:/tools/nexora.exe"),
            std::path::Path::new("C:/tools/.nexora-update.tmp.exe"),
            123,
            "abc",
        ),
        vec![
            "__update-helper",
            "--parent-pid",
            "42",
            "--target",
            "C:/tools/nexora.exe",
            "--replacement",
            "C:/tools/.nexora-update.tmp.exe",
            "--expected-size",
            "123",
            "--expected-sha256",
            "abc",
        ]
    );
}

#[test]
fn release_workflow_builds_every_manifest_target_with_the_exact_asset_name() {
    for (target, asset) in [
        ("x86_64-apple-darwin", "nexora-x86_64-apple-darwin"),
        ("aarch64-apple-darwin", "nexora-aarch64-apple-darwin"),
        (
            "x86_64-pc-windows-msvc",
            "nexora-x86_64-pc-windows-msvc.exe",
        ),
        (
            "aarch64-pc-windows-msvc",
            "nexora-aarch64-pc-windows-msvc.exe",
        ),
        (
            "x86_64-unknown-linux-gnu",
            "nexora-x86_64-unknown-linux-gnu",
        ),
        (
            "aarch64-unknown-linux-gnu",
            "nexora-aarch64-unknown-linux-gnu",
        ),
    ] {
        assert!(RELEASE_WORKFLOW.contains(&format!("target: {target}")));
        assert!(RELEASE_WORKFLOW.contains(&format!("asset: {asset}")));
        assert!(RELEASE_WORKFLOW.contains(&format!("\"{target}\": \"{asset}\"")));
    }
    assert!(RELEASE_WORKFLOW.contains("sha256"));
    assert!(RELEASE_WORKFLOW.contains("nexora-update.json"));
}

#[test]
fn github_workflows_keep_only_docs_and_reproducible_release() {
    let workflow_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".github/workflows");
    let mut names = fs::read_dir(workflow_directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["docs.yml", "release.yml"]);

    let workflow = RELEASE_WORKFLOW.replace("\r\n", "\n");
    assert!(workflow.contains("tags:\n      - \"v*\""));
    assert!(!workflow.contains("pull_request:"));
    assert!(!workflow.contains("schedule:"));
    assert!(!workflow.contains("branches:"));
    assert!(workflow.contains("--id JRSoftware.InnoSetup.7 --version 7.0.2"));
    assert!(workflow.contains("Programs\\Inno Setup 7\\ISCC.exe"));
    assert!(workflow.contains("installed_inno_setup_ -- --ignored --nocapture"));
    assert_eq!(workflow.matches("retention-days: 1").count(), 2);
}

#[test]
#[cfg(unix)]
fn unix_install_replaces_atomically_and_preserves_current_on_failure() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = Fixture::new("unix-install");
    let executable = fixture.root.join("nexora");
    let replacement = fixture.root.join("replacement");
    fs::write(&executable, b"old").unwrap();
    fs::write(&replacement, b"new").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    cli_update::install_unix(&executable, &replacement).unwrap();
    assert_eq!(fs::read(&executable).unwrap(), b"new");
    assert_ne!(
        fs::metadata(&executable).unwrap().permissions().mode() & 0o100,
        0
    );

    let missing = fixture.root.join("missing");
    let error = cli_update::install_unix(&executable, &missing).unwrap_err();
    assert!(error.contains("执行权限"));
    assert_eq!(fs::read(&executable).unwrap(), b"new");

    let locked_directory = fixture.root.join("locked");
    fs::create_dir(&locked_directory).unwrap();
    let locked_executable = locked_directory.join("nexora");
    let locked_replacement = locked_directory.join("replacement");
    fs::write(&locked_executable, b"still-usable").unwrap();
    fs::write(&locked_replacement, b"candidate").unwrap();
    fs::set_permissions(&locked_executable, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&locked_directory, fs::Permissions::from_mode(0o555)).unwrap();
    let result = cli_update::install_unix(&locked_executable, &locked_replacement);
    fs::set_permissions(&locked_directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.unwrap_err().contains("当前 CLI 保持不变"));
    assert_eq!(fs::read(locked_executable).unwrap(), b"still-usable");
}
