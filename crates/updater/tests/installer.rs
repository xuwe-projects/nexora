#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const INSTALL_HELPER: &str = include_str!("../assets/install-update.sh");
const NON_EXISTENT_PROCESS_ID: &str = "999999999";

#[test]
fn helper_preserves_current_app_when_initial_backup_move_fails() {
    let root = temporary_directory("backup-move-failure");
    let current_app = root.join("a".repeat(240));
    let staging_root = root.join("staging");
    let staged_app = staging_root.join("Console.app");
    let marker = current_app.join("current-version");
    fs::create_dir_all(&current_app).unwrap();
    fs::create_dir_all(&staged_app).unwrap();
    fs::write(&marker, "current").unwrap();

    let status = run_helper(&root, &current_app, &staged_app, &staging_root);

    assert!(!status.success());
    assert_eq!(fs::read_to_string(marker).unwrap(), "current");
    assert!(!staging_root.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn helper_restores_backup_when_staged_move_fails() {
    let root = temporary_directory("rollback");
    let current_app = root.join("Console.app");
    let staging_root = root.join("staging");
    let missing_staged_app = staging_root.join("Missing.app");
    let marker = current_app.join("current-version");
    fs::create_dir_all(&current_app).unwrap();
    fs::create_dir_all(&staging_root).unwrap();
    fs::write(&marker, "current").unwrap();

    let status = run_helper(&root, &current_app, &missing_staged_app, &staging_root);

    assert!(!status.success());
    assert_eq!(fs::read_to_string(marker).unwrap(), "current");
    assert!(
        !current_app
            .with_extension("app.nexora-updater-backup")
            .exists()
    );
    assert!(!staging_root.exists());
    fs::remove_dir_all(root).unwrap();
}

fn run_helper(
    root: &Path,
    current_app: &Path,
    staged_app: &Path,
    staging_root: &Path,
) -> std::process::ExitStatus {
    let script = root.join("install-update.sh");
    let fake_bin = root.join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    fs::write(&script, INSTALL_HELPER).unwrap();
    let fake_open = fake_bin.join("open");
    fs::write(&fake_open, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&fake_open, fs::Permissions::from_mode(0o755)).unwrap();

    Command::new("/bin/sh")
        .arg(script)
        .arg(NON_EXISTENT_PROCESS_ID)
        .arg(current_app)
        .arg(staged_app)
        .arg(staging_root)
        .env("PATH", format!("{}:/bin:/usr/bin", fake_bin.display()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
}

fn temporary_directory(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "nexora-updater-test-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
