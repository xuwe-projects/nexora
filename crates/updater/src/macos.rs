//! macOS `.app.zip` 解压、签名校验与退出后替换实现。

use std::{
    fs::{self, File},
    io::Read as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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

pub(crate) fn default_sidecar_path() -> Result<PathBuf, UpdateError> {
    let executable = std::env::current_exe()?;
    let executable_name = executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| UpdateError::SidecarUnavailable("当前可执行文件名无效".to_owned()))?;
    let helpers = executable
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "Contents"))
        .map(|contents| contents.join("Helpers"))
        .ok_or(UpdateError::AppBundleNotFound)?;

    Ok(helpers.join(format!("{executable_name}-updater")))
}

pub(crate) struct InstallHelperRequest<'a> {
    pub process_id: u32,
    pub app_id: &'a str,
    pub current_app: &'a Path,
    pub staged_app: &'a Path,
    pub staging_root: &'a Path,
    pub sidecar_path: &'a Path,
    pub health_timeout: Duration,
    pub pending_records: Option<(&'a Path, &'a Path)>,
    pub operation_log_session: Option<&'a str>,
}

pub(crate) fn spawn_install_helper(request: InstallHelperRequest<'_>) -> Result<(), UpdateError> {
    ensure_macos()?;
    if !request.sidecar_path.is_file() {
        return Err(UpdateError::SidecarUnavailable(format!(
            "`{}` 不存在或不是文件",
            request.sidecar_path.display()
        )));
    }

    let sidecar_runtime = copy_sidecar_to_temp(request.app_id, request.sidecar_path)?;
    let mut command = Command::new(sidecar_runtime);
    command
        .arg("--nexora-updater-sidecar")
        .arg("apply")
        .arg("--app-id")
        .arg(request.app_id)
        .arg("--parent-pid")
        .arg(request.process_id.to_string())
        .arg("--current-app")
        .arg(request.current_app)
        .arg("--staged-app")
        .arg(request.staged_app)
        .arg("--staging-root")
        .arg(request.staging_root)
        .arg("--health-timeout-seconds")
        .arg(request.health_timeout.as_secs().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some((pending_record, installing_record)) = request.pending_records {
        command
            .arg("--pending-record")
            .arg(pending_record)
            .arg("--installing-record")
            .arg(installing_record);
    }
    if let Some(session) = request.operation_log_session {
        command.arg("--operation-log-session").arg(session);
    }
    command.spawn()?;
    Ok(())
}

pub(crate) fn apply_staged_update(
    parent_pid: u32,
    current_app: &Path,
    staged_app: &Path,
    staging_root: &Path,
    health_session: &str,
    health_timeout: Duration,
) -> Result<(), UpdateError> {
    ensure_macos()?;
    wait_for_process_exit(parent_pid, Duration::from_secs(120))?;

    let app_name = current_app
        .file_name()
        .ok_or_else(|| UpdateError::SidecarFailed("当前 .app 路径缺少文件名".to_owned()))?;
    let backup_root = staging_root.join("backup");
    let failed_root = staging_root.join("failed");
    let health_file = staging_root.join("health").join("session.json");
    let backup_app = backup_root.join(app_name);
    let failed_app = failed_root.join(app_name);

    fs::create_dir_all(&backup_root)?;
    fs::create_dir_all(&failed_root)?;
    if let Some(parent) = health_file.parent() {
        fs::create_dir_all(parent)?;
    }

    if backup_app.exists() {
        fs::remove_dir_all(&backup_app)?;
    }
    fs::rename(current_app, &backup_app).map_err(|error| {
        UpdateError::SidecarFailed(format!(
            "无法备份旧版本 `{}`: {error}",
            current_app.display()
        ))
    })?;
    if let Err(error) = fs::rename(staged_app, current_app) {
        restore_backup(current_app, &backup_app)?;
        return Err(UpdateError::SidecarFailed(format!(
            "无法替换新版本 `{}`: {error}",
            current_app.display()
        )));
    }

    let launched = launch_app(current_app, health_session, &health_file);
    let healthy =
        launched.and_then(|()| wait_for_health(&health_file, health_session, health_timeout));
    if let Err(error) = healthy {
        if current_app.exists() {
            let _ = fs::rename(current_app, &failed_app);
        }
        restore_backup(current_app, &backup_app)?;
        _ = launch_app(current_app, "", Path::new(""));
        return Err(error);
    }

    _ = fs::remove_dir_all(&backup_root);
    _ = fs::remove_dir_all(staging_root);
    Ok(())
}

fn ensure_macos() -> Result<(), UpdateError> {
    if cfg!(target_os = "macos") {
        return Ok(());
    }

    Err(UpdateError::UnsupportedPlatform)
}

fn copy_sidecar_to_temp(app_id: &str, sidecar_path: &Path) -> Result<PathBuf, UpdateError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| UpdateError::Random(error.to_string()))?;
    let nonce = random.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("写入 String 不会失败");
        output
    });
    let temp_root = std::env::temp_dir()
        .join("nexora-updater-sidecar")
        .join(app_id)
        .join(format!("{timestamp}-{nonce}"));
    fs::create_dir_all(&temp_root)?;
    let sidecar_name = sidecar_path
        .file_name()
        .ok_or_else(|| UpdateError::SidecarUnavailable("sidecar 路径缺少文件名".to_owned()))?;
    let runtime_path = temp_root.join(sidecar_name);
    fs::copy(sidecar_path, &runtime_path).map_err(|error| {
        UpdateError::SidecarUnavailable(format!(
            "无法复制 `{}` 到 `{}`: {error}",
            sidecar_path.display(),
            runtime_path.display()
        ))
    })?;
    let permissions = fs::metadata(sidecar_path)?.permissions();
    fs::set_permissions(&runtime_path, permissions)?;
    Ok(runtime_path)
}

fn wait_for_process_exit(process_id: u32, timeout: Duration) -> Result<(), UpdateError> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        let running = Command::new("/bin/kill")
            .arg("-0")
            .arg(process_id.to_string())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !running {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }

    Err(UpdateError::SidecarFailed(format!(
        "主应用进程 {process_id} 未在限定时间内退出"
    )))
}

fn restore_backup(current_app: &Path, backup_app: &Path) -> Result<(), UpdateError> {
    if current_app.exists() {
        fs::remove_dir_all(current_app)?;
    }
    fs::rename(backup_app, current_app).map_err(|error| {
        UpdateError::SidecarFailed(format!(
            "无法恢复旧版本 `{}`: {error}",
            current_app.display()
        ))
    })
}

fn launch_app(
    app_path: &Path,
    health_session: &str,
    health_file: &Path,
) -> Result<(), UpdateError> {
    let mut command = Command::new("/usr/bin/open");
    command.arg("-n").arg(app_path);
    if !health_session.is_empty() {
        command
            .arg("--args")
            .arg("--nexora-updater-health-session")
            .arg(health_session)
            .arg("--nexora-updater-health-file")
            .arg(health_file);
    }
    let output = command.output()?;
    command_succeeded("open", output)
}

fn wait_for_health(
    health_file: &Path,
    health_session: &str,
    timeout: Duration,
) -> Result<(), UpdateError> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Ok(mut file) = File::open(health_file) {
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            if contents.contains(health_session) {
                return Ok(());
            }
            return Err(UpdateError::InvalidHealthSession);
        }
        thread::sleep(Duration::from_millis(250));
    }

    Err(UpdateError::HealthCheckTimedOut)
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
