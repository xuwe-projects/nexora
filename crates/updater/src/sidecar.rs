//! 独立 updater sidecar 的命令行入口和健康确认工具。

use std::{env, ffi::OsString, fs, path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::{UpdateError, macos, windows};

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
    finalize_pending_record(&command, result.is_ok());
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
        return windows::apply_staged_update(
            command.parent_pid,
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
    parent_pid: u32,
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
        let app_id = required_value(args, "--app-id")?;
        let parent_pid = required_value(args, "--parent-pid")?
            .into_string()
            .map_err(|_| UpdateError::SidecarFailed("parent pid 不是 UTF-8".to_owned()))?
            .parse::<u32>()
            .map_err(|_| UpdateError::SidecarFailed("parent pid 无效".to_owned()))?;
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

        drop(app_id);

        Ok(Self {
            parent_pid,
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
