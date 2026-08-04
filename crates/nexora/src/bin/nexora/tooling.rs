//! Nexora CLI 的本地工具编排入口。

#[path = "tooling/lint.rs"]
mod lint;
#[path = "tooling/release.rs"]
mod release;

use clap::{Args, Subcommand, ValueEnum};
use std::{error::Error, fmt, path::PathBuf};

pub(super) use release::{BuildConfig, PublishConfig};

#[allow(unused_imports)]
pub use release::{
    inspect_app_selection, inspect_build_datetime_number, inspect_build_plans,
    inspect_build_plans_for_channel, inspect_create_windows_update_zip,
    inspect_freeze_release_notes, inspect_latest_dmg_aliases,
    inspect_latest_windows_installer_aliases, inspect_prepare_release_receipt,
    inspect_release_artifacts, inspect_release_artifacts_for_channel, inspect_release_resources,
    inspect_release_selection, inspect_signing_key, inspect_windows_installer_sources,
    validate_display_name, write_bundle_icon, write_bundle_info, write_sha256_sidecar,
};

/// CLI 命令解析与执行流程共用的结果类型。
pub type CliResult<T> = Result<T, CliError>;

/// `nexora` 执行过程中的面向用户错误。
#[derive(Debug)]
pub struct CliError {
    message: String,
}

impl CliError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CliError {}

#[derive(Args, Debug, Clone)]
pub(super) struct DoctorConfig {
    /// 缺少可自动安装的依赖时尝试安装。
    #[arg(long)]
    fix: bool,
}

#[derive(Args, Debug, Clone)]
pub(super) struct LintConfig {
    /// 要检查的 Cargo workspace 根目录。
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// 将启发式警告也视为失败。
    #[arg(long)]
    deny_warnings: bool,
    /// 诊断输出格式。
    #[arg(long, value_enum, default_value = "human")]
    format: LintOutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LintOutputFormat {
    Human,
    Json,
}

#[derive(Args, Debug, Clone)]
pub(super) struct UpdaterConfig {
    /// 要执行的 updater 管理命令。
    #[command(subcommand)]
    command: UpdaterCommand,
}

#[derive(Args, Debug, Clone)]
pub(super) struct IconsConfig {
    /// 要执行的图标资源命令。
    #[command(subcommand)]
    command: IconsCommand,
}

#[derive(Debug, Clone, Subcommand)]
enum IconsCommand {
    /// 从 app 注册的源 PNG 生成标准 PNG、ICNS 和 ICO。
    Generate {
        /// nexora.toml 中的稳定 app key。
        #[arg(long)]
        app: String,
        /// 确认覆盖标记为手工维护的品牌资源。
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum UpdaterCommand {
    /// 生成 Ed25519 更新签名密钥。
    Keygen {
        /// nexora.toml 中的稳定 app 选择器。
        #[arg(long)]
        app: String,
        /// 公钥轮换标识；省略时使用 `<app>-main`。
        #[arg(long)]
        key_id: Option<String>,
        /// 私钥输出文件；省略时只输出到终端。
        #[arg(long)]
        private_key_file: Option<PathBuf>,
    },
}

pub(super) fn run_build_command(config: BuildConfig) -> CliResult<()> {
    release::run_build_command(config)
}

pub(super) fn run_publish_command(config: PublishConfig) -> CliResult<()> {
    release::run_publish_command(config)
}

pub(super) fn run_doctor_command(config: DoctorConfig) -> CliResult<()> {
    release::run_doctor(config.fix)
}

pub(super) fn run_updater_command(config: UpdaterConfig) -> CliResult<()> {
    match config.command {
        UpdaterCommand::Keygen {
            app,
            key_id,
            private_key_file,
        } => release::run_updater_keygen(&app, key_id, private_key_file),
    }
}

pub(super) fn run_icons_command(config: IconsConfig) -> CliResult<()> {
    match config.command {
        IconsCommand::Generate { app, force } => release::run_icons_generate(&app, force),
    }
}

pub(super) fn run_lint_command(config: LintConfig) -> CliResult<()> {
    lint::run(
        &config.workspace,
        config.deny_warnings,
        config.format == LintOutputFormat::Json,
    )
}
