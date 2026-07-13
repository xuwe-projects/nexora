//! macOS `.app.zip` 解压、签名校验与退出后替换实现。

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::UpdateError;

pub(crate) fn extract_app_archive(
    archive_path: &Path,
    destination: &Path,
) -> Result<(), UpdateError> {
    ensure_macos()?;
    let output = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(archive_path)
        .arg(destination)
        .output()?;
    command_succeeded("ditto", output)
}

pub(crate) fn find_app_bundle(root: &Path) -> Result<PathBuf, UpdateError> {
    let mut apps = Vec::new();
    collect_app_bundles(root, &mut apps)?;
    if apps.len() != 1 {
        return Err(UpdateError::InvalidAppArchive);
    }

    Ok(apps.remove(0))
}

pub(crate) fn verify_code_signature(
    app_path: &Path,
    expected_team_id: Option<&str>,
) -> Result<(), UpdateError> {
    ensure_macos()?;
    let output = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "--verbose=2"])
        .arg(app_path)
        .output()?;
    command_succeeded("codesign --verify", output)?;

    let Some(expected_team_id) = expected_team_id else {
        return Ok(());
    };

    let output = Command::new("/usr/bin/codesign")
        .args(["-d", "--verbose=4"])
        .arg(app_path)
        .output()?;
    if !output.status.success() {
        return command_succeeded("codesign -d", output);
    }

    let details = String::from_utf8_lossy(&output.stderr);
    let actual_team_id = details
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .unwrap_or("未提供")
        .to_owned();
    if actual_team_id != expected_team_id {
        return Err(UpdateError::TeamIdMismatch {
            expected: expected_team_id.to_owned(),
            actual: actual_team_id,
        });
    }

    Ok(())
}

pub(crate) fn current_app_bundle() -> Result<PathBuf, UpdateError> {
    std::env::current_exe()?
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(Path::to_path_buf)
        .ok_or(UpdateError::AppBundleNotFound)
}

pub(crate) fn spawn_install_helper(
    process_id: u32,
    current_app: &Path,
    staged_app: &Path,
    staging_root: &Path,
) -> Result<(), UpdateError> {
    ensure_macos()?;
    let helper_path = staging_root.join("install-update.sh");
    fs::write(&helper_path, INSTALL_HELPER)?;

    Command::new("/bin/sh")
        .arg(helper_path)
        .arg(process_id.to_string())
        .arg(current_app)
        .arg(staged_app)
        .arg(staging_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn ensure_macos() -> Result<(), UpdateError> {
    if cfg!(target_os = "macos") {
        return Ok(());
    }

    Err(UpdateError::UnsupportedPlatform)
}

fn collect_app_bundles(root: &Path, apps: &mut Vec<PathBuf>) -> Result<(), UpdateError> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() && path.extension().is_some_and(|extension| extension == "app") {
            apps.push(path);
            continue;
        }

        if file_type.is_dir() {
            collect_app_bundles(&path, apps)?;
        }
    }

    Ok(())
}

fn command_succeeded(
    command: &'static str,
    output: std::process::Output,
) -> Result<(), UpdateError> {
    if output.status.success() {
        return Ok(());
    }

    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(UpdateError::CommandFailed { command, message })
}

const INSTALL_HELPER: &str = r#"#!/bin/sh
pid="$1"
current_app="$2"
staged_app="$3"
staging_root="$4"
backup_app="${current_app}.xuwe-updater-backup"

while kill -0 "$pid" 2>/dev/null; do
  sleep 0.1
done

rm -rf "$backup_app"
if mv "$current_app" "$backup_app" && mv "$staged_app" "$current_app"; then
  open "$current_app"
  rm -rf "$backup_app"
  rm -rf "$staging_root"
  exit 0
fi

rm -rf "$current_app"
if [ -d "$backup_app" ]; then
  mv "$backup_app" "$current_app"
  open "$current_app"
fi
exit 1
"#;
