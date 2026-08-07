//! 独立 updater sidecar 的命令行入口和健康确认工具。

use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;

use crate::{UpdateError, macos, windows};

const INSTALL_RESULT_SCHEMA_VERSION: u32 = 1;
const INSTALL_RESULT_FILE_NAME: &str = "last-install-result.json";

/// 如果当前进程由 `--nexora-updater-sidecar apply` 启动，则执行 sidecar 安装流程。
///
/// 应用的独立 sidecar 二进制可以在 `main` 的第一行调用该函数；返回 `Ok(true)` 表示
/// sidecar 已经接管并完成执行，调用方应立即退出进程，不再启动 GPUI 或业务逻辑。
///
/// # Errors
///
/// 当 sidecar 参数缺失、macOS 事务替换失败、健康确认超时或回滚失败时返回错误。
pub fn run_sidecar_from_env_args() -> Result<bool, UpdateError> {
    let args = env::args_os().collect::<Vec<_>>();
    let Some(position) = args
        .iter()
        .position(|arg| arg == "--nexora-updater-sidecar")
    else {
        return Ok(false);
    };
    if args.get(position + 1).and_then(|arg| arg.to_str()) != Some("apply") {
        return Err(UpdateError::SidecarFailed(
            "未知 updater sidecar 子命令".to_owned(),
        ));
    }

    let command = SidecarApplyCommand::parse(&args[position + 2..])?;
    let result = apply_staged_update(&command);
    let old_version_available = !matches!(
        &result,
        Err(UpdateError::SidecarFailed(message)) if message.contains("无法恢复旧版本")
    );
    if let Err(error) = &result
        && cfg!(target_os = "windows")
        && let Err(record_error) = write_install_failure(&command, error)
    {
        tracing::warn!(error = %record_error, "无法记录 Windows 更新安装结果");
    }
    finalize_pending_record(&command, result.is_ok());
    if result.is_err()
        && cfg!(target_os = "windows")
        && old_version_available
        && !windows::process_is_running(command.parent_pid)
        && let Some(main_exe_name) = command.main_exe_name.as_deref()
        && let Err(error) = windows::relaunch_app(&command.current_app, main_exe_name)
    {
        tracing::warn!(error = %error, "Windows 更新失败后无法自动重新打开旧版本");
    }
    result?;
    Ok(true)
}

/// 如果当前进程由 sidecar 以健康确认参数启动，则写入一次性健康确认文件。
///
/// 新版本应在完成应用初始化并创建首个主窗口后调用本函数。参数不完整时返回 `Ok(false)`，
/// 方便普通启动路径无条件调用。
///
/// # Errors
///
/// 当健康确认文件路径不可写，或会话标识格式无效时返回错误。
fn apply_staged_update(command: &SidecarApplyCommand) -> Result<(), UpdateError> {
    if cfg!(target_os = "windows") {
        let main_exe_name = command.main_exe_name.as_deref().ok_or_else(|| {
            UpdateError::SidecarFailed("Windows sidecar 缺少主 EXE 文件名".to_owned())
        })?;
        return windows::apply_staged_update(
            command.parent_pid,
            main_exe_name,
            &command.current_app,
            &command.staged_app,
            &command.staging_root,
            &command.health_session,
            Duration::from_secs(command.health_timeout_seconds),
        );
    }
    macos::apply_staged_update(
        command.parent_pid,
        &command.current_app,
        &command.staged_app,
        &command.staging_root,
        &command.health_session,
        Duration::from_secs(command.health_timeout_seconds),
    )
}

/// 从当前进程参数读取 updater 健康确认会话并写入确认文件。
///
/// 主程序启动早期可调用该函数；当参数中不存在健康确认会话时返回 `Ok(false)`，
/// 存在并成功写入确认文件时返回 `Ok(true)`。
///
/// # Errors
///
/// 当健康会话标识不是 URL-safe base64、确认文件父目录无法创建，或确认文件无法写入时返回错误。
pub fn report_health_from_env_args() -> Result<bool, UpdateError> {
    let args = env::args_os().collect::<Vec<_>>();
    let Some(session) = value_after(&args, "--nexora-updater-health-session") else {
        return Ok(false);
    };
    let Some(file) = value_after(&args, "--nexora-updater-health-file") else {
        return Ok(false);
    };
    let session = session
        .into_string()
        .map_err(|_| UpdateError::InvalidHealthSession)?;
    if session.len() < 32 || URL_SAFE_NO_PAD.decode(session.as_bytes()).is_err() {
        return Err(UpdateError::InvalidHealthSession);
    }
    let file = PathBuf::from(file);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &file,
        format!(
            "{{\"session\":\"{}\",\"pid\":{}}}\n",
            session,
            std::process::id()
        ),
    )?;
    Ok(true)
}

#[derive(Debug)]
struct SidecarApplyCommand {
    app_id: String,
    parent_pid: u32,
    main_exe_name: Option<String>,
    current_app: PathBuf,
    staged_app: PathBuf,
    staging_root: PathBuf,
    health_session: String,
    health_timeout_seconds: u64,
    pending_record: Option<PathBuf>,
    installing_record: Option<PathBuf>,
}

impl SidecarApplyCommand {
    fn parse(args: &[OsString]) -> Result<Self, UpdateError> {
        let app_id = required_value(args, "--app-id")?
            .into_string()
            .map_err(|_| UpdateError::SidecarFailed("app id 不是 UTF-8".to_owned()))?;
        let parent_pid = required_value(args, "--parent-pid")?
            .into_string()
            .map_err(|_| UpdateError::SidecarFailed("parent pid 不是 UTF-8".to_owned()))?
            .parse::<u32>()
            .map_err(|_| UpdateError::SidecarFailed("parent pid 无效".to_owned()))?;
        let main_exe_name = optional_value(args, "--main-exe-name")
            .map(|value| {
                value
                    .into_string()
                    .map_err(|_| UpdateError::SidecarFailed("主 EXE 文件名不是 UTF-8".to_owned()))
            })
            .transpose()?;
        let current_app = PathBuf::from(required_value(args, "--current-app")?);
        let staged_app = PathBuf::from(required_value(args, "--staged-app")?);
        let staging_root = PathBuf::from(required_value(args, "--staging-root")?);
        let health_timeout_seconds = required_value(args, "--health-timeout-seconds")?
            .into_string()
            .map_err(|_| UpdateError::SidecarFailed("健康超时参数不是 UTF-8".to_owned()))?
            .parse::<u64>()
            .map_err(|_| UpdateError::SidecarFailed("健康超时参数无效".to_owned()))?;
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(|error| UpdateError::Random(error.to_string()))?;
        let health_session = URL_SAFE_NO_PAD.encode(random);
        let pending_record = optional_value(args, "--pending-record").map(PathBuf::from);
        let installing_record = optional_value(args, "--installing-record").map(PathBuf::from);
        if pending_record.is_some() != installing_record.is_some() {
            return Err(UpdateError::SidecarFailed(
                "待安装记录参数必须成对提供".to_owned(),
            ));
        }

        Ok(Self {
            app_id,
            parent_pid,
            main_exe_name,
            current_app,
            staged_app,
            staging_root,
            health_session,
            health_timeout_seconds,
            pending_record,
            installing_record,
        })
    }
}

fn finalize_pending_record(command: &SidecarApplyCommand, succeeded: bool) {
    let (Some(pending), Some(installing)) = (&command.pending_record, &command.installing_record)
    else {
        return;
    };

    if succeeded {
        _ = fs::remove_file(installing);
        remove_empty_pending_directory(installing);
        return;
    }

    if cfg!(target_os = "windows") {
        let backup_exists = command
            .current_app
            .file_name()
            .is_some_and(|name| command.staging_root.join("backup").join(name).exists());
        if command.current_app.exists() && !backup_exists {
            _ = fs::remove_file(installing);
            _ = fs::remove_dir_all(&command.staging_root);
            remove_empty_pending_directory(installing);
        }
        return;
    }

    if command.current_app.exists() && command.staged_app.exists() {
        _ = fs::rename(installing, pending);
        return;
    }

    let backup_exists = command
        .current_app
        .file_name()
        .is_some_and(|name| command.staging_root.join("backup").join(name).exists());
    if command.current_app.exists() && !backup_exists {
        _ = fs::remove_file(installing);
        _ = fs::remove_dir_all(&command.staging_root);
        remove_empty_pending_directory(installing);
    }
}

#[derive(Serialize)]
struct InstallResultRecord<'a> {
    schema_version: u32,
    app_id: &'a str,
    message: &'a str,
    occurred_at: u64,
}

fn write_install_failure(
    command: &SidecarApplyCommand,
    error: &UpdateError,
) -> Result<(), UpdateError> {
    let cache_dir = transaction_cache_dir(&command.staging_root)?;
    fs::create_dir_all(&cache_dir)?;
    let message = install_failure_message(error);
    let record = InstallResultRecord {
        schema_version: INSTALL_RESULT_SCHEMA_VERSION,
        app_id: &command.app_id,
        message,
        occurred_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).map_err(|error| UpdateError::Random(error.to_string()))?;
    let temporary = cache_dir.join(format!(
        ".install-result-{}.tmp",
        URL_SAFE_NO_PAD.encode(random)
    ));
    let destination = cache_dir.join(INSTALL_RESULT_FILE_NAME);
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        serde_json::to_writer(&mut file, &record)
            .map_err(UpdateError::InvalidPendingRecordSerialization)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        windows::replace_file(&temporary, &destination)
    })();
    if result.is_err() {
        _ = fs::remove_file(temporary);
    }
    result
}

fn transaction_cache_dir(staging_root: &Path) -> Result<PathBuf, UpdateError> {
    let category = staging_root
        .parent()
        .ok_or(UpdateError::InvalidPendingPath)?;
    if !matches!(
        category.file_name().and_then(|name| name.to_str()),
        Some("staging" | "pending")
    ) {
        return Err(UpdateError::InvalidPendingPath);
    }
    category
        .parent()
        .map(Path::to_path_buf)
        .ok_or(UpdateError::InvalidPendingPath)
}

fn install_failure_message(error: &UpdateError) -> &'static str {
    match error {
        UpdateError::HealthCheckTimedOut | UpdateError::InvalidHealthSession => {
            "新版本未能正常启动，已自动恢复旧版本。若应用未自动打开，请手动启动后重新下载。"
        }
        UpdateError::SidecarFailed(message) if message.contains("无法恢复旧版本") => {
            "更新安装失败，并且旧版本自动恢复未完成。请重新运行安装程序修复应用。"
        }
        _ => "更新安装未完成，已自动恢复旧版本。若应用未自动打开，请手动启动后重新下载。",
    }
}

fn remove_empty_pending_directory(record_path: &std::path::Path) {
    if let Some(cache_dir) = record_path.parent() {
        _ = fs::remove_dir(cache_dir.join("pending"));
    }
}

fn required_value(args: &[OsString], name: &str) -> Result<OsString, UpdateError> {
    value_after(args, name)
        .ok_or_else(|| UpdateError::SidecarFailed(format!("sidecar 缺少参数 `{name}`")))
}

fn value_after(args: &[OsString], name: &str) -> Option<OsString> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|position| args.get(position + 1))
        .cloned()
}

fn optional_value(args: &[OsString], name: &str) -> Option<OsString> {
    value_after(args, name)
}
