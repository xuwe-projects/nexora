//! `nexora build` 与 `nexora publish` 的配置、打包和发布实现。

use super::{CliError, CliResult};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{FixedOffset, Local, TimeZone as _, Utc};
use clap::{Args, Subcommand};
use dialoguer::{Confirm, MultiSelect};
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use hmac::{Hmac, Mac as _};
use image::{GenericImageView as _, ImageFormat, imageops::FilterType};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, OpenOptions},
    io::{self, IsTerminal as _, Write as _},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use updater::{
    ApplicationReleaseMetadata, MAX_RELEASE_NOTES_BYTES, RELEASE_METADATA_FILE_NAME,
    RELEASE_NOTES_FILE_NAME, ReleaseNotesMetadata, UpdateChannel, verify_release_notes_bytes,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const CONFIG_FILE_NAME: &str = "nexora.toml";
const ARTIFACT_SCHEMA_VERSION: u32 = 1;
const RELEASE_RECEIPT_SCHEMA_VERSION: u32 = 3;
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const DIST_DIRECTORY: &str = "dist";
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";
const MUTABLE_CACHE: &str = "no-cache";
const MINIMUM_GPUI_WINDOWS_BUILD: u32 = 15_063;
const INNO_SETUP_VERSION: &str = "6.7.3";
const INNO_SETUP_WINGET_PACKAGE: &str = "JRSoftware.InnoSetup";
const WINDOWS_INSTALLER_TEMPLATE: &str = include_str!("../../templates/windows/installer.iss");
const HOMEBREW_INSTALL_COMMAND: &str = "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"";
const RUSTUP_INSTALL_COMMAND: &str =
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y";
const WINDOWS_SDK_WINGET_PACKAGE: &str = "Microsoft.WindowsSDK.10.0.26100";
const WINDOWS_SDK_DOWNLOAD_URL: &str =
    "https://developer.microsoft.com/windows/downloads/windows-sdk/";

/// `nexora build` 的 app/channel 多选参数。
#[derive(Args, Debug, Clone)]
pub(crate) struct BuildConfig {
    /// `nexora.toml` 中的 app key；可以重复传入。
    #[arg(long, conflicts_with = "all_apps")]
    app: Vec<String>,
    /// 选择全部 app。
    #[arg(long)]
    all_apps: bool,
    /// 要构建的 release channel；可以重复传入。
    #[arg(long, conflicts_with = "all_channels")]
    channel: Vec<String>,
    /// 为每个选中 app 构建全部 channel。
    #[arg(long)]
    all_channels: bool,
    /// 显式指定构建 target；省略时从当前 rustc host 自动推导。
    #[arg(long = "target")]
    target: Vec<String>,
}

/// `nexora publish` 的操作型参数。
#[derive(Args, Debug, Clone)]
pub(crate) struct PublishConfig {
    /// `nexora.toml` 中的 app key；可以重复传入。
    #[arg(long, conflicts_with = "all_apps")]
    app: Vec<String>,
    /// 明确发布全部 app；`--all` 是兼容旧命令的别名。
    #[arg(long = "all-apps", visible_alias = "all")]
    all_apps: bool,
    /// 要发布或撤回的 release channel；可以重复传入。
    #[arg(long, conflicts_with = "all_channels")]
    channel: Vec<String>,
    /// 为每个选中 app 处理全部 channel。
    #[arg(long)]
    all_channels: bool,
    /// 完成全部只读预检并输出计划，但不上传。
    #[arg(long)]
    dry_run: bool,
    /// 跳过上传前的交互确认。
    #[arg(long)]
    yes: bool,
    /// 发布控制操作；省略时发布可安装版本。
    #[command(subcommand)]
    command: Option<PublishCommand>,
}

#[derive(Debug, Clone, Copy, Subcommand)]
enum PublishCommand {
    /// 撤回当前 release 配置指向的版本和 build。
    Yank,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectConfig {
    schema_version: u32,
    publish: PublishConfigFile,
    apps: BTreeMap<String, AppConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishConfigFile {
    targets: BTreeMap<String, PublishTarget>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishTarget {
    provider: String,
    endpoint: String,
    bucket: String,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    force_path_style: bool,
    public_base_url: String,
    #[serde(default)]
    allow_insecure_http: bool,
    #[serde(default)]
    channels: BTreeMap<String, PublishTargetOverride>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishTargetOverride {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    bucket: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    force_path_style: Option<bool>,
    #[serde(default)]
    public_base_url: Option<String>,
    #[serde(default)]
    allow_insecure_http: Option<bool>,
}

impl PublishTarget {
    fn for_channel(&self, channel: &str) -> Self {
        let Some(override_target) = self.channels.get(channel) else {
            let mut target = self.clone();
            target.channels.clear();
            return target;
        };
        Self {
            provider: override_target
                .provider
                .clone()
                .unwrap_or_else(|| self.provider.clone()),
            endpoint: override_target
                .endpoint
                .clone()
                .unwrap_or_else(|| self.endpoint.clone()),
            bucket: override_target
                .bucket
                .clone()
                .unwrap_or_else(|| self.bucket.clone()),
            region: override_target
                .region
                .clone()
                .or_else(|| self.region.clone()),
            force_path_style: override_target
                .force_path_style
                .unwrap_or(self.force_path_style),
            public_base_url: override_target
                .public_base_url
                .clone()
                .unwrap_or_else(|| self.public_base_url.clone()),
            allow_insecure_http: override_target
                .allow_insecure_http
                .unwrap_or(self.allow_insecure_http),
            channels: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppConfig {
    package: String,
    app_id: String,
    display_name: String,
    publish_target: String,
    object_prefix: String,
    branding: BrandingConfig,
    release: Option<ReleaseConfig>,
    updater: UpdaterConfigFile,
    #[serde(default)]
    targets: TargetConfig,
    platforms: PlatformConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrandingConfig {
    application_logo: String,
    icon_source: String,
    #[serde(default)]
    managed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseConfig {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    default_channel: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    build_number: Option<BuildNumberConfig>,
    #[serde(default)]
    minimum_supported_version: Option<String>,
    #[serde(default)]
    signing_key_file: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    channels: BTreeMap<String, ReleaseChannelConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseChannelConfig {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    build_number: Option<BuildNumberConfig>,
    #[serde(default)]
    minimum_supported_version: Option<String>,
    #[serde(default)]
    runtime_config: Option<String>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum BuildNumberConfig {
    Literal(u64),
    Expression(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdaterConfigFile {
    enabled: bool,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    check_on_launch: bool,
    #[serde(default)]
    feed_url: String,
    #[serde(default)]
    channels: Vec<String>,
    #[serde(default)]
    trusted_public_keys: Vec<String>,
    #[serde(default)]
    signing_key_env: String,
    #[serde(default = "default_check_interval")]
    check_interval: String,
    #[serde(default = "default_check_jitter")]
    check_jitter: String,
    #[serde(default = "default_offline_grace_period")]
    offline_grace_period: String,
    #[serde(default = "default_mandatory_restart_delay")]
    mandatory_restart_delay: String,
    #[serde(default = "default_health_timeout")]
    health_timeout: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetConfig {
    #[serde(default)]
    required: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformConfig {
    macos: MacOsConfig,
    windows: WindowsConfig,
    linux: LinuxConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct MacOsConfig {
    icon: String,
    signing: SigningMode,
    notarize: bool,
    #[serde(default)]
    expected_team_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowsConfig {
    icon: String,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    signing: WindowsSigningMode,
    #[serde(default)]
    signing_thumbprint: Option<String>,
    #[serde(default)]
    timestamp_url: Option<String>,
    #[serde(default)]
    expected_publisher: Option<String>,
    #[serde(default)]
    desktop_shortcut_default: bool,
    #[serde(default = "default_start_menu_shortcut")]
    start_menu_shortcut_default: bool,
    #[serde(default = "default_launch_after_install")]
    launch_after_install_default: bool,
    #[serde(default)]
    minimum_windows_build: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinuxConfig {
    icons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SigningMode {
    DeveloperId,
    AdHoc,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum WindowsSigningMode {
    #[default]
    None,
    Authenticode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildTargetPlatform {
    MacOs,
    Windows,
}

#[derive(Debug)]
struct ProjectDocument {
    root: PathBuf,
    config: ProjectConfig,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum VersionSource {
    CargoPkgVersion,
    Literal,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BuildNumberSource {
    BuildDatetime,
    Literal,
}

#[derive(Debug, Clone)]
enum ResolvedBuildNumber {
    BuildDatetime,
    Literal(u64),
}

#[derive(Debug, Clone)]
struct ResolvedReleaseConfig {
    channel: String,
    version: Version,
    version_source: VersionSource,
    build_number: ResolvedBuildNumber,
    build_number_source: BuildNumberSource,
    minimum_supported_version: Version,
    runtime_config: PathBuf,
    runtime_config_source: String,
    runtime_config_sha256: String,
    updater_feed: String,
    notes_source: Option<String>,
}

#[derive(Debug, Clone)]
struct ValidatedRelease {
    channel: String,
    version: Version,
    build_number: u64,
    version_source: VersionSource,
    build_number_source: BuildNumberSource,
    minimum_supported_version: Version,
    runtime_config: PathBuf,
    runtime_config_source: String,
    runtime_config_sha256: String,
    updater_feed: String,
    targets: Vec<String>,
    notes_source: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ReleaseReceipt {
    schema_version: u32,
    app_key: String,
    package: String,
    channel: String,
    version: String,
    build_number: u64,
    version_source: VersionSource,
    build_number_source: BuildNumberSource,
    created_at: i64,
    targets: Vec<String>,
    runtime_config_source: String,
    runtime_config_sha256: String,
    updater_feed: String,
    notes_source: Option<String>,
}

#[derive(Debug, Clone)]
struct BuildPlan {
    project_root: PathBuf,
    app_key: String,
    package: String,
    app_id: String,
    updater_app_id: String,
    display_name: String,
    release: ValidatedRelease,
    target: String,
    platform: BuildTargetPlatform,
    signing: SigningMode,
    notarize: bool,
    expected_team_id: Option<String>,
    allow_insecure_http: bool,
    updater: Option<UpdaterConfigFile>,
    macos_icon: PathBuf,
    windows_icon: PathBuf,
    windows: Option<WindowsBuildOptions>,
    app_path: PathBuf,
    app_zip_path: PathBuf,
    dmg_path: PathBuf,
    setup_path: PathBuf,
    artifact_path: PathBuf,
    notes_path: PathBuf,
    notes: Option<ReleaseNotesMetadata>,
}

#[derive(Debug, Clone)]
struct FrozenReleaseNotes {
    path: PathBuf,
    metadata: ReleaseNotesMetadata,
}

#[derive(Debug, Clone)]
struct WindowsBuildOptions {
    publisher: String,
    signing: WindowsSigningMode,
    signing_thumbprint: Option<String>,
    timestamp_url: Option<String>,
    expected_publisher: Option<String>,
    desktop_shortcut_default: bool,
    start_menu_shortcut_default: bool,
    launch_after_install_default: bool,
    minimum_windows_build: u32,
}

#[derive(Debug, Clone, Serialize)]
struct BundledUpdaterConfig {
    schema_version: u32,
    app_id: String,
    channel: String,
    feed_url: String,
    trusted_public_keys: Vec<String>,
    current_version: String,
    current_build_number: u64,
    allow_insecure_http: bool,
    health_timeout: String,
    expected_team_id: Option<String>,
    expected_windows_signer_thumbprint: Option<String>,
    expected_windows_publisher: Option<String>,
    check_on_launch: bool,
}

#[derive(Debug, Clone)]
struct BrandAssets {
    application_logo: PathBuf,
    icon_source: PathBuf,
    macos_icon: PathBuf,
    windows_icon: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ArtifactManifest {
    schema_version: u32,
    app_id: String,
    channel: String,
    version: String,
    build_number: u64,
    target: String,
    artifacts: Vec<ArtifactEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ArtifactEntry {
    kind: ArtifactKind,
    file_name: String,
    sha256: String,
    size: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    MacosAppZip,
    MacosDmg,
    WindowsSetupExe,
    #[serde(rename = "windows_update_zip")]
    WindowsZip,
}

#[derive(Debug, Clone)]
struct LocalArtifact {
    target: String,
    kind: ArtifactKind,
    path: PathBuf,
    file_name: String,
    sha256: String,
    size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SignedManifest {
    schema_version: u32,
    payload: ManifestPayload,
    signatures: Vec<ManifestSignature>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ManifestPayload {
    manifest_sequence: u64,
    app_id: String,
    channel: String,
    version: Version,
    build_number: u64,
    minimum_supported_version: Version,
    published_at: i64,
    status: ReleaseStatus,
    notes_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notes_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notes_size: Option<u64>,
    artifacts: Vec<ManifestArtifact>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReleaseStatus {
    Available,
    Yanked,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ManifestArtifact {
    target: String,
    url: String,
    sha256: String,
    size: u64,
    kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ManifestSignature {
    key_id: String,
    algorithm: String,
    signature: String,
}

#[derive(Debug, Clone)]
struct Upload {
    key: String,
    source: UploadSource,
    content_type: &'static str,
    cache_control: &'static str,
    immutable: bool,
}

#[derive(Debug, Clone)]
enum UploadSource {
    File(PathBuf),
    Bytes(Vec<u8>),
}

impl UploadSource {
    fn bytes(&self) -> CliResult<Vec<u8>> {
        match self {
            Self::File(path) => fs::read(path).map_err(|error| {
                CliError::new(format!("无法读取发布文件 `{}`: {error}", path.display()))
            }),
            Self::Bytes(bytes) => Ok(bytes.clone()),
        }
    }
}

impl ReleaseReceipt {
    fn validated_release(&self, configured: &ResolvedReleaseConfig) -> CliResult<ValidatedRelease> {
        if self.schema_version != RELEASE_RECEIPT_SCHEMA_VERSION {
            return Err(CliError::new(format!(
                "release receipt schema_version {} 不受支持",
                self.schema_version
            )));
        }
        let version = Version::parse(&self.version)
            .map_err(|error| CliError::new(format!("release receipt version 非法: {error}")))?;
        if self.build_number == 0 {
            return Err(CliError::new("release receipt build_number 必须大于 0"));
        }
        Ok(ValidatedRelease {
            channel: self.channel.clone(),
            version,
            build_number: self.build_number,
            version_source: self.version_source,
            build_number_source: self.build_number_source,
            minimum_supported_version: configured.minimum_supported_version.clone(),
            runtime_config: configured.runtime_config.clone(),
            runtime_config_source: configured.runtime_config_source.clone(),
            runtime_config_sha256: configured.runtime_config_sha256.clone(),
            updater_feed: configured.updater_feed.clone(),
            targets: self.targets.clone(),
            notes_source: configured.notes_source.clone(),
        })
    }
}

impl ResolvedReleaseConfig {
    fn validated_release(&self, build_number: u64, targets: Vec<String>) -> ValidatedRelease {
        ValidatedRelease {
            channel: self.channel.clone(),
            version: self.version.clone(),
            build_number,
            version_source: self.version_source,
            build_number_source: self.build_number_source,
            minimum_supported_version: self.minimum_supported_version.clone(),
            runtime_config: self.runtime_config.clone(),
            runtime_config_source: self.runtime_config_source.clone(),
            runtime_config_sha256: self.runtime_config_sha256.clone(),
            updater_feed: self.updater_feed.clone(),
            targets,
            notes_source: self.notes_source.clone(),
        }
    }
}

#[derive(Debug)]
struct PublishPlan {
    app_key: String,
    display_name: String,
    publish_target_name: String,
    target: PublishTarget,
    trusted_keys: Vec<TrustedKey>,
    observed_sequence: Option<u64>,
    sequence: u64,
    release: ValidatedRelease,
    required_targets: Vec<String>,
    immutable_payloads: Vec<Upload>,
    sequence_manifest: Upload,
    channel_artifacts: Vec<Upload>,
    latest: Upload,
    latest_json: Vec<u8>,
    immutable_verify_urls: Vec<Verification>,
    channel_verify_urls: Vec<Verification>,
    latest_url: String,
}

#[derive(Debug)]
struct Verification {
    url: String,
    expected_sha256: String,
    label: String,
}

#[derive(Debug, Clone)]
struct TrustedKey {
    key_id: String,
    key: VerifyingKey,
}

/// 执行零参数配置驱动的桌面构建。
pub(super) fn run_build_command(config: BuildConfig) -> CliResult<()> {
    ensure_supported_build_host()?;
    let project = ProjectDocument::discover()?;
    let selections = project.select_release_targets(
        &config.app,
        config.all_apps,
        &config.channel,
        config.all_channels,
        terminal_is_interactive(),
    )?;
    ensure_rust_toolchain(true)?;
    for (app_key, channel) in selections {
        let app = &project.config.apps[&app_key];
        let package_version = cargo_package_version(&project.root, &app.package, false)?;
        let configured =
            project.resolved_release(&app_key, app, &channel, Some(package_version))?;
        let targets = project.resolve_build_targets(app_key.as_str(), app, &config.target)?;
        let receipt = project.prepare_build_receipt(&app_key, app, &configured, &targets)?;
        let release = receipt.validated_release(&configured)?;
        let frozen_notes = freeze_release_notes(&project.root, &app_key, app, &release)?;
        let plans = project.build_plans(&app_key, &release, &targets, frozen_notes.as_ref())?;
        for plan in &plans {
            execute_build(plan)?;
        }
    }
    Ok(())
}

/// 执行只发布既有产物的发布流程。
pub(super) fn run_publish_command(config: PublishConfig) -> CliResult<()> {
    let interactive = terminal_is_interactive();
    if !config.dry_run && !config.yes && !interactive {
        return Err(CliError::new("非交互 publish 必须提供 `--yes`"));
    }
    let project = ProjectDocument::discover()?;
    let selections = project.select_release_targets(
        &config.app,
        config.all_apps,
        &config.channel,
        config.all_channels,
        interactive,
    )?;
    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|error| CliError::new(format!("无法创建 HTTP 客户端: {error}")))?;
    let status = if matches!(config.command, Some(PublishCommand::Yank)) {
        ReleaseStatus::Yanked
    } else {
        ReleaseStatus::Available
    };
    let plans = selections
        .iter()
        .map(|(app_key, channel)| project.publish_plan(app_key, channel, status, &client))
        .collect::<CliResult<Vec<_>>>()?;

    for plan in &plans {
        preflight_immutable_objects(
            &client,
            &plan.target,
            plan.immutable_payloads
                .iter()
                .chain(std::iter::once(&plan.sequence_manifest)),
        )?;
    }

    for plan in &plans {
        print_publish_summary(plan, config.dry_run);
    }
    if config.dry_run {
        println!("dry-run: 已完成远端读取与全部预检，没有写入任何对象");
        return Ok(());
    }
    if !config.yes
        && !Confirm::new()
            .with_prompt("确认发布？")
            .default(false)
            .interact()
            .map_err(|error| CliError::new(format!("无法读取发布确认: {error}")))?
    {
        return Err(CliError::new("已取消发布"));
    }

    for plan in &plans {
        let credentials = S3Credentials::from_env(&plan.release.channel)?;
        publish_plan(plan, &client, &credentials)?;
    }
    Ok(())
}

impl ProjectDocument {
    fn discover() -> CliResult<Self> {
        let start = env::current_dir()
            .map_err(|error| CliError::new(format!("无法读取当前目录: {error}")))?;
        let config_path = start
            .ancestors()
            .map(|directory| directory.join(CONFIG_FILE_NAME))
            .find(|path| path.is_file())
            .ok_or_else(|| {
                CliError::new(format!(
                    "从 `{}` 向上没有找到 {CONFIG_FILE_NAME}",
                    start.display()
                ))
            })?;
        Self::load(config_path)
    }

    fn load(config_path: PathBuf) -> CliResult<Self> {
        let contents = fs::read_to_string(&config_path).map_err(|error| {
            CliError::new(format!("无法读取 `{}`: {error}", config_path.display()))
        })?;
        let config: ProjectConfig = toml::from_str(&contents).map_err(|error| {
            CliError::new(format!("无法解析 `{}`: {error}", config_path.display()))
        })?;
        if config.schema_version != 1 {
            return Err(CliError::new(format!(
                "不支持 nexora.toml schema_version {}，当前只支持 1",
                config.schema_version
            )));
        }
        if config.apps.is_empty() {
            return Err(CliError::new("nexora.toml 没有配置任何 app"));
        }
        let root = config_path
            .parent()
            .ok_or_else(|| CliError::new("nexora.toml 路径缺少父目录"))?
            .to_path_buf();
        let document = Self { root, config };
        document.validate_project()?;
        Ok(document)
    }

    fn validate_project(&self) -> CliResult<()> {
        for (name, target) in &self.config.publish.targets {
            validate_safe_component(name, "publish target 名称")?;
            validate_publish_target(target)?;
            for channel in target.channels.keys() {
                validate_channel_name(channel)?;
                validate_publish_target(&target.for_channel(channel))?;
            }
        }
        for (app_key, app) in &self.config.apps {
            validate_safe_component(app_key, "app key")?;
            validate_safe_component(&app.package, "package")?;
            validate_app_id(&app.app_id)?;
            validate_display_name(&app.display_name)?;
            if !app.object_prefix.is_empty() {
                validate_safe_component(&app.object_prefix, "object_prefix")?;
            }
            validate_workspace_relative_path(&app.branding.application_logo, "application_logo")?;
            validate_workspace_relative_path(&app.branding.icon_source, "icon_source")?;
            validate_workspace_relative_path(&app.platforms.macos.icon, "macOS icon")?;
            validate_workspace_relative_path(&app.platforms.windows.icon, "Windows icon")?;
            if app.platforms.linux.icons.is_empty() {
                return Err(CliError::new(format!(
                    "app `{app_key}` 的 platforms.linux.icons 不能为空"
                )));
            }
            for icon in &app.platforms.linux.icons {
                validate_workspace_relative_path(icon, "Linux icon")?;
            }
            if let Some(updater_app_id) = app.updater.app_id.as_deref() {
                validate_app_id(updater_app_id)?;
            }
            if !self
                .config
                .publish
                .targets
                .contains_key(&app.publish_target)
            {
                return Err(CliError::new(format!(
                    "app `{app_key}` 引用了不存在的 publish target `{}`",
                    app.publish_target
                )));
            }
            let channels = self.release_channel_names(app_key, app)?;
            let mut seen = BTreeSet::new();
            for target in &app.targets.required {
                validate_required_target(target)?;
                if !seen.insert(target) {
                    return Err(CliError::new(format!(
                        "app `{app_key}` 重复声明 required target `{target}`"
                    )));
                }
            }
            if app.platforms.macos.notarize
                && app.platforms.macos.signing != SigningMode::DeveloperId
            {
                return Err(CliError::new(format!(
                    "app `{app_key}` 启用 macOS notarize 时 signing 必须是 developer_id"
                )));
            }
            if app.updater.enabled {
                if let Some(channel) = channels
                    .iter()
                    .find(|channel| !app.updater.channels.contains(channel))
                {
                    return Err(CliError::new(format!(
                        "app `{app_key}` 的 release channel `{channel}` 不属于 updater.channels"
                    )));
                }
                if app.updater.trusted_public_keys.is_empty() {
                    return Err(CliError::new(format!(
                        "app `{app_key}` 必须配置 trusted_public_keys"
                    )));
                }
                for (label, value) in [
                    ("check_interval", &app.updater.check_interval),
                    ("check_jitter", &app.updater.check_jitter),
                    ("offline_grace_period", &app.updater.offline_grace_period),
                    (
                        "mandatory_restart_delay",
                        &app.updater.mandatory_restart_delay,
                    ),
                    ("health_timeout", &app.updater.health_timeout),
                ] {
                    if value.trim().is_empty() {
                        return Err(CliError::new(format!(
                            "app `{app_key}` 的 updater.{label} 不能为空"
                        )));
                    }
                }
                parse_trusted_keys(&app.updater.trusted_public_keys)?;
                let release = app.release.as_ref().ok_or_else(|| {
                    CliError::new(format!("app `{app_key}` 缺少 [apps.{app_key}.release]"))
                })?;
                if !release.channels.is_empty() && !app.updater.feed_url.trim().is_empty() {
                    return Err(CliError::new(format!(
                        "app `{app_key}` 使用 release.channels 时不能配置静态 updater.feed_url；请删除该字段，Nexora 会按 channel 生成"
                    )));
                }
            }
        }
        Ok(())
    }

    fn resolve_build_targets(
        &self,
        app_key: &str,
        app: &AppConfig,
        explicit: &[String],
    ) -> CliResult<Vec<String>> {
        let requested = if !explicit.is_empty() {
            explicit.to_vec()
        } else if !app.targets.required.is_empty() {
            app.targets.required.clone()
        } else {
            vec![rustc_host_target()?]
        };
        let mut targets = Vec::new();
        for target in requested {
            validate_required_target(&target)?;
            if !host_can_build(&target) {
                return Err(CliError::new(format!(
                    "当前宿主不能构建 app `{app_key}` 的 target `{target}`"
                )));
            }
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        Ok(targets)
    }

    fn release_channel_names(&self, app_key: &str, app: &AppConfig) -> CliResult<Vec<String>> {
        let release = app.release.as_ref().ok_or_else(|| {
            CliError::new(format!("app `{app_key}` 缺少 [apps.{app_key}.release]"))
        })?;
        if release.channel.is_some() && !release.channels.is_empty() {
            return Err(CliError::new(format!(
                "app `{app_key}` 不能同时使用 release.channel 和 release.channels"
            )));
        }
        if let Some(channel) = release.channel.as_deref() {
            validate_channel_name(channel)?;
            if release.default_channel.is_some() {
                return Err(CliError::new(format!(
                    "app `{app_key}` 的单通道配置不能同时声明 default_channel"
                )));
            }
            return Ok(vec![channel.to_owned()]);
        }
        if release.channels.is_empty() {
            return Err(CliError::new(format!(
                "app `{app_key}` 必须声明 release.channel 或 release.channels"
            )));
        }
        for channel in release.channels.keys() {
            validate_channel_name(channel)?;
        }
        let default_channel = release.default_channel.as_deref().ok_or_else(|| {
            CliError::new(format!(
                "app `{app_key}` 使用 release.channels 时必须声明 default_channel"
            ))
        })?;
        if !release.channels.contains_key(default_channel) {
            return Err(CliError::new(format!(
                "app `{app_key}` 的 default_channel `{default_channel}` 不存在于 release.channels"
            )));
        }
        Ok(release.channels.keys().cloned().collect())
    }

    fn default_release_channel(&self, app_key: &str, app: &AppConfig) -> CliResult<String> {
        let release = app.release.as_ref().ok_or_else(|| {
            CliError::new(format!("app `{app_key}` 缺少 [apps.{app_key}.release]"))
        })?;
        self.release_channel_names(app_key, app)?;
        release
            .default_channel
            .clone()
            .or_else(|| release.channel.clone())
            .ok_or_else(|| CliError::new(format!("app `{app_key}` 缺少默认 channel")))
    }

    fn resolved_release(
        &self,
        app_key: &str,
        app: &AppConfig,
        selected_channel: &str,
        package_version: Option<Version>,
    ) -> CliResult<ResolvedReleaseConfig> {
        let release = app.release.as_ref().ok_or_else(|| {
            CliError::new(format!("app `{app_key}` 缺少 [apps.{app_key}.release]"))
        })?;
        let channels = self.release_channel_names(app_key, app)?;
        if !channels.iter().any(|channel| channel == selected_channel) {
            return Err(CliError::new(format!(
                "app `{app_key}` 不支持 channel `{selected_channel}`；可用值：{}",
                channels.join(", ")
            )));
        }
        if app.updater.enabled
            && !app
                .updater
                .channels
                .iter()
                .any(|channel| channel == selected_channel)
        {
            return Err(CliError::new(format!(
                "app `{app_key}` 的 release channel `{selected_channel}` 不属于 updater.channels"
            )));
        }
        let channel = release.channels.get(selected_channel);
        let version_value = channel
            .and_then(|channel| channel.version.as_deref())
            .or(release.version.as_deref())
            .ok_or_else(|| {
                CliError::new(format!(
                    "app `{app_key}` channel `{selected_channel}` 合并后缺少 release.version"
                ))
            })?;
        let (version, version_source) = match version_value {
            "${CARGO_PKG_VERSION}" => (
                package_version
                    .map(Ok)
                    .unwrap_or_else(|| cargo_package_version(&self.root, &app.package, false))?,
                VersionSource::CargoPkgVersion,
            ),
            value if value.contains("${") => {
                return Err(CliError::new(format!(
                    "app `{app_key}` 的 release.version 包含不支持的表达式 `{value}`；只支持完整字段值 `${{CARGO_PKG_VERSION}}`"
                )));
            }
            value => (
                Version::parse(value).map_err(|error| {
                    CliError::new(format!(
                        "app `{app_key}` 的 release.version `{value}` 不是合法 SemVer: {error}"
                    ))
                })?,
                VersionSource::Literal,
            ),
        };
        let build_number_value = channel
            .and_then(|channel| channel.build_number.as_ref())
            .or(release.build_number.as_ref())
            .ok_or_else(|| {
                CliError::new(format!(
                    "app `{app_key}` channel `{selected_channel}` 合并后缺少 release.build_number"
                ))
            })?;
        let (build_number, build_number_source) = match build_number_value {
            BuildNumberConfig::Literal(0) => {
                return Err(CliError::new(format!(
                    "app `{app_key}` 的 release.build_number 必须大于 0"
                )));
            }
            BuildNumberConfig::Literal(value) => (
                ResolvedBuildNumber::Literal(*value),
                BuildNumberSource::Literal,
            ),
            BuildNumberConfig::Expression(value) if value == "${BUILD_DATETIME}" => (
                ResolvedBuildNumber::BuildDatetime,
                BuildNumberSource::BuildDatetime,
            ),
            BuildNumberConfig::Expression(value) => {
                return Err(CliError::new(format!(
                    "app `{app_key}` 的 release.build_number 字符串 `{value}` 不受支持；只支持正整数或完整字段值 `${{BUILD_DATETIME}}`"
                )));
            }
        };
        let minimum_supported_version_value = channel
            .and_then(|channel| channel.minimum_supported_version.as_deref())
            .or(release.minimum_supported_version.as_deref())
            .unwrap_or("0.0.0");
        let minimum_supported_version = Version::parse(minimum_supported_version_value)
            .map_err(|error| {
                CliError::new(format!(
                    "app `{app_key}` 的 release.minimum_supported_version `{}` 不是合法 SemVer: {error}",
                    minimum_supported_version_value
                ))
            })?;
        let (runtime_config, runtime_config_source) = self.resolve_runtime_config(
            app_key,
            app,
            selected_channel,
            channel.and_then(|channel| channel.runtime_config.as_deref()),
        )?;
        let runtime_config_sha256 = sha256_file(&runtime_config)?;
        let target = self.config.publish.targets[&app.publish_target].for_channel(selected_channel);
        let updater_feed = public_object_url(
            &target,
            &object_key([
                app.object_prefix.as_str(),
                app_key,
                selected_channel,
                "latest.json",
            ]),
        );
        let notes_source = channel
            .and_then(|channel| channel.notes.clone())
            .or_else(|| release.notes.clone());
        if app.updater.enabled && notes_source.is_none() {
            return Err(CliError::new(format!(
                "app `{app_key}` channel `{selected_channel}` 启用了 updater，但合并后缺少 release.notes"
            )));
        }
        if let Some(notes_source) = notes_source.as_deref() {
            validate_workspace_relative_path(notes_source, "release.notes")?;
        }
        if release.channels.is_empty()
            && app.updater.enabled
            && app.updater.feed_url != updater_feed
        {
            return Err(CliError::new(format!(
                "app `{app_key}` 的 updater.feed_url 与发布 latest.json 地址不一致；期望 `{updater_feed}`"
            )));
        }
        Ok(ResolvedReleaseConfig {
            channel: selected_channel.to_owned(),
            version,
            version_source,
            build_number,
            build_number_source,
            minimum_supported_version,
            runtime_config,
            runtime_config_source,
            runtime_config_sha256,
            updater_feed,
            notes_source,
        })
    }

    fn resolve_runtime_config(
        &self,
        app_key: &str,
        app: &AppConfig,
        channel: &str,
        explicit: Option<&str>,
    ) -> CliResult<(PathBuf, String)> {
        if let Some(value) = explicit {
            validate_workspace_relative_path(value, "runtime_config")?;
            let path = resolve_workspace_file(&self.root, value, "runtime_config")?;
            return Ok((path, value.to_owned()));
        }

        let channel_relative =
            PathBuf::from("config").join(format!("{}-{channel}.toml", app.package));
        let base_relative = PathBuf::from("config").join(format!("{}.toml", app.package));
        for relative in [&channel_relative, &base_relative] {
            let value = relative.to_string_lossy().replace('\\', "/");
            validate_workspace_relative_path(&value, "runtime_config")?;
            if self.root.join(relative).is_file() {
                let path = resolve_workspace_file(&self.root, &value, "runtime_config")?;
                return Ok((path, value));
            }
        }

        Err(CliError::new(format!(
            "app `{app_key}` channel `{channel}` 缺少运行配置；请创建 `{}` 或 `{}`",
            channel_relative.display(),
            base_relative.display()
        )))
    }

    fn release_receipt_path(&self, app_key: &str, channel: &str) -> PathBuf {
        self.root
            .join(DIST_DIRECTORY)
            .join(app_key)
            .join(channel)
            .join("release.json")
    }

    fn prepare_build_receipt(
        &self,
        app_key: &str,
        app: &AppConfig,
        configured: &ResolvedReleaseConfig,
        targets: &[String],
    ) -> CliResult<ReleaseReceipt> {
        let path = self.release_receipt_path(app_key, &configured.channel);
        let previous = path
            .is_file()
            .then(|| read_release_receipt(&path))
            .transpose()?;
        if let Some(receipt) = &previous {
            validate_receipt_structure(receipt, &path)?;
            if receipt_matches_configuration(receipt, app_key, app, configured) {
                let complete = release_targets_complete(
                    &self.root,
                    app_key,
                    app,
                    &receipt.validated_release(configured)?,
                );
                let already_contains_targets = targets
                    .iter()
                    .all(|target| receipt.targets.contains(target));
                if matches!(configured.build_number, ResolvedBuildNumber::BuildDatetime)
                    && complete
                    && already_contains_targets
                {
                    // 已完成的动态构建再次执行时创建新 build；新增架构则继续补齐同一 receipt。
                } else {
                    let original_targets = &receipt.targets;
                    let mut receipt = receipt.clone();
                    for target in targets {
                        if !receipt.targets.contains(target) {
                            receipt.targets.push(target.clone());
                        }
                    }
                    if receipt.targets != *original_targets {
                        write_release_receipt_atomic(&path, &receipt)?;
                    }
                    println!(
                        "复用 release receipt：{} / build {}",
                        receipt.version, receipt.build_number
                    );
                    return Ok(receipt);
                }
            }
        }

        let previous_build_number = previous.as_ref().map(|receipt| receipt.build_number);
        let build_number = match configured.build_number {
            ResolvedBuildNumber::Literal(value) => value,
            ResolvedBuildNumber::BuildDatetime => {
                build_datetime_number(Local::now().fixed_offset(), previous_build_number)?
            }
        };
        let receipt = ReleaseReceipt {
            schema_version: RELEASE_RECEIPT_SCHEMA_VERSION,
            app_key: app_key.to_owned(),
            package: app.package.clone(),
            channel: configured.channel.clone(),
            version: configured.version.to_string(),
            build_number,
            version_source: configured.version_source,
            build_number_source: configured.build_number_source,
            created_at: unix_now()?,
            targets: targets.to_vec(),
            runtime_config_source: configured.runtime_config_source.clone(),
            runtime_config_sha256: configured.runtime_config_sha256.clone(),
            updater_feed: configured.updater_feed.clone(),
            notes_source: configured.notes_source.clone(),
        };
        write_release_receipt_atomic(&path, &receipt)?;
        println!("RELEASE RECEIPT: {}", path.display());
        Ok(receipt)
    }

    fn release_from_receipt(
        &self,
        app_key: &str,
        app: &AppConfig,
        channel: &str,
    ) -> CliResult<ValidatedRelease> {
        let package_version = cargo_package_version(&self.root, &app.package, true)?;
        let configured = self.resolved_release(app_key, app, channel, Some(package_version))?;
        let receipt_path = self.release_receipt_path(app_key, &configured.channel);
        let receipt = read_release_receipt(&receipt_path)?;
        validate_receipt_structure(&receipt, &receipt_path)?;
        if !receipt_matches_configuration(&receipt, app_key, app, &configured) {
            return Err(CliError::new(format!(
                "`{}` 与当前 app/package/channel/version/source/build_number 配置不一致",
                receipt_path.display()
            )));
        }
        receipt.validated_release(&configured)
    }

    fn brand_assets(&self, _app_key: &str, app: &AppConfig) -> CliResult<BrandAssets> {
        let assets = BrandAssets {
            application_logo: resolve_workspace_file(
                &self.root,
                &app.branding.application_logo,
                "应用内 Logo",
            )?,
            icon_source: resolve_workspace_file(
                &self.root,
                &app.branding.icon_source,
                "图标源文件",
            )?,
            macos_icon: resolve_workspace_file(
                &self.root,
                &app.platforms.macos.icon,
                "macOS ICNS",
            )?,
            windows_icon: resolve_workspace_file(
                &self.root,
                &app.platforms.windows.icon,
                "Windows ICO",
            )?,
        };
        validate_png(&assets.application_logo, None, "应用内 Logo")?;
        validate_png(&assets.icon_source, None, "图标源文件")?;
        validate_icns(&assets.macos_icon)?;
        validate_ico(&assets.windows_icon)?;
        Ok(assets)
    }

    fn select_many(
        &self,
        requested_apps: &[String],
        all: bool,
        interactive: bool,
    ) -> CliResult<Vec<String>> {
        if !requested_apps.is_empty() {
            let mut selected = Vec::new();
            for app_key in requested_apps {
                if !self.config.apps.contains_key(app_key) {
                    return Err(CliError::new(format!("nexora.toml 不存在 app `{app_key}`")));
                }
                if !selected.contains(app_key) {
                    selected.push(app_key.clone());
                }
            }
            return Ok(selected);
        }
        if all {
            return Ok(self.config.apps.keys().cloned().collect());
        }
        if self.config.apps.len() == 1 {
            return Ok(self.config.apps.keys().cloned().collect());
        }
        if !interactive {
            return Err(CliError::new(
                "nexora.toml 配置了多个 app；非交互环境必须提供 `--app` 或 `--all`",
            ));
        }
        let entries = self.config.apps.iter().collect::<Vec<_>>();
        let labels = entries
            .iter()
            .map(|(key, app)| format!("{}（{} / {}）", app.display_name, key, app.package))
            .collect::<Vec<_>>();
        let defaults = labels.iter().map(|_| true).collect::<Vec<_>>();
        let selected = MultiSelect::new()
            .with_prompt("请选择 app")
            .items(&labels)
            .defaults(&defaults)
            .interact()
            .map_err(|error| CliError::new(format!("无法读取 app 选择: {error}")))?;
        if selected.is_empty() {
            return Err(CliError::new("没有选择 app"));
        }
        Ok(selected
            .into_iter()
            .map(|index| entries[index].0.clone())
            .collect())
    }

    fn select_release_targets(
        &self,
        requested_apps: &[String],
        all_apps: bool,
        requested_channels: &[String],
        all_channels: bool,
        interactive: bool,
    ) -> CliResult<Vec<(String, String)>> {
        let apps = self.select_many(requested_apps, all_apps, interactive)?;
        let mut selections = Vec::new();
        for app_key in apps {
            let app = &self.config.apps[&app_key];
            let available = self.release_channel_names(&app_key, app)?;
            let selected_channels = if !requested_channels.is_empty() {
                let mut selected = Vec::new();
                for channel in requested_channels {
                    if !available.contains(channel) {
                        return Err(CliError::new(format!(
                            "app `{app_key}` 不支持 channel `{channel}`；可用值：{}",
                            available.join(", ")
                        )));
                    }
                    if !selected.contains(channel) {
                        selected.push(channel.clone());
                    }
                }
                selected
            } else if all_channels {
                available
            } else if available.len() > 1 && interactive {
                let default_channel = self.default_release_channel(&app_key, app)?;
                let labels = available
                    .iter()
                    .map(|channel| format!("{channel} channel"))
                    .collect::<Vec<_>>();
                let defaults = available
                    .iter()
                    .map(|channel| channel == &default_channel)
                    .collect::<Vec<_>>();
                let selected = MultiSelect::new()
                    .with_prompt(format!("请选择 app `{app_key}` 的 channel"))
                    .items(&labels)
                    .defaults(&defaults)
                    .interact()
                    .map_err(|error| CliError::new(format!("无法读取 channel 选择: {error}")))?;
                if selected.is_empty() {
                    return Err(CliError::new(format!("app `{app_key}` 没有选择 channel")));
                }
                selected
                    .into_iter()
                    .map(|index| available[index].clone())
                    .collect()
            } else {
                vec![self.default_release_channel(&app_key, app)?]
            };
            selections.extend(
                selected_channels
                    .into_iter()
                    .map(|channel| (app_key.clone(), channel)),
            );
        }
        if selections.is_empty() {
            return Err(CliError::new("没有选择 app/channel 组合"));
        }
        Ok(selections)
    }

    fn build_plans(
        &self,
        app_key: &str,
        release: &ValidatedRelease,
        targets: &[String],
        frozen_notes: Option<&FrozenReleaseNotes>,
    ) -> CliResult<Vec<BuildPlan>> {
        let app = &self.config.apps[app_key];
        let brand_assets = self.brand_assets(app_key, app)?;
        let publish_target =
            self.config.publish.targets[&app.publish_target].for_channel(&release.channel);
        targets
            .iter()
            .map(|target| {
                let arch = target_arch_alias(target)?;
                let platform = target_platform(target)?;
                let release_dir = self
                    .root
                    .join(DIST_DIRECTORY)
                    .join(app_key)
                    .join(&release.channel)
                    .join(release.version.to_string())
                    .join(release.build_number.to_string())
                    .join(target);
                let artifact_stem = format!("{}-{arch}", app.display_name);
                Ok(BuildPlan {
                    project_root: self.root.clone(),
                    app_key: app_key.to_owned(),
                    package: app.package.clone(),
                    app_id: app.app_id.clone(),
                    updater_app_id: app
                        .updater
                        .app_id
                        .clone()
                        .unwrap_or_else(|| app.app_id.clone()),
                    display_name: app.display_name.clone(),
                    release: release.clone(),
                    target: target.clone(),
                    platform,
                    signing: app.platforms.macos.signing,
                    notarize: app.platforms.macos.notarize,
                    expected_team_id: app.platforms.macos.expected_team_id.clone(),
                    allow_insecure_http: publish_target.allow_insecure_http,
                    updater: app.updater.enabled.then(|| app.updater.clone()),
                    macos_icon: brand_assets.macos_icon.clone(),
                    windows_icon: brand_assets.windows_icon.clone(),
                    windows: (platform == BuildTargetPlatform::Windows)
                        .then(|| windows_build_options(&app.platforms.windows))
                        .transpose()?,
                    app_path: match platform {
                        BuildTargetPlatform::MacOs => self
                            .root
                            .join("target")
                            .join(target)
                            .join("release/bundle/osx")
                            .join(format!("{}.app", app.package)),
                        BuildTargetPlatform::Windows => {
                            windows_binary_path(&self.root, target, &app.package)
                        }
                    },
                    app_zip_path: match platform {
                        BuildTargetPlatform::MacOs => {
                            release_dir.join(format!("{artifact_stem}.app.zip"))
                        }
                        BuildTargetPlatform::Windows => {
                            release_dir.join(format!("{artifact_stem}.windows.zip"))
                        }
                    },
                    dmg_path: release_dir.join(format!("{artifact_stem}.dmg")),
                    setup_path: release_dir.join(format!("{artifact_stem}.exe")),
                    artifact_path: release_dir.join("artifact.json"),
                    notes_path: frozen_notes
                        .map(|notes| notes.path.clone())
                        .unwrap_or_else(|| release_notes_path(&self.root, app_key, release)),
                    notes: frozen_notes.map(|notes| notes.metadata.clone()),
                })
            })
            .collect()
    }

    fn publish_plan(
        &self,
        app_key: &str,
        channel: &str,
        status: ReleaseStatus,
        client: &reqwest::blocking::Client,
    ) -> CliResult<PublishPlan> {
        let app = &self.config.apps[app_key];
        if !app.updater.enabled {
            return Err(CliError::new(format!(
                "app `{app_key}` 未启用 updater，不能发布更新清单"
            )));
        }
        let release = self.release_from_receipt(app_key, app, channel)?;
        let updater_app_id = app.updater.app_id.as_deref().unwrap_or(&app.app_id);
        let target = self.config.publish.targets[&app.publish_target].for_channel(&release.channel);
        let trusted_keys = parse_trusted_keys(&app.updater.trusted_public_keys)?;
        let (signing_key_id, signing_key) = read_signing_key(self, app_key, app, &trusted_keys)?;
        let channel_prefix = object_key([
            app.object_prefix.as_str(),
            app_key,
            release.channel.as_str(),
        ]);
        let latest_key = object_key([channel_prefix.as_str(), "latest.json"]);
        let latest_url = public_object_url(&target, &latest_key);
        let remote = read_remote_manifest(client, &latest_url, &trusted_keys)?;
        if let Some(payload) = &remote
            && (payload.app_id != updater_app_id || payload.channel != release.channel)
        {
            return Err(CliError::new(format!(
                "远端 latest.json 的 app_id/channel 与 app `{app_key}` 配置不一致"
            )));
        }
        if matches!(status, ReleaseStatus::Available)
            && remote.as_ref().is_some_and(|payload| {
                payload.version > release.version
                    || (payload.version == release.version
                        && payload.build_number >= release.build_number)
            })
        {
            return Err(CliError::new(format!(
                "待发布 identity ({}, {}) 必须严格高于远端 latest.json",
                release.version, release.build_number
            )));
        }
        let observed_sequence = remote.as_ref().map(|manifest| manifest.manifest_sequence);
        let sequence = observed_sequence
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| CliError::new("远端 manifest sequence 已达到 u64 上限"))?;

        let mut immutable_payloads = Vec::new();
        let mut channel_artifacts = Vec::new();
        let mut immutable_verify_urls = Vec::new();
        let mut channel_verify_urls = Vec::new();
        let mut manifest_artifacts = Vec::new();
        if matches!(status, ReleaseStatus::Available) {
            let local_artifacts = load_release_artifacts(&self.root, app_key, app, &release)?;
            let release_prefix = object_key([
                channel_prefix.as_str(),
                release.version.to_string().as_str(),
                release.build_number.to_string().as_str(),
            ]);
            for kind in [
                ArtifactKind::MacosAppZip,
                ArtifactKind::MacosDmg,
                ArtifactKind::WindowsSetupExe,
                ArtifactKind::WindowsZip,
            ] {
                for artifact in local_artifacts.iter().filter(|item| item.kind == kind) {
                    let arch = target_arch_alias(&artifact.target)?;
                    let key =
                        object_key([release_prefix.as_str(), arch, artifact.file_name.as_str()]);
                    let url = public_object_url(&target, &key);
                    immutable_payloads.push(Upload {
                        key: key.clone(),
                        source: UploadSource::File(artifact.path.clone()),
                        content_type: artifact_content_type(kind),
                        cache_control: IMMUTABLE_CACHE,
                        immutable: true,
                    });
                    immutable_verify_urls.push(Verification {
                        url: url.clone(),
                        expected_sha256: artifact.sha256.clone(),
                        label: format!("{} versioned artifact", artifact.file_name),
                    });
                    let checksum_key = format!("{key}.sha256");
                    let checksum_url = public_object_url(&target, &checksum_key);
                    let checksum =
                        sha256_sidecar_contents(&artifact.file_name, &artifact.sha256).into_bytes();
                    immutable_verify_urls.push(Verification {
                        url: checksum_url,
                        expected_sha256: sha256_hex(&checksum),
                        label: format!("{} SHA-256 sidecar", artifact.target),
                    });
                    immutable_payloads.push(Upload {
                        key: checksum_key,
                        source: UploadSource::Bytes(checksum),
                        content_type: "text/plain; charset=utf-8",
                        cache_control: IMMUTABLE_CACHE,
                        immutable: true,
                    });
                    if let Some(manifest_kind) = updater_manifest_artifact_kind(kind) {
                        manifest_artifacts.push(ManifestArtifact {
                            target: artifact.target.clone(),
                            url: url.clone(),
                            sha256: artifact.sha256.clone(),
                            size: artifact.size,
                            kind: manifest_kind.to_owned(),
                        });
                    }
                }
            }
            let notes_path = self
                .root
                .join(DIST_DIRECTORY)
                .join(app_key)
                .join(&release.channel)
                .join(release.version.to_string())
                .join(release.build_number.to_string())
                .join("notes.md");
            if !notes_path.is_file() {
                return Err(CliError::new(format!(
                    "发布产物缺少已冻结 notes.md：{}；请先重新执行 nexora build",
                    notes_path.display()
                )));
            }
            let notes_bytes = fs::read(&notes_path).map_err(|error| {
                CliError::new(format!(
                    "无法读取已冻结 notes.md `{}`: {error}",
                    notes_path.display()
                ))
            })?;
            let notes_metadata = ReleaseNotesMetadata {
                file_name: RELEASE_NOTES_FILE_NAME.to_owned(),
                size: u64::try_from(notes_bytes.len())
                    .map_err(|_| CliError::new("已冻结 notes.md 大小无法在当前平台表示"))?,
                sha256: sha256_hex(&notes_bytes),
            };
            verify_release_notes_bytes(&notes_metadata, &notes_bytes)
                .map_err(|error| CliError::new(format!("已冻结 notes.md 无效: {error}")))?;
            let key = object_key([release_prefix.as_str(), RELEASE_NOTES_FILE_NAME]);
            let notes_url = public_object_url(&target, &key);
            immutable_verify_urls.push(Verification {
                url: notes_url.clone(),
                expected_sha256: notes_metadata.sha256.clone(),
                label: "versioned release notes".to_owned(),
            });
            immutable_payloads.push(Upload {
                key,
                source: UploadSource::File(notes_path),
                content_type: "text/markdown; charset=utf-8",
                cache_control: IMMUTABLE_CACHE,
                immutable: true,
            });
            channel_artifacts = channel_artifact_uploads(&local_artifacts, &channel_prefix)?;
            for upload in &channel_artifacts {
                let bytes = upload.source.bytes()?;
                channel_verify_urls.push(Verification {
                    url: public_object_url(&target, &upload.key),
                    expected_sha256: sha256_hex(&bytes),
                    label: upload.key.clone(),
                });
            }
            let payload = ManifestPayload {
                manifest_sequence: sequence,
                app_id: updater_app_id.to_owned(),
                channel: release.channel.clone(),
                version: release.version.clone(),
                build_number: release.build_number,
                minimum_supported_version: release.minimum_supported_version.clone(),
                published_at: unix_now()?,
                status,
                notes_url: Some(notes_url),
                notes_sha256: Some(notes_metadata.sha256),
                notes_size: Some(notes_metadata.size),
                artifacts: manifest_artifacts,
            };
            return finalize_publish_plan(
                app_key,
                app,
                release,
                target,
                trusted_keys,
                observed_sequence,
                sequence,
                immutable_payloads,
                channel_artifacts,
                latest_key,
                latest_url,
                immutable_verify_urls,
                channel_verify_urls,
                signing_key_id,
                &signing_key,
                payload,
            );
        }

        let payload = ManifestPayload {
            manifest_sequence: sequence,
            app_id: updater_app_id.to_owned(),
            channel: release.channel.clone(),
            version: release.version.clone(),
            build_number: release.build_number,
            minimum_supported_version: release.minimum_supported_version.clone(),
            published_at: unix_now()?,
            status,
            notes_url: None,
            notes_sha256: None,
            notes_size: None,
            artifacts: Vec::new(),
        };
        finalize_publish_plan(
            app_key,
            app,
            release,
            target,
            trusted_keys,
            observed_sequence,
            sequence,
            immutable_payloads,
            channel_artifacts,
            latest_key,
            latest_url,
            immutable_verify_urls,
            channel_verify_urls,
            signing_key_id,
            &signing_key,
            payload,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn finalize_publish_plan(
    app_key: &str,
    app: &AppConfig,
    release: ValidatedRelease,
    target: PublishTarget,
    trusted_keys: Vec<TrustedKey>,
    observed_sequence: Option<u64>,
    sequence: u64,
    immutable_payloads: Vec<Upload>,
    channel_artifacts: Vec<Upload>,
    latest_key: String,
    latest_url: String,
    immutable_verify_urls: Vec<Verification>,
    channel_verify_urls: Vec<Verification>,
    signing_key_id: String,
    signing_key: &SigningKey,
    payload: ManifestPayload,
) -> CliResult<PublishPlan> {
    let required_targets = release.targets.clone();
    let payload_bytes = serde_json::to_vec(&payload)
        .map_err(|error| CliError::new(format!("无法序列化 manifest payload: {error}")))?;
    let envelope = SignedManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        payload,
        signatures: vec![ManifestSignature {
            key_id: signing_key_id,
            algorithm: "ed25519".to_owned(),
            signature: STANDARD.encode(signing_key.sign(&payload_bytes).to_bytes()),
        }],
    };
    let mut latest_json = serde_json::to_vec_pretty(&envelope)
        .map_err(|error| CliError::new(format!("无法生成 latest.json: {error}")))?;
    latest_json.push(b'\n');
    let sequence_key = object_key([
        app.object_prefix.as_str(),
        app_key,
        release.channel.as_str(),
        "manifests",
        format!("{sequence}.json").as_str(),
    ]);
    Ok(PublishPlan {
        app_key: app_key.to_owned(),
        display_name: app.display_name.clone(),
        publish_target_name: app.publish_target.clone(),
        target,
        trusted_keys,
        observed_sequence,
        sequence,
        release,
        required_targets,
        immutable_payloads,
        sequence_manifest: Upload {
            key: sequence_key,
            source: UploadSource::Bytes(latest_json.clone()),
            content_type: "application/json; charset=utf-8",
            cache_control: IMMUTABLE_CACHE,
            immutable: true,
        },
        channel_artifacts,
        latest: Upload {
            key: latest_key,
            source: UploadSource::Bytes(latest_json.clone()),
            content_type: "application/json; charset=utf-8",
            cache_control: MUTABLE_CACHE,
            immutable: false,
        },
        latest_json,
        immutable_verify_urls,
        channel_verify_urls,
        latest_url,
    })
}

fn execute_build(plan: &BuildPlan) -> CliResult<()> {
    match plan.platform {
        BuildTargetPlatform::MacOs => execute_macos_build(plan),
        BuildTargetPlatform::Windows => execute_windows_build(plan),
    }
}

fn execute_macos_build(plan: &BuildPlan) -> CliResult<()> {
    println!("构建：{}（{}）", plan.display_name, plan.app_key);
    println!(
        "版本：{} / build {} / {}",
        plan.release.version, plan.release.build_number, plan.target
    );
    ensure_build_dependencies(plan)?;
    run_cargo_bundle(plan)?;
    ensure_app_exists(&plan.app_path, &plan.package)?;
    let executable = bundle_executable_name(&plan.app_path)?;
    write_bundle_info(
        &plan.app_path,
        &plan.app_id,
        &plan.display_name,
        &plan.release.version.to_string(),
        plan.release.build_number,
    )?;
    write_bundle_icon(&plan.app_path, &plan.macos_icon)?;
    write_bundle_runtime_config(plan)?;
    write_bundle_release_resources(plan)?;
    if plan.updater.is_some() {
        write_bundle_updater_config(plan)?;
        build_and_install_sidecar(plan, &executable)?;
    }
    sign_app(plan)?;
    create_update_zip(plan)?;
    create_dmg(plan)?;
    if plan.notarize {
        notarize_dmg(plan)?;
    }
    write_artifact_manifest(plan)?;
    println!("APP: {}", plan.app_path.display());
    println!("APP ZIP: {}", plan.app_zip_path.display());
    println!("DMG: {}", plan.dmg_path.display());
    println!("ARTIFACT: {}", plan.artifact_path.display());
    Ok(())
}

fn write_bundle_runtime_config(plan: &BuildPlan) -> CliResult<()> {
    let config_directory = plan.app_path.join("Contents/Resources").join("config");
    write_runtime_config_to_directory(plan, &config_directory)
}

fn write_runtime_config_to_directory(plan: &BuildPlan, config_directory: &Path) -> CliResult<()> {
    fs::create_dir_all(config_directory).map_err(|error| {
        CliError::new(format!(
            "无法创建 bundle 运行配置目录 `{}`: {error}",
            config_directory.display()
        ))
    })?;
    let destination = config_directory.join(format!("{}.toml", plan.package));
    fs::copy(&plan.release.runtime_config, &destination).map_err(|error| {
        CliError::new(format!(
            "无法把 runtime_config `{}` 写入 bundle `{}`: {error}",
            plan.release.runtime_config_source,
            destination.display()
        ))
    })?;
    let bundled_hash = sha256_file(&destination)?;
    if bundled_hash != plan.release.runtime_config_sha256 {
        return Err(CliError::new(format!(
            "bundle runtime_config SHA-256 与预检结果不一致：{}",
            plan.release.runtime_config_source
        )));
    }
    Ok(())
}

fn execute_windows_build(plan: &BuildPlan) -> CliResult<()> {
    println!("build: {} ({})", plan.display_name, plan.app_key);
    remove_existing_file(&plan.artifact_path)?;
    ensure_windows_build_dependencies(plan)?;
    let main_resource = compile_windows_resource(plan, WindowsResourceKind::Main)?;
    build_windows_binary(plan, &plan.package, &main_resource)?;
    let updater_path = if plan.updater.is_some() {
        let sidecar = format!("{}-updater", plan.package);
        let updater_resource = compile_windows_resource(plan, WindowsResourceKind::Updater)?;
        build_windows_binary(plan, &sidecar, &updater_resource)?;
        Some(windows_binary_path(
            &plan.project_root,
            &plan.target,
            &sidecar,
        ))
    } else {
        None
    };
    sign_windows_file(plan, &plan.app_path)?;
    if let Some(path) = updater_path.as_ref() {
        sign_windows_file(plan, path)?;
    }
    let staging = stage_windows_update_payload(plan, updater_path.as_deref())?;
    create_windows_update_zip(plan, &staging)?;
    let installer_source = write_windows_installer_source(plan, &staging)?;
    build_windows_setup(plan, &installer_source)?;
    sign_windows_file(plan, &plan.setup_path)?;
    write_artifact_manifest(plan)?;
    println!("EXE: {}", plan.app_path.display());
    println!("WINDOWS ZIP: {}", plan.app_zip_path.display());
    println!("SETUP: {}", plan.setup_path.display());
    println!("ARTIFACT: {}", plan.artifact_path.display());
    Ok(())
}

fn ensure_build_dependencies(plan: &BuildPlan) -> CliResult<()> {
    ensure_macos_dependencies(
        &plan.target,
        plan.signing != SigningMode::None,
        plan.signing == SigningMode::DeveloperId,
        plan.notarize,
        true,
    )
}

fn run_cargo_bundle(plan: &BuildPlan) -> CliResult<()> {
    run_status(
        "cargo bundle",
        Command::new("cargo")
            .current_dir(&plan.project_root)
            // cargo-bundle 0.11 在 TERM=dumb 下会把无颜色终端误判为不可用并直接失败。
            .env("TERM", "xterm-256color")
            .args(["bundle", "--release", "--package", &plan.package])
            .args(["--target", &plan.target, "--format", "osx"]),
    )
}

fn build_and_install_sidecar(plan: &BuildPlan, executable: &str) -> CliResult<()> {
    let sidecar_binary = format!("{executable}-updater");
    run_status(
        "cargo build updater sidecar",
        Command::new("cargo")
            .current_dir(&plan.project_root)
            .args(["build", "--release", "--package", &plan.package])
            .args(["--bin", &sidecar_binary, "--target", &plan.target]),
    )?;
    let source = plan
        .project_root
        .join("target")
        .join(&plan.target)
        .join("release")
        .join(&sidecar_binary);
    if !source.is_file() {
        return Err(CliError::new(format!(
            "updater sidecar binary 不存在：{}",
            source.display()
        )));
    }
    let helpers = plan.app_path.join("Contents/Helpers");
    fs::create_dir_all(&helpers)
        .map_err(|error| CliError::new(format!("无法创建 `{}`: {error}", helpers.display())))?;
    let destination = helpers.join(format!("{executable}-updater"));
    fs::copy(&source, &destination).map_err(|error| {
        CliError::new(format!(
            "无法安装 updater sidecar `{}`: {error}",
            destination.display()
        ))
    })?;
    Ok(())
}

fn bundle_executable_name(app_path: &Path) -> CliResult<String> {
    let info_plist = app_path.join("Contents/Info.plist");
    let output = Command::new("plutil")
        .args(["-extract", "CFBundleExecutable", "raw", "-o", "-"])
        .arg(&info_plist)
        .output()
        .map_err(|error| CliError::new(format!("无法读取 CFBundleExecutable: {error}")))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "无法从 `{}` 读取 CFBundleExecutable: {}",
            info_plist.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let executable = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    validate_safe_component(&executable, "CFBundleExecutable")?;
    Ok(executable)
}

/// 更新 macOS bundle 中的技术身份、展示名称和发布版本。
///
/// # Errors
///
/// `Info.plist` 不存在、`plutil` 执行失败或任一值无法写入时返回错误。
pub fn write_bundle_info(
    app_path: &Path,
    app_id: &str,
    display_name: &str,
    version: &str,
    build_number: u64,
) -> CliResult<()> {
    validate_display_name(display_name)?;
    let info_plist = app_path.join("Contents/Info.plist");
    if !info_plist.is_file() {
        return Err(CliError::new(format!(
            "Info.plist 不存在：{}",
            info_plist.display()
        )));
    }
    if !command_exists("plutil") {
        let build_number = build_number.to_string();
        return write_plist_strings(
            &info_plist,
            &[
                ("CFBundleIdentifier", app_id),
                ("CFBundleDisplayName", display_name),
                ("CFBundleName", display_name),
                ("CFBundleShortVersionString", version),
                ("CFBundleVersion", &build_number),
            ],
        );
    }
    for (key, value) in [
        ("CFBundleIdentifier", app_id.to_owned()),
        ("CFBundleDisplayName", display_name.to_owned()),
        ("CFBundleName", display_name.to_owned()),
        ("CFBundleShortVersionString", version.to_owned()),
        ("CFBundleVersion", build_number.to_string()),
    ] {
        run_status(
            "plutil bundle metadata",
            Command::new("plutil")
                .args(["-replace", key, "-string", &value])
                .arg(&info_plist),
        )?;
    }
    Ok(())
}

fn write_bundle_updater_config(plan: &BuildPlan) -> CliResult<()> {
    let resources = plan.app_path.join("Contents/Resources");
    fs::create_dir_all(&resources)
        .map_err(|error| CliError::new(format!("无法创建 `{}`: {error}", resources.display())))?;
    write_updater_config_to_path(plan, &resources.join("nexora-updater.json"))
}

fn bundled_updater_config(plan: &BuildPlan) -> CliResult<BundledUpdaterConfig> {
    let updater = plan
        .updater
        .as_ref()
        .ok_or_else(|| CliError::new("当前构建计划未启用 updater"))?;
    let windows = plan.windows.as_ref();
    let windows_signature = windows
        .filter(|options| options.signing == WindowsSigningMode::Authenticode)
        .map(|options| {
            let thumbprint = resolve_windows_signing_thumbprint(options)?;
            let publisher = options.expected_publisher.clone().ok_or_else(|| {
                CliError::new("Windows Authenticode 签名需要 expected_publisher 或 publisher")
            })?;
            Ok((thumbprint, publisher))
        })
        .transpose()?;
    let (expected_windows_signer_thumbprint, expected_windows_publisher) = windows_signature
        .map_or((None, None), |(thumbprint, publisher)| {
            (Some(thumbprint), Some(publisher))
        });
    Ok(BundledUpdaterConfig {
        schema_version: 1,
        app_id: plan.updater_app_id.clone(),
        channel: plan.release.channel.clone(),
        feed_url: plan.release.updater_feed.clone(),
        trusted_public_keys: updater.trusted_public_keys.clone(),
        current_version: plan.release.version.to_string(),
        current_build_number: plan.release.build_number,
        allow_insecure_http: plan.allow_insecure_http,
        health_timeout: updater.health_timeout.clone(),
        expected_team_id: plan.expected_team_id.clone(),
        expected_windows_signer_thumbprint,
        expected_windows_publisher,
        check_on_launch: updater.check_on_launch,
    })
}

fn write_updater_config_to_path(plan: &BuildPlan, path: &Path) -> CliResult<()> {
    let config = bundled_updater_config(plan)?;
    let contents = serde_json::to_vec_pretty(&config)
        .map_err(|error| CliError::new(format!("无法生成 updater bundle 配置: {error}")))?;
    fs::write(path, contents)
        .map_err(|error| CliError::new(format!("无法写入 updater bundle 配置: {error}")))
}

fn ensure_windows_build_dependencies(plan: &BuildPlan) -> CliResult<()> {
    if env::consts::OS != "windows" {
        return Err(CliError::new("当前宿主不能构建 Windows targets"));
    }
    ensure_windows_dependencies(
        &plan.target,
        plan.windows
            .as_ref()
            .is_some_and(|options| options.signing == WindowsSigningMode::Authenticode),
        true,
    )
}

fn windows_work_dir(plan: &BuildPlan) -> PathBuf {
    plan.project_root
        .join(".runtime")
        .join("windows-build")
        .join(&plan.app_key)
        .join(&plan.target)
}

#[derive(Clone, Copy)]
enum WindowsResourceKind {
    Main,
    Updater,
}

fn compile_windows_resource(
    plan: &BuildPlan,
    resource_kind: WindowsResourceKind,
) -> CliResult<PathBuf> {
    validate_ico(&plan.windows_icon)?;
    let work_dir = windows_work_dir(plan);
    fs::create_dir_all(&work_dir)
        .map_err(|error| CliError::new(format!("无法创建 `{}`: {error}", work_dir.display())))?;
    let role = match resource_kind {
        WindowsResourceKind::Main => "main",
        WindowsResourceKind::Updater => "updater",
    };
    let rc_path = work_dir.join(format!("nexora-{role}.rc"));
    let res_path = work_dir.join(format!("nexora-{role}.res"));
    let contents = windows_resource_script(plan, resource_kind)?;
    fs::write(&rc_path, contents).map_err(|error| {
        CliError::new(format!(
            "无法写入 Windows resource `{}`: {error}",
            rc_path.display()
        ))
    })?;
    let rc = windows_sdk_tool("rc.exe")?;
    run_status(
        "rc Windows resources",
        Command::new(rc)
            .arg("/nologo")
            .arg("/c65001")
            .arg(format!("/fo{}", res_path.display()))
            .arg(&rc_path),
    )?;
    Ok(res_path)
}

fn windows_resource_script(
    plan: &BuildPlan,
    resource_kind: WindowsResourceKind,
) -> CliResult<String> {
    let options = windows_options(plan)?;
    let version = &plan.release.version;
    if [version.major, version.minor, version.patch]
        .into_iter()
        .any(|part| part > u64::from(u16::MAX))
    {
        return Err(CliError::new(format!(
            "版本 `{version}` 超出 Windows PE VERSIONINFO 的四段 u16 范围"
        )));
    }
    let build = plan.release.build_number % (u64::from(u16::MAX) + 1);
    let dotted_version = windows_file_version(version, plan.release.build_number)?;
    let (description, internal_name, original_filename) = match resource_kind {
        WindowsResourceKind::Main => (
            plan.display_name.clone(),
            plan.package.clone(),
            format!("{}.exe", plan.package),
        ),
        WindowsResourceKind::Updater => (
            format!("{} 更新程序", plan.display_name),
            format!("{}-updater", plan.package),
            format!("{}-updater.exe", plan.package),
        ),
    };
    Ok(format!(
        r#"#pragma code_page(65001)
1 ICON "{}"
1 VERSIONINFO
FILEVERSION {},{},{},{}
PRODUCTVERSION {},{},{},{}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "080404B0"
    BEGIN
      VALUE "CompanyName", "{}\0"
      VALUE "FileDescription", "{}\0"
      VALUE "FileVersion", "{}\0"
      VALUE "InternalName", "{}\0"
      VALUE "OriginalFilename", "{}\0"
      VALUE "ProductName", "{}\0"
      VALUE "ProductVersion", "{}\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0804, 1200
  END
END
"#,
        escape_rc_string(&plan.windows_icon.to_string_lossy()),
        version.major,
        version.minor,
        version.patch,
        build,
        version.major,
        version.minor,
        version.patch,
        build,
        escape_rc_string(&options.publisher),
        escape_rc_string(&description),
        dotted_version,
        escape_rc_string(&internal_name),
        escape_rc_string(&original_filename),
        escape_rc_string(&plan.display_name),
        version,
    ))
}

fn build_windows_binary(plan: &BuildPlan, binary: &str, resource: &Path) -> CliResult<()> {
    let mut command = Command::new("cargo");
    command
        .current_dir(&plan.project_root)
        .args(["rustc", "--release", "--package", &plan.package])
        .args(["--bin", binary, "--target", &plan.target])
        .arg("--");
    for argument in windows_binary_link_args(resource) {
        command.arg("-C").arg(argument);
    }
    prepend_windows_sdk_path(&mut command)?;
    run_status("cargo build Windows binary", &mut command)
}

fn windows_binary_link_args(resource: &Path) -> [String; 3] {
    [
        format!("link-arg={}", resource.display()),
        "link-arg=/SUBSYSTEM:WINDOWS".to_owned(),
        "link-arg=/ENTRY:mainCRTStartup".to_owned(),
    ]
}

fn sign_windows_file(plan: &BuildPlan, path: &Path) -> CliResult<()> {
    let options = windows_options(plan)?;
    if options.signing != WindowsSigningMode::Authenticode {
        return Ok(());
    }
    let thumbprint = resolve_windows_signing_thumbprint(options)?;
    let timestamp_url = options
        .timestamp_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CliError::new("Windows Authenticode 签名需要 timestamp_url"))?;
    let signtool = windows_sdk_tool("signtool.exe")?;
    run_status(
        "signtool sign",
        Command::new(&signtool)
            .args(["sign", "/fd", "SHA256", "/s", "My", "/sha1"])
            .arg(&thumbprint)
            .args(["/tr", timestamp_url, "/td", "SHA256"])
            .arg(path),
    )?;
    run_status(
        "signtool verify",
        Command::new(signtool).args(["verify", "/pa"]).arg(path),
    )
}

fn resolve_windows_signing_thumbprint(options: &WindowsBuildOptions) -> CliResult<String> {
    options
        .signing_thumbprint
        .clone()
        .ok_or_else(|| {
            CliError::new(
                "Windows Authenticode 签名需要 platforms.windows.signing_thumbprint 或 WINDOWS_SIGN_CERTIFICATE_SHA1",
            )
        })
}

fn stage_windows_update_payload(
    plan: &BuildPlan,
    updater_path: Option<&Path>,
) -> CliResult<PathBuf> {
    let staging = windows_work_dir(plan).join("payload");
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| {
            CliError::new(format!(
                "无法清理 Windows payload staging `{}`: {error}",
                staging.display()
            ))
        })?;
    }
    fs::create_dir_all(&staging).map_err(|error| {
        CliError::new(format!(
            "无法创建 Windows payload staging `{}`: {error}",
            staging.display()
        ))
    })?;
    fs::copy(
        &plan.app_path,
        staging.join(safe_file_name(&plan.app_path)?),
    )
    .map_err(|error| {
        CliError::new(format!(
            "无法复制 Windows 主程序 `{}`: {error}",
            plan.app_path.display()
        ))
    })?;
    if let Some(updater_path) = updater_path {
        fs::copy(updater_path, staging.join(safe_file_name(updater_path)?)).map_err(|error| {
            CliError::new(format!(
                "无法复制 Windows updater `{}`: {error}",
                updater_path.display()
            ))
        })?;
        write_updater_config_to_path(plan, &staging.join("nexora-updater.json"))?;
    }
    write_runtime_config_to_directory(plan, &staging.join("config"))?;
    write_release_resources_to_directory(plan, &staging)?;
    Ok(staging)
}

fn create_windows_update_zip(plan: &BuildPlan, staging: &Path) -> CliResult<()> {
    create_windows_update_zip_at(staging, &plan.app_zip_path)
}

fn create_windows_update_zip_at(staging: &Path, destination: &Path) -> CliResult<()> {
    create_parent(destination)?;
    remove_existing_file(destination)?;
    let archive_file = fs::File::create(destination).map_err(|error| {
        CliError::new(format!(
            "无法创建 Windows 更新 ZIP `{}`: {error}",
            destination.display()
        ))
    })?;
    let mut archive = ZipWriter::new(archive_file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for relative in collect_relative_files(staging, staging)? {
        archive
            .start_file_from_path(&relative, options)
            .map_err(|error| {
                CliError::new(format!(
                    "无法把 Windows 更新条目 `{}` 写入 ZIP: {error}",
                    relative.display()
                ))
            })?;
        let source_path = staging.join(&relative);
        let mut source = fs::File::open(&source_path).map_err(|error| {
            CliError::new(format!(
                "无法读取 Windows 更新条目 `{}`: {error}",
                source_path.display()
            ))
        })?;
        io::copy(&mut source, &mut archive).map_err(|error| {
            CliError::new(format!(
                "无法压缩 Windows 更新条目 `{}`: {error}",
                relative.display()
            ))
        })?;
    }
    archive.finish().map_err(|error| {
        CliError::new(format!(
            "无法完成 Windows 更新 ZIP `{}`: {error}",
            destination.display()
        ))
    })?;
    Ok(())
}

struct WindowsInstallerSource {
    iss: String,
    updater_config: Option<BundledUpdaterConfig>,
    file_version: String,
}

fn write_windows_installer_source(plan: &BuildPlan, staging: &Path) -> CliResult<PathBuf> {
    let work_dir = windows_work_dir(plan);
    fs::create_dir_all(&work_dir)
        .map_err(|error| CliError::new(format!("无法创建 {}: {error}", work_dir.display())))?;
    let source_path = work_dir.join("installer.iss");
    let source = windows_installer_source(plan, staging)?;
    fs::write(&source_path, source.iss).map_err(|error| {
        CliError::new(format!(
            "无法写入 Inno Setup 源文件 {}: {error}",
            source_path.display()
        ))
    })?;
    Ok(source_path)
}

fn build_windows_setup(plan: &BuildPlan, source: &Path) -> CliResult<()> {
    create_parent(&plan.setup_path)?;
    remove_existing_file(&plan.setup_path)?;
    let compiler = compatible_inno_setup_compiler()?;
    run_inno_setup(&compiler, source, &plan.setup_path)
}

fn run_inno_setup(compiler: &Path, source: &Path, setup_path: &Path) -> CliResult<()> {
    let mut command = Command::new(compiler);
    command.arg(source);
    run_status("ISCC Inno Setup", &mut command)?;
    if setup_path.is_file() {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "ISCC 未生成预期安装程序：{}",
            setup_path.display()
        )))
    }
}

fn windows_installer_source(plan: &BuildPlan, staging: &Path) -> CliResult<WindowsInstallerSource> {
    let definitions = windows_inno_definitions(plan, staging)?;
    Ok(WindowsInstallerSource {
        iss: format!("{}\n\n{WINDOWS_INSTALLER_TEMPLATE}", definitions.join("\n")),
        updater_config: plan
            .updater
            .as_ref()
            .map(|_| bundled_updater_config(plan))
            .transpose()?,
        file_version: windows_file_version(&plan.release.version, plan.release.build_number)?,
    })
}

fn windows_inno_definitions(plan: &BuildPlan, staging: &Path) -> CliResult<Vec<String>> {
    let options = windows_options(plan)?;
    if plan.app_id.len() > 127 {
        return Err(CliError::new(
            "Windows Inno Setup AppId 不能超过 127 个 ASCII 字符",
        ));
    }
    let output_directory = plan
        .setup_path
        .parent()
        .ok_or_else(|| CliError::new("Windows 安装程序输出路径缺少父目录"))?;
    let output_base_filename = plan
        .setup_path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| CliError::new("Windows 安装程序输出文件名不是 UTF-8"))?;
    validate_file_name(&format!("{output_base_filename}.exe"))?;
    let (architecture_allowed, architecture_install_mode) = inno_architecture(&plan.target)?;
    let file_version = windows_file_version(&plan.release.version, plan.release.build_number)?;
    Ok(vec![
        inno_string_definition("AppId", &plan.app_id)?,
        inno_string_definition("AppName", &plan.display_name)?,
        inno_string_definition("AppPublisher", &options.publisher)?,
        inno_string_definition("AppVersion", &plan.release.version.to_string())?,
        inno_string_definition("FileVersion", &file_version)?,
        inno_number_definition("BuildNumber", plan.release.build_number),
        inno_path_definition("SourceDir", staging, "Windows payload staging")?,
        inno_path_definition("OutputDir", output_directory, "Windows 安装程序输出目录")?,
        inno_string_definition("OutputBaseFilename", output_base_filename)?,
        inno_string_definition("MainExeName", &safe_file_name(&plan.app_path)?)?,
        inno_path_definition("IconPath", &plan.windows_icon, "Windows 安装程序图标")?,
        inno_string_definition("ArchitectureAllowed", architecture_allowed)?,
        inno_string_definition("ArchitectureInstallMode", architecture_install_mode)?,
        inno_number_definition(
            "DesktopShortcutDefault",
            u64::from(options.desktop_shortcut_default),
        ),
        inno_number_definition(
            "StartMenuShortcutDefault",
            u64::from(options.start_menu_shortcut_default),
        ),
        inno_number_definition(
            "LaunchAfterInstallDefault",
            u64::from(options.launch_after_install_default),
        ),
        inno_number_definition(
            "MinimumWindowsBuild",
            u64::from(options.minimum_windows_build),
        ),
    ])
}

fn inno_architecture(target: &str) -> CliResult<(&'static str, &'static str)> {
    match target {
        "x86_64-pc-windows-msvc" => Ok(("x64compatible and not arm64", "x64compatible")),
        "aarch64-pc-windows-msvc" => Ok(("arm64", "arm64")),
        _ => Err(CliError::new(format!(
            "target {target} 不是 Inno Setup 支持的 Windows 架构"
        ))),
    }
}

fn inno_string_definition(name: &str, value: &str) -> CliResult<String> {
    if value.chars().any(char::is_control) {
        return Err(CliError::new(format!(
            "Inno Setup 定义 {name} 不能包含控制字符"
        )));
    }
    let escaped = value.replace('{', "{{").replace('"', "\"\"");
    Ok(format!("#define {name} \"{escaped}\""))
}

fn inno_path_definition(name: &str, path: &Path, label: &str) -> CliResult<String> {
    let value = path
        .to_str()
        .ok_or_else(|| CliError::new(format!("{label} 不是 UTF-8 路径：{}", path.display())))?;
    if value.chars().any(|character| {
        matches!(character, '"' | '<' | '>' | '|' | '?' | '*') || character.is_control()
    }) {
        return Err(CliError::new(format!(
            "{label} 包含 Windows 路径不允许的字符：{}",
            path.display()
        )));
    }
    inno_string_definition(name, value)
}

fn inno_number_definition(name: &str, value: u64) -> String {
    format!("#define {name} {value}")
}

fn collect_relative_files(root: &Path, directory: &Path) -> CliResult<Vec<PathBuf>> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| CliError::new(format!("无法读取 {}: {error}", directory.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CliError::new(format!("无法读取 Windows payload: {error}")))?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_relative_files(root, &path)?);
        } else if path.is_file() {
            files.push(
                path.strip_prefix(root)
                    .map_err(|error| {
                        CliError::new(format!("无法生成 Windows payload 相对路径: {error}"))
                    })?
                    .to_path_buf(),
            );
        }
    }
    Ok(files)
}
fn windows_file_version(version: &Version, build_number: u64) -> CliResult<String> {
    let build = build_number % (u64::from(u16::MAX) + 1);
    Ok(format!(
        "{}.{}.{}.{}",
        version.major, version.minor, version.patch, build
    ))
}

fn windows_options(plan: &BuildPlan) -> CliResult<&WindowsBuildOptions> {
    plan.windows
        .as_ref()
        .ok_or_else(|| CliError::new("当前构建计划不是 Windows target"))
}

fn windows_build_options(config: &WindowsConfig) -> CliResult<WindowsBuildOptions> {
    let publisher = config
        .publisher
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CliError::new("Windows Setup.exe 需要 platforms.windows.publisher"))?;
    let (signing_thumbprint, timestamp_url, expected_publisher) = match config.signing {
        WindowsSigningMode::None => {
            for (field, configured) in [
                ("signing_thumbprint", config.signing_thumbprint.is_some()),
                ("expected_publisher", config.expected_publisher.is_some()),
                ("timestamp_url", config.timestamp_url.is_some()),
            ] {
                if configured {
                    return Err(CliError::new(format!(
                        "Windows signing = \"none\" 不能配置 platforms.windows.{field}；请删除该字段或改用 signing = \"authenticode\""
                    )));
                }
            }
            (None, None, None)
        }
        WindowsSigningMode::Authenticode => {
            let thumbprint = config
                .signing_thumbprint
                .clone()
                .or_else(|| {
                    env::var("WINDOWS_SIGN_CERTIFICATE_SHA1")
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                })
                .ok_or_else(|| {
                    CliError::new(
                        "Windows Authenticode 签名需要 platforms.windows.signing_thumbprint 或 WINDOWS_SIGN_CERTIFICATE_SHA1",
                    )
                })?;
            let thumbprint = normalize_windows_signing_thumbprint(&thumbprint)?;
            let timestamp_url = config
                .timestamp_url
                .clone()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| CliError::new("Windows Authenticode 签名需要 timestamp_url"))?;
            let expected_publisher = match &config.expected_publisher {
                Some(value) if value.trim().is_empty() => {
                    return Err(CliError::new(
                        "Windows Authenticode 签名的 expected_publisher 不能为空",
                    ));
                }
                Some(value) => value.clone(),
                None => publisher.clone(),
            };
            (
                Some(thumbprint),
                Some(timestamp_url),
                Some(expected_publisher),
            )
        }
    };
    let minimum_windows_build = config
        .minimum_windows_build
        .unwrap_or(MINIMUM_GPUI_WINDOWS_BUILD);
    if minimum_windows_build < MINIMUM_GPUI_WINDOWS_BUILD {
        return Err(CliError::new(format!(
            "minimum_windows_build 不能低于当前锁定 GPUI 的基线 {MINIMUM_GPUI_WINDOWS_BUILD}（Windows 10 1703）"
        )));
    }
    Ok(WindowsBuildOptions {
        publisher,
        signing: config.signing,
        signing_thumbprint,
        timestamp_url,
        expected_publisher,
        desktop_shortcut_default: config.desktop_shortcut_default,
        start_menu_shortcut_default: config.start_menu_shortcut_default,
        launch_after_install_default: config.launch_after_install_default,
        minimum_windows_build,
    })
}

fn normalize_windows_signing_thumbprint(value: &str) -> CliResult<String> {
    let normalized = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    if normalized.len() == 40
        && normalized
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        Ok(normalized)
    } else {
        Err(CliError::new(
            "Windows signing_thumbprint 必须是 40 位 SHA-1 证书指纹",
        ))
    }
}

fn windows_binary_path(root: &Path, target: &str, package: &str) -> PathBuf {
    cargo_target_root(root)
        .join(target)
        .join("release")
        .join(format!("{package}.exe"))
}

fn cargo_target_root(root: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
}

fn is_windows_target(target: &str) -> bool {
    matches!(target, "x86_64-pc-windows-msvc" | "aarch64-pc-windows-msvc")
}

fn target_platform(target: &str) -> CliResult<BuildTargetPlatform> {
    if target.ends_with("-apple-darwin") {
        Ok(BuildTargetPlatform::MacOs)
    } else if is_windows_target(target) {
        Ok(BuildTargetPlatform::Windows)
    } else {
        Err(CliError::new(format!(
            "当前只支持 macOS 与 Windows target，收到 `{target}`"
        )))
    }
}

fn escape_rc_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// 把配置选择的 ICNS 安装到 macOS bundle 并更新 `CFBundleIconFile`。
///
/// # Errors
///
/// ICNS 格式无效、bundle 资源目录不可写、文件名不安全或 `plutil` 更新失败时返回错误。
pub fn write_bundle_icon(app_path: &Path, icon_path: &Path) -> CliResult<()> {
    validate_icns(icon_path)?;
    let resources = app_path.join("Contents/Resources");
    fs::create_dir_all(&resources)
        .map_err(|error| CliError::new(format!("无法创建 `{}`: {error}", resources.display())))?;
    let file_name = icon_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::new("macOS icon 文件名不是有效 UTF-8"))?;
    validate_safe_component(file_name, "macOS icon 文件名")?;
    let destination = resources.join(file_name);
    fs::copy(icon_path, &destination).map_err(|error| {
        CliError::new(format!(
            "无法把 macOS icon `{}` 安装到 bundle: {error}",
            icon_path.display()
        ))
    })?;
    let info_plist = app_path.join("Contents/Info.plist");
    if !command_exists("plutil") {
        return write_plist_strings(&info_plist, &[("CFBundleIconFile", file_name)]);
    }
    run_status(
        "plutil bundle icon",
        Command::new("plutil")
            .args(["-replace", "CFBundleIconFile", "-string", file_name])
            .arg(info_plist),
    )
}

fn write_plist_strings(info_plist: &Path, values: &[(&str, &str)]) -> CliResult<()> {
    let mut contents = fs::read_to_string(info_plist)
        .map_err(|error| CliError::new(format!("无法读取 `{}`: {error}", info_plist.display())))?;
    for (key, value) in values {
        contents = replace_plist_string(&contents, key, value);
    }
    fs::write(info_plist, contents)
        .map_err(|error| CliError::new(format!("无法写入 `{}`: {error}", info_plist.display())))
}

fn replace_plist_string(contents: &str, key: &str, value: &str) -> String {
    let marker = format!("<key>{key}</key>");
    let Some(key_start) = contents.find(&marker) else {
        return contents.to_owned();
    };
    let search_start = key_start + marker.len();
    let Some(relative_string_start) = contents[search_start..].find("<string>") else {
        return contents.to_owned();
    };
    let string_start = search_start + relative_string_start;
    let value_start = string_start + "<string>".len();
    let Some(relative_string_end) = contents[value_start..].find("</string>") else {
        return contents.to_owned();
    };
    let string_end = value_start + relative_string_end;
    let mut updated = String::with_capacity(contents.len() + value.len());
    updated.push_str(&contents[..value_start]);
    updated.push_str(&escape_xml_text(value));
    updated.push_str(&contents[string_end..]);
    updated
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn sign_app(plan: &BuildPlan) -> CliResult<()> {
    match plan.signing {
        SigningMode::None => Ok(()),
        SigningMode::AdHoc => run_status(
            "codesign ad-hoc",
            Command::new("codesign")
                .args(["--force", "--deep", "--sign", "-"])
                .arg(&plan.app_path),
        ),
        SigningMode::DeveloperId => {
            let identity = env::var("MACOS_SIGN_IDENTITY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(Ok)
                .unwrap_or_else(discover_developer_id_identity)?;
            run_status(
                "codesign Developer ID",
                Command::new("codesign")
                    .args([
                        "--force",
                        "--deep",
                        "--options",
                        "runtime",
                        "--timestamp",
                        "--sign",
                        &identity,
                    ])
                    .arg(&plan.app_path),
            )
        }
    }?;
    if plan.signing != SigningMode::None {
        run_status(
            "codesign verify",
            Command::new("codesign")
                .args(["--verify", "--deep", "--strict", "--verbose=2"])
                .arg(&plan.app_path),
        )?;
    }
    Ok(())
}

fn discover_developer_id_identity() -> CliResult<String> {
    require_command("security")?;
    let output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .map_err(|error| CliError::new(format!("无法读取签名身份: {error}")))?;
    if !output.status.success() {
        return Err(CliError::new("读取 Keychain 签名身份失败"));
    }
    let identities = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("Developer ID Application:"))
        .filter_map(extract_quoted_identity)
        .collect::<Vec<_>>();
    match identities.as_slice() {
        [identity] => Ok(identity.clone()),
        [] => Err(CliError::new("没有找到 Developer ID Application 证书")),
        _ => Err(CliError::new(
            "找到多个 Developer ID Application 证书；请设置 MACOS_SIGN_IDENTITY",
        )),
    }
}

fn extract_quoted_identity(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let end = line.rfind('"')?;
    (end > start).then(|| line[start + 1..end].to_owned())
}

fn create_update_zip(plan: &BuildPlan) -> CliResult<()> {
    create_parent(&plan.app_zip_path)?;
    remove_existing_file(&plan.app_zip_path)?;
    run_status(
        "ditto app zip",
        Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&plan.app_path)
            .arg(&plan.app_zip_path),
    )
}

fn create_dmg(plan: &BuildPlan) -> CliResult<()> {
    create_parent(&plan.dmg_path)?;
    remove_existing_file(&plan.dmg_path)?;
    let staging = plan
        .project_root
        .join("target/nexora-dmg-staging")
        .join(&plan.app_key)
        .join(format!("{}-{}", plan.target, std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| {
            CliError::new(format!(
                "无法清理 DMG staging `{}`: {error}",
                staging.display()
            ))
        })?;
    }
    fs::create_dir_all(&staging).map_err(|error| {
        CliError::new(format!(
            "无法创建 DMG staging `{}`: {error}",
            staging.display()
        ))
    })?;
    let displayed_app = staging.join(format!("{}.app", plan.display_name));
    let copy_result = run_status(
        "stage display-name app",
        Command::new("ditto")
            .arg(&plan.app_path)
            .arg(&displayed_app),
    );
    if let Err(error) = copy_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let result = run_status(
        "create-dmg",
        Command::new("create-dmg")
            .arg("--volname")
            .arg(&plan.display_name)
            .args(["--window-size", "800", "400", "--icon-size", "100"])
            .arg("--icon")
            .arg(format!("{}.app", plan.display_name))
            .args(["200", "190", "--app-drop-link", "600", "185"])
            .arg(&plan.dmg_path)
            .arg(&staging),
    );
    let cleanup = fs::remove_dir_all(&staging).map_err(|error| {
        CliError::new(format!(
            "无法清理 DMG staging `{}`: {error}",
            staging.display()
        ))
    });
    result.and(cleanup)
}

fn notarize_dmg(plan: &BuildPlan) -> CliResult<()> {
    let profile = env::var("NOTARY_PROFILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "nexora".to_owned());
    run_status(
        "notarytool submit",
        Command::new("xcrun")
            .args(["notarytool", "submit"])
            .arg(&plan.dmg_path)
            .args(["--keychain-profile", &profile, "--wait"]),
    )?;
    run_status(
        "stapler staple",
        Command::new("xcrun")
            .args(["stapler", "staple"])
            .arg(&plan.dmg_path),
    )
}

fn release_notes_path(root: &Path, app_key: &str, release: &ValidatedRelease) -> PathBuf {
    root.join(DIST_DIRECTORY)
        .join(app_key)
        .join(&release.channel)
        .join(release.version.to_string())
        .join(release.build_number.to_string())
        .join(RELEASE_NOTES_FILE_NAME)
}

fn freeze_release_notes(
    root: &Path,
    app_key: &str,
    app: &AppConfig,
    release: &ValidatedRelease,
) -> CliResult<Option<FrozenReleaseNotes>> {
    let path = release_notes_path(root, app_key, release);
    let Some(source) = release.notes_source.as_deref() else {
        if app.updater.enabled {
            return Err(CliError::new(format!(
                "app `{app_key}` channel `{}` 启用了 updater，但没有配置 release.notes",
                release.channel
            )));
        }
        return Ok(None);
    };
    let bytes = if path.is_file() {
        fs::read(&path).map_err(|error| {
            CliError::new(format!(
                "无法读取已冻结 release notes `{}`: {error}",
                path.display()
            ))
        })?
    } else {
        let source_path = resolve_workspace_file(root, source, "release.notes")?;
        let size = fs::metadata(&source_path)
            .map_err(|error| {
                CliError::new(format!(
                    "无法读取 release.notes `{}` 元数据: {error}",
                    source_path.display()
                ))
            })?
            .len();
        if size == 0 || size > MAX_RELEASE_NOTES_BYTES {
            return Err(CliError::new(format!(
                "release.notes 必须在 1..={MAX_RELEASE_NOTES_BYTES} 字节范围内：{}",
                source_path.display()
            )));
        }
        let bytes = fs::read(&source_path).map_err(|error| {
            CliError::new(format!(
                "无法读取 release.notes `{}`: {error}",
                source_path.display()
            ))
        })?;
        create_parent(&path)?;
        fs::write(&path, &bytes).map_err(|error| {
            CliError::new(format!(
                "无法冻结 release notes `{}`: {error}",
                path.display()
            ))
        })?;
        bytes
    };
    let metadata = ReleaseNotesMetadata {
        file_name: RELEASE_NOTES_FILE_NAME.to_owned(),
        size: u64::try_from(bytes.len())
            .map_err(|_| CliError::new("release.notes 大小无法在当前平台表示"))?,
        sha256: sha256_hex(&bytes),
    };
    verify_release_notes_bytes(&metadata, &bytes)
        .map_err(|error| CliError::new(format!("release.notes 无效: {error}")))?;
    Ok(Some(FrozenReleaseNotes { path, metadata }))
}

fn write_bundle_release_resources(plan: &BuildPlan) -> CliResult<()> {
    write_release_resources_to_directory(plan, &plan.app_path.join("Contents/Resources"))
}

fn write_release_resources_to_directory(plan: &BuildPlan, directory: &Path) -> CliResult<()> {
    fs::create_dir_all(directory).map_err(|error| {
        CliError::new(format!(
            "无法创建发布资源目录 `{}`: {error}",
            directory.display()
        ))
    })?;
    if let Some(notes) = &plan.notes {
        let destination = directory.join(RELEASE_NOTES_FILE_NAME);
        fs::copy(&plan.notes_path, &destination).map_err(|error| {
            CliError::new(format!(
                "无法把冻结 release notes 写入安装包 `{}`: {error}",
                destination.display()
            ))
        })?;
        if sha256_file(&destination)? != notes.sha256 {
            return Err(CliError::new(
                "安装包内 release notes SHA-256 与冻结内容不一致",
            ));
        }
    } else {
        let destination = directory.join(RELEASE_NOTES_FILE_NAME);
        if destination.exists() {
            fs::remove_file(&destination).map_err(|error| {
                CliError::new(format!(
                    "无法移除安装包内过期 release notes `{}`: {error}",
                    destination.display()
                ))
            })?;
        }
    }
    let channel = parse_update_channel(&plan.release.channel)?;
    let metadata = ApplicationReleaseMetadata {
        schema_version: 1,
        app_key: plan.app_key.clone(),
        app_id: plan.updater_app_id.clone(),
        display_name: plan.display_name.clone(),
        package: plan.package.clone(),
        version: plan.release.version.clone(),
        build_number: plan.release.build_number,
        channel,
        target: plan.target.clone(),
        notes: plan.notes.clone(),
    };
    metadata
        .validate()
        .map_err(|error| CliError::new(format!("无法生成发布元数据: {error}")))?;
    let mut contents = serde_json::to_vec_pretty(&metadata)
        .map_err(|error| CliError::new(format!("无法序列化发布元数据: {error}")))?;
    contents.push(b'\n');
    fs::write(directory.join(RELEASE_METADATA_FILE_NAME), contents)
        .map_err(|error| CliError::new(format!("无法写入发布元数据: {error}")))
}

fn parse_update_channel(channel: &str) -> CliResult<UpdateChannel> {
    match channel {
        "stable" => Ok(UpdateChannel::Stable),
        "beta" => Ok(UpdateChannel::Beta),
        "nightly" => Ok(UpdateChannel::Nightly),
        _ => Err(CliError::new(format!("发布通道 `{channel}` 不受支持"))),
    }
}

fn write_artifact_manifest(plan: &BuildPlan) -> CliResult<()> {
    let artifact_paths = match plan.platform {
        BuildTargetPlatform::MacOs => vec![
            (ArtifactKind::MacosAppZip, &plan.app_zip_path),
            (ArtifactKind::MacosDmg, &plan.dmg_path),
        ],
        BuildTargetPlatform::Windows => vec![
            (ArtifactKind::WindowsSetupExe, &plan.setup_path),
            (ArtifactKind::WindowsZip, &plan.app_zip_path),
        ],
    };
    let artifacts = artifact_paths
        .into_iter()
        .map(|(kind, path)| {
            let artifact = artifact_entry(kind, path)?;
            let checksum_path = write_sha256_sidecar_with_digest(path, &artifact.sha256)?;
            println!("SHA256: {}", checksum_path.display());
            Ok(artifact)
        })
        .collect::<CliResult<Vec<_>>>()?;
    let manifest = ArtifactManifest {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        app_id: plan.updater_app_id.clone(),
        channel: plan.release.channel.clone(),
        version: plan.release.version.to_string(),
        build_number: plan.release.build_number,
        target: plan.target.clone(),
        artifacts,
    };
    create_parent(&plan.artifact_path)?;
    let mut contents = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| CliError::new(format!("无法生成 artifact.json: {error}")))?;
    contents.push(b'\n');
    fs::write(&plan.artifact_path, contents).map_err(|error| {
        CliError::new(format!(
            "无法写入 `{}`: {error}",
            plan.artifact_path.display()
        ))
    })
}

fn artifact_entry(kind: ArtifactKind, path: &Path) -> CliResult<ArtifactEntry> {
    if !path.is_file() {
        return Err(CliError::new(format!("构建产物不存在：{}", path.display())));
    }
    let file_name = safe_file_name(path)?;
    Ok(ArtifactEntry {
        kind,
        file_name,
        sha256: sha256_file(path)?,
        size: fs::metadata(path)
            .map_err(|error| {
                CliError::new(format!("无法读取 `{}` 元数据: {error}", path.display()))
            })?
            .len(),
    })
}

fn load_release_artifacts(
    root: &Path,
    app_key: &str,
    app: &AppConfig,
    release: &ValidatedRelease,
) -> CliResult<Vec<LocalArtifact>> {
    let mut result = Vec::new();
    for target in &release.targets {
        let directory = root
            .join(DIST_DIRECTORY)
            .join(app_key)
            .join(&release.channel)
            .join(release.version.to_string())
            .join(release.build_number.to_string())
            .join(target);
        let path = directory.join("artifact.json");
        let contents = fs::read_to_string(&path)
            .map_err(|error| CliError::new(format!("无法读取 `{}`: {error}", path.display())))?;
        let manifest: ArtifactManifest = serde_json::from_str(&contents)
            .map_err(|error| CliError::new(format!("无法解析 `{}`: {error}", path.display())))?;
        validate_artifact_identity(&manifest, app, release, target, &path)?;
        let kinds = manifest
            .artifacts
            .iter()
            .map(|artifact| artifact.kind)
            .collect::<BTreeSet<_>>();
        let required_kinds = required_artifact_kinds(target)?;
        for required in &required_kinds {
            if !kinds.contains(required) {
                return Err(CliError::new(format!(
                    "`{}` 缺少 {}",
                    path.display(),
                    artifact_kind_name(*required)
                )));
            }
        }
        if manifest.artifacts.len() != required_kinds.len() {
            return Err(CliError::new(format!(
                "`{}` 必须且只能描述当前 target 需要的 {} 个产物",
                path.display(),
                required_kinds.len()
            )));
        }
        for artifact in manifest.artifacts {
            if !required_kinds.contains(&artifact.kind) {
                return Err(CliError::new(format!(
                    "`{}` 包含 target `{target}` 不支持的 {} 产物",
                    path.display(),
                    artifact_kind_name(artifact.kind)
                )));
            }
            validate_file_name(&artifact.file_name)?;
            let expected_suffix = match artifact.kind {
                ArtifactKind::MacosAppZip => ".app.zip",
                ArtifactKind::MacosDmg => ".dmg",
                ArtifactKind::WindowsSetupExe => ".exe",
                ArtifactKind::WindowsZip => ".windows.zip",
            };
            if !artifact.file_name.ends_with(expected_suffix) {
                return Err(CliError::new(format!(
                    "artifact `{}` 的 kind 与文件扩展名不一致",
                    artifact.file_name
                )));
            }
            let expected_name = format!(
                "{}-{}{}",
                app.display_name,
                target_arch_alias(target)?,
                expected_suffix
            );
            if artifact.file_name != expected_name {
                return Err(CliError::new(format!(
                    "artifact 分发文件名不匹配；期望 `{expected_name}`，实际 `{}`",
                    artifact.file_name
                )));
            }
            let local_path = directory.join(&artifact.file_name);
            if !local_path.is_file() {
                return Err(CliError::new(format!(
                    "artifact.json 指向的文件不存在：{}",
                    local_path.display()
                )));
            }
            let size = fs::metadata(&local_path)
                .map_err(|error| {
                    CliError::new(format!(
                        "无法读取 `{}` 元数据: {error}",
                        local_path.display()
                    ))
                })?
                .len();
            if size != artifact.size {
                return Err(CliError::new(format!(
                    "`{}` 文件大小与 artifact.json 不一致",
                    local_path.display()
                )));
            }
            let sha256 = sha256_file(&local_path)?;
            if !sha256.eq_ignore_ascii_case(&artifact.sha256) {
                return Err(CliError::new(format!(
                    "`{}` SHA-256 与 artifact.json 不一致",
                    local_path.display()
                )));
            }
            result.push(LocalArtifact {
                target: target.clone(),
                kind: artifact.kind,
                path: local_path,
                file_name: artifact.file_name,
                sha256,
                size,
            });
        }
    }
    Ok(result)
}

fn validate_artifact_identity(
    manifest: &ArtifactManifest,
    app: &AppConfig,
    release: &ValidatedRelease,
    target: &str,
    path: &Path,
) -> CliResult<()> {
    if manifest.schema_version != ARTIFACT_SCHEMA_VERSION
        || manifest.app_id != app.app_id
        || manifest.channel != release.channel
        || manifest.version != release.version.to_string()
        || manifest.build_number != release.build_number
        || manifest.target != target
    {
        return Err(CliError::new(format!(
            "`{}` 的 app/channel/version/build/target 与 nexora.toml 不一致",
            path.display()
        )));
    }
    Ok(())
}

fn channel_artifact_uploads(
    artifacts: &[LocalArtifact],
    channel_prefix: &str,
) -> CliResult<Vec<Upload>> {
    let mut keys = BTreeSet::new();
    let mut uploads = Vec::with_capacity(artifacts.len() * 2);
    for artifact in artifacts {
        let key = object_key([channel_prefix, artifact.file_name.as_str()]);
        if !keys.insert(key.clone()) {
            return Err(CliError::new(format!(
                "channel 根目录产物文件名冲突：{}",
                artifact.file_name
            )));
        }
        uploads.push(Upload {
            key: key.clone(),
            source: UploadSource::File(artifact.path.clone()),
            content_type: artifact_content_type(artifact.kind),
            cache_control: MUTABLE_CACHE,
            immutable: false,
        });

        let checksum_key = format!("{key}.sha256");
        keys.insert(checksum_key.clone());
        uploads.push(Upload {
            key: checksum_key,
            source: UploadSource::Bytes(
                sha256_sidecar_contents(&artifact.file_name, &artifact.sha256).into_bytes(),
            ),
            content_type: "text/plain; charset=utf-8",
            cache_control: MUTABLE_CACHE,
            immutable: false,
        });
    }
    Ok(uploads)
}

fn print_publish_summary(plan: &PublishPlan, dry_run: bool) {
    println!("应用：{}", plan.display_name);
    println!(
        "版本：{}（{}）",
        plan.release.version,
        version_source_name(plan.release.version_source)
    );
    println!(
        "Build：{}（{}）",
        plan.release.build_number,
        build_number_source_name(plan.release.build_number_source)
    );
    println!("Manifest sequence：{}（自动计算）", plan.sequence);
    println!("Channel：{}", plan.release.channel);
    println!("Targets：{}", plan.required_targets.join(", "));
    println!("发布目标：{}", plan.publish_target_name);
    println!("将更新：");
    for upload in &plan.channel_artifacts {
        println!("  {}", upload.key);
    }
    println!("  {}", plan.latest.key);
    if dry_run {
        println!("模式：dry-run");
    }
}

fn version_source_name(source: VersionSource) -> &'static str {
    match source {
        VersionSource::CargoPkgVersion => "cargo_pkg_version",
        VersionSource::Literal => "literal",
    }
}

fn build_number_source_name(source: BuildNumberSource) -> &'static str {
    match source {
        BuildNumberSource::BuildDatetime => "build_datetime",
        BuildNumberSource::Literal => "literal",
    }
}

fn publish_plan(
    plan: &PublishPlan,
    client: &reqwest::blocking::Client,
    credentials: &S3Credentials,
) -> CliResult<()> {
    for upload in &plan.immutable_payloads {
        upload_object(client, &plan.target, credentials, upload)?;
    }
    for verification in &plan.immutable_verify_urls {
        verify_public_sha256(client, verification)?;
    }

    let current = read_remote_manifest(client, &plan.latest_url, &plan.trusted_keys)?;
    let current_sequence = current.as_ref().map(|payload| payload.manifest_sequence);
    if current_sequence != plan.observed_sequence {
        return Err(CliError::new(format!(
            "远端 manifest sequence 在发布期间从 {:?} 变为 {:?}；请重新执行 publish",
            plan.observed_sequence, current_sequence
        )));
    }

    for upload in &plan.channel_artifacts {
        upload_object(client, &plan.target, credentials, upload)?;
    }
    for verification in &plan.channel_verify_urls {
        verify_public_sha256(client, verification)?;
    }
    upload_object(client, &plan.target, credentials, &plan.sequence_manifest)?;
    let sequence_url = public_object_url(&plan.target, &plan.sequence_manifest.key);
    let sequence_bytes = plan.sequence_manifest.source.bytes()?;
    verify_public_bytes(client, &sequence_url, &sequence_bytes, "sequence manifest")?;
    upload_object(client, &plan.target, credentials, &plan.latest)?;
    verify_public_bytes(client, &plan.latest_url, &plan.latest_json, "latest.json")?;
    println!(
        "publish: {} sequence {} 发布并完成匿名验证",
        plan.app_key, plan.sequence
    );
    Ok(())
}

fn read_remote_manifest(
    client: &reqwest::blocking::Client,
    url: &str,
    trusted_keys: &[TrustedKey],
) -> CliResult<Option<ManifestPayload>> {
    let response = client
        .get(url)
        .send()
        .map_err(|error| CliError::new(format!("读取远端 latest.json 失败: {error}")))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(CliError::new(format!(
            "读取远端 latest.json 返回 HTTP {}；只有 404 表示首次发布",
            response.status()
        )));
    }
    let text = response
        .text()
        .map_err(|error| CliError::new(format!("读取远端 latest.json 内容失败: {error}")))?;
    verify_signed_manifest(&text, trusted_keys).map(Some)
}

fn verify_signed_manifest(json: &str, trusted_keys: &[TrustedKey]) -> CliResult<ManifestPayload> {
    let envelope: SignedManifest = serde_json::from_str(json)
        .map_err(|error| CliError::new(format!("远端 latest.json 结构非法: {error}")))?;
    if envelope.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(CliError::new(format!(
            "远端 latest.json schema_version {} 不受支持",
            envelope.schema_version
        )));
    }
    let bytes = serde_json::to_vec(&envelope.payload)
        .map_err(|error| CliError::new(format!("无法规范化远端 manifest payload: {error}")))?;
    let verified = envelope.signatures.iter().any(|signature| {
        signature.algorithm == "ed25519"
            && trusted_keys
                .iter()
                .find(|key| key.key_id == signature.key_id)
                .is_some_and(|key| {
                    STANDARD
                        .decode(&signature.signature)
                        .ok()
                        .and_then(|value| Signature::from_slice(&value).ok())
                        .is_some_and(|signature| key.key.verify(&bytes, &signature).is_ok())
                })
    });
    if !verified {
        return Err(CliError::new(
            "远端 latest.json 无法由 trusted_public_keys 验签",
        ));
    }
    Ok(envelope.payload)
}

fn preflight_immutable_objects<'a>(
    client: &reqwest::blocking::Client,
    target: &PublishTarget,
    uploads: impl IntoIterator<Item = &'a Upload>,
) -> CliResult<()> {
    for upload in uploads {
        let url = public_object_url(target, &upload.key);
        let response = client
            .head(&url)
            .send()
            .map_err(|error| CliError::new(format!("检查 immutable 对象 `{url}` 失败: {error}")))?;
        if response.status().is_success() {
            return Err(CliError::new(format!(
                "immutable 对象已存在，拒绝覆盖：{}",
                upload.key
            )));
        }
        if response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(CliError::new(format!(
                "检查 immutable 对象 `{}` 返回 HTTP {}",
                upload.key,
                response.status()
            )));
        }
    }
    Ok(())
}

fn parse_trusted_keys(values: &[String]) -> CliResult<Vec<TrustedKey>> {
    values
        .iter()
        .map(|value| {
            let parts = value.split(':').collect::<Vec<_>>();
            let [key_id, algorithm, encoded] = parts.as_slice() else {
                return Err(CliError::new("trusted_public_keys 格式无效"));
            };
            if key_id.is_empty() || *algorithm != "ed25519" {
                return Err(CliError::new("trusted_public_keys 格式无效"));
            }
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| CliError::new("trusted_public_keys Base64 无效"))?;
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| CliError::new("Ed25519 公钥必须是 32 字节"))?;
            let key =
                VerifyingKey::from_bytes(&bytes).map_err(|_| CliError::new("Ed25519 公钥无效"))?;
            Ok(TrustedKey {
                key_id: (*key_id).to_owned(),
                key,
            })
        })
        .collect()
}

fn read_signing_key(
    project: &ProjectDocument,
    app_key: &str,
    app: &AppConfig,
    trusted_keys: &[TrustedKey],
) -> CliResult<(String, SigningKey)> {
    let release = app
        .release
        .as_ref()
        .ok_or_else(|| CliError::new(format!("app `{app_key}` 缺少 release 配置")))?;
    let value = match release
        .signing_key_file
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => {
            let path = Path::new(value);
            let resolved = if path.is_absolute() {
                path.to_path_buf()
            } else {
                project.root.join(path)
            };
            fs::read_to_string(&resolved).map_err(|error| {
                CliError::new(format!(
                    "无法读取签名私钥文件 `{}`: {error}",
                    resolved.display()
                ))
            })?
        }
        None => env::var(&app.updater.signing_key_env).map_err(|_| {
            CliError::new(format!(
                "未配置 signing_key_file，且环境变量 `{}` 未设置",
                app.updater.signing_key_env
            ))
        })?,
    };
    let parts = value.trim().split(':').collect::<Vec<_>>();
    let [key_id, algorithm, encoded] = parts.as_slice() else {
        return Err(CliError::new("签名私钥格式无效"));
    };
    if key_id.is_empty() || *algorithm != "ed25519" {
        return Err(CliError::new("签名私钥格式无效"));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| CliError::new("签名私钥 Base64 无效"))?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CliError::new("Ed25519 私钥 seed 必须是 32 字节"))?;
    let signing_key = SigningKey::from_bytes(&seed);
    let trusted = trusted_keys
        .iter()
        .find(|trusted| trusted.key_id == *key_id)
        .ok_or_else(|| CliError::new("签名私钥 key id 不在 trusted_public_keys 中"))?;
    if trusted.key.to_bytes() != signing_key.verifying_key().to_bytes() {
        return Err(CliError::new(
            "签名私钥派生公钥与 trusted_public_keys 不匹配",
        ));
    }
    Ok(((*key_id).to_owned(), signing_key))
}

pub(super) fn run_icons_generate(app_key: &str, force: bool) -> CliResult<()> {
    const PNG_SIZES: [u32; 9] = [16, 24, 32, 48, 64, 128, 256, 512, 1024];

    let project = ProjectDocument::discover()?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("nexora.toml 不存在 app `{app_key}`")))?;
    let source = resolve_workspace_file(&project.root, &app.branding.icon_source, "图标源文件")?;
    let source_image = image::open(&source).map_err(|error| {
        CliError::new(format!(
            "无法解码图标源文件 `{}`: {error}",
            source.display()
        ))
    })?;
    let (width, height) = source_image.dimensions();
    if width != height || width < 1024 {
        return Err(CliError::new(format!(
            "图标源文件必须是至少 1024×1024 的正方形 PNG，当前为 {width}×{height}"
        )));
    }
    if !source_image.color().has_alpha() {
        return Err(CliError::new("图标源文件必须包含透明通道"));
    }
    validate_png(&source, Some((width, height)), "图标源文件")?;

    let source_directory = source
        .parent()
        .ok_or_else(|| CliError::new("图标源文件缺少父目录"))?;
    let mut png_outputs = PNG_SIZES
        .into_iter()
        .map(|size| (source_directory.join(format!("logo-icon-{size}.png")), size))
        .collect::<BTreeMap<_, _>>();
    for configured in
        std::iter::once(&app.branding.application_logo).chain(app.platforms.linux.icons.iter())
    {
        let output = resolve_workspace_output(&project.root, configured)?;
        let size = png_size_from_path(&output)?;
        png_outputs.insert(output, size);
    }
    let macos_icon = resolve_workspace_output(&project.root, &app.platforms.macos.icon)?;
    let windows_icon = resolve_workspace_output(&project.root, &app.platforms.windows.icon)?;
    let outputs = png_outputs
        .keys()
        .chain([&macos_icon, &windows_icon])
        .collect::<Vec<_>>();
    if !app.branding.managed && !force && outputs.iter().any(|path| path.exists()) {
        return Err(CliError::new(
            "品牌配置标记为手工资源；如确认覆盖，请重新执行并增加 `--force`",
        ));
    }

    for (path, size) in &png_outputs {
        create_parent(path)?;
        source_image
            .resize_exact(*size, *size, FilterType::Lanczos3)
            .save_with_format(path, ImageFormat::Png)
            .map_err(|error| {
                CliError::new(format!("无法生成 PNG `{}`: {error}", path.display()))
            })?;
    }
    create_parent(&macos_icon)?;
    create_parent(&windows_icon)?;
    write_icns(&macos_icon, source_directory)?;
    write_ico(&windows_icon, source_directory)?;
    validate_icns(&macos_icon)?;
    validate_ico(&windows_icon)?;
    println!(
        "已为 app `{app_key}` 生成品牌图标：{}",
        source_directory.display()
    );
    Ok(())
}

fn png_size_from_path(path: &Path) -> CliResult<u32> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::new(format!("PNG 路径无效：{}", path.display())))?;
    let size = name
        .strip_prefix("logo-icon-")
        .and_then(|value| value.strip_suffix(".png"))
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| {
            CliError::new(format!(
                "受管 PNG 必须使用 `logo-icon-<尺寸>.png` 命名：{}",
                path.display()
            ))
        })?;
    if !matches!(size, 16 | 24 | 32 | 48 | 64 | 128 | 256 | 512 | 1024) {
        return Err(CliError::new(format!("不支持的标准图标尺寸：{size}")));
    }
    Ok(size)
}

fn write_icns(path: &Path, png_directory: &Path) -> CliResult<()> {
    let entries = [
        ("icp4", 16_u32),
        ("icp5", 32),
        ("icp6", 64),
        ("ic07", 128),
        ("ic08", 256),
        ("ic09", 512),
        ("ic10", 1024),
    ];
    let mut chunks = Vec::new();
    for (kind, size) in entries {
        let png = fs::read(png_directory.join(format!("logo-icon-{size}.png")))
            .map_err(|error| CliError::new(format!("无法读取 ICNS 输入 PNG: {error}")))?;
        let chunk_size =
            u32::try_from(png.len() + 8).map_err(|_| CliError::new("ICNS chunk 过大"))?;
        chunks.extend_from_slice(kind.as_bytes());
        chunks.extend_from_slice(&chunk_size.to_be_bytes());
        chunks.extend_from_slice(&png);
    }
    let file_size = u32::try_from(chunks.len() + 8).map_err(|_| CliError::new("ICNS 文件过大"))?;
    let mut output = Vec::with_capacity(chunks.len() + 8);
    output.extend_from_slice(b"icns");
    output.extend_from_slice(&file_size.to_be_bytes());
    output.extend_from_slice(&chunks);
    fs::write(path, output)
        .map_err(|error| CliError::new(format!("无法写入 ICNS `{}`: {error}", path.display())))
}

fn write_ico(path: &Path, png_directory: &Path) -> CliResult<()> {
    let sizes = [16_u32, 24, 32, 48, 64, 128, 256];
    let images = sizes
        .into_iter()
        .map(|size| {
            fs::read(png_directory.join(format!("logo-icon-{size}.png")))
                .map(|bytes| (size, bytes))
                .map_err(|error| CliError::new(format!("无法读取 ICO 输入 PNG: {error}")))
        })
        .collect::<CliResult<Vec<_>>>()?;
    let count = u16::try_from(images.len()).map_err(|_| CliError::new("ICO 图像数量过多"))?;
    let directory_size = 6_usize + images.len() * 16;
    let mut offset = u32::try_from(directory_size).map_err(|_| CliError::new("ICO 目录过大"))?;
    let mut output = Vec::new();
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&1_u16.to_le_bytes());
    output.extend_from_slice(&count.to_le_bytes());
    for (size, bytes) in &images {
        output.push(if *size == 256 { 0 } else { *size as u8 });
        output.push(if *size == 256 { 0 } else { *size as u8 });
        output.extend_from_slice(&[0, 0]);
        output.extend_from_slice(&1_u16.to_le_bytes());
        output.extend_from_slice(&32_u16.to_le_bytes());
        let byte_len = u32::try_from(bytes.len()).map_err(|_| CliError::new("ICO PNG 过大"))?;
        output.extend_from_slice(&byte_len.to_le_bytes());
        output.extend_from_slice(&offset.to_le_bytes());
        offset = offset
            .checked_add(byte_len)
            .ok_or_else(|| CliError::new("ICO 文件过大"))?;
    }
    for (_, bytes) in images {
        output.write_all(&bytes).map_err(|error| {
            CliError::new(format!("无法组装 ICO `{}`: {error}", path.display()))
        })?;
    }
    fs::write(path, output)
        .map_err(|error| CliError::new(format!("无法写入 ICO `{}`: {error}", path.display())))
}

fn validate_workspace_relative_path(value: &str, label: &str) -> CliResult<()> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CliError::new(format!(
            "{label} 必须是 workspace 内的相对路径：`{value}`"
        )));
    }
    Ok(())
}

fn resolve_workspace_output(root: &Path, value: &str) -> CliResult<PathBuf> {
    validate_workspace_relative_path(value, "品牌资源路径")?;
    Ok(root.join(value))
}

fn resolve_workspace_file(root: &Path, value: &str, label: &str) -> CliResult<PathBuf> {
    let path = resolve_workspace_output(root, value)?;
    if !path.is_file() {
        return Err(CliError::new(format!("{label} 不存在：{}", path.display())));
    }
    let canonical_root = root.canonicalize().map_err(|error| {
        CliError::new(format!(
            "无法解析 workspace 根目录 `{}`: {error}",
            root.display()
        ))
    })?;
    let canonical_path = path.canonicalize().map_err(|error| {
        CliError::new(format!("无法解析品牌资源 `{}`: {error}", path.display()))
    })?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(CliError::new(format!(
            "{label} 必须位于 workspace 范围内：{}",
            path.display()
        )));
    }
    Ok(path)
}

fn validate_png(
    path: &Path,
    expected_dimensions: Option<(u32, u32)>,
    label: &str,
) -> CliResult<()> {
    let reader = image::ImageReader::open(path)
        .map_err(|error| CliError::new(format!("无法读取 {label} `{}`: {error}", path.display())))?
        .with_guessed_format()
        .map_err(|error| {
            CliError::new(format!("无法识别 {label} `{}`: {error}", path.display()))
        })?;
    if reader.format() != Some(ImageFormat::Png) {
        return Err(CliError::new(format!(
            "{label} 必须是 PNG：{}",
            path.display()
        )));
    }
    let image = reader.decode().map_err(|error| {
        CliError::new(format!("无法解码 {label} `{}`: {error}", path.display()))
    })?;
    if let Some(expected) = expected_dimensions
        && image.dimensions() != expected
    {
        return Err(CliError::new(format!(
            "{label} 尺寸不正确：期望 {}×{}，实际 {}×{}",
            expected.0,
            expected.1,
            image.width(),
            image.height()
        )));
    }
    Ok(())
}

fn validate_icns(path: &Path) -> CliResult<()> {
    let bytes = fs::read(path)
        .map_err(|error| CliError::new(format!("无法读取 ICNS `{}`: {error}", path.display())))?;
    let declared = bytes
        .get(4..8)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_be_bytes);
    if !bytes.starts_with(b"icns") || declared != u32::try_from(bytes.len()).ok() {
        return Err(CliError::new(format!("ICNS 格式无效：{}", path.display())));
    }
    Ok(())
}

fn validate_ico(path: &Path) -> CliResult<()> {
    let bytes = fs::read(path)
        .map_err(|error| CliError::new(format!("无法读取 ICO `{}`: {error}", path.display())))?;
    if bytes.len() < 6 || bytes[0..4] != [0, 0, 1, 0] || bytes[4..6] == [0, 0] {
        return Err(CliError::new(format!("ICO 格式无效：{}", path.display())));
    }
    Ok(())
}

fn validate_publish_target(target: &PublishTarget) -> CliResult<()> {
    if target.provider != "s3" {
        return Err(CliError::new(format!(
            "不支持 publish provider `{}`",
            target.provider
        )));
    }
    validate_http_url(&target.endpoint, "publish endpoint")?;
    validate_http_url(&target.public_base_url, "public_base_url")?;
    let endpoint = url::Url::parse(&target.endpoint)
        .map_err(|error| CliError::new(format!("publish endpoint 无效: {error}")))?;
    let public = url::Url::parse(&target.public_base_url)
        .map_err(|error| CliError::new(format!("public_base_url 无效: {error}")))?;
    if (endpoint.scheme() == "http" || public.scheme() == "http") && !target.allow_insecure_http {
        return Err(CliError::new(
            "HTTP endpoint/public_base_url 必须显式设置 allow_insecure_http = true",
        ));
    }
    if target.region.as_deref().is_some_and(str::is_empty) {
        return Err(CliError::new("publish region 不能为空"));
    }
    validate_safe_component(&target.bucket, "bucket")
}

fn validate_http_url(value: &str, label: &str) -> CliResult<()> {
    let url =
        url::Url::parse(value).map_err(|error| CliError::new(format!("{label} 无效: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(CliError::new(format!(
            "{label} 必须是包含 host 的 http/https URL：`{value}`"
        )));
    }
    Ok(())
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    name: String,
    version: Version,
}

fn cargo_package_version(root: &Path, package: &str, locked: bool) -> CliResult<Version> {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args(["metadata", "--no-deps", "--format-version", "1"]);
    if locked {
        command.arg("--locked");
    }
    let output = command
        .output()
        .map_err(|error| CliError::new(format!("无法执行 cargo metadata: {error}")))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "cargo metadata 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)
        .map_err(|error| CliError::new(format!("无法解析 cargo metadata: {error}")))?;
    metadata
        .packages
        .into_iter()
        .find(|item| item.name == package)
        .map(|item| item.version)
        .ok_or_else(|| CliError::new(format!("Cargo workspace 中不存在 package `{package}`")))
}

fn read_release_receipt(path: &Path) -> CliResult<ReleaseReceipt> {
    let contents = fs::read_to_string(path).map_err(|error| {
        CliError::new(format!(
            "无法读取 release receipt `{}`: {error}",
            path.display()
        ))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        CliError::new(format!(
            "无法解析 release receipt `{}`: {error}",
            path.display()
        ))
    })
}

fn validate_receipt_structure(receipt: &ReleaseReceipt, path: &Path) -> CliResult<()> {
    if receipt.schema_version != RELEASE_RECEIPT_SCHEMA_VERSION {
        return Err(CliError::new(format!(
            "release receipt `{}` schema_version {} 不受支持",
            path.display(),
            receipt.schema_version
        )));
    }
    validate_safe_component(&receipt.app_key, "release receipt app_key")?;
    validate_safe_component(&receipt.package, "release receipt package")?;
    validate_safe_component(&receipt.channel, "release receipt channel")?;
    Version::parse(&receipt.version).map_err(|error| {
        CliError::new(format!(
            "release receipt `{}` version 非法: {error}",
            path.display()
        ))
    })?;
    if receipt.build_number == 0 {
        return Err(CliError::new(format!(
            "release receipt `{}` build_number 必须大于 0",
            path.display()
        )));
    }
    if receipt.targets.is_empty() {
        return Err(CliError::new(format!(
            "release receipt `{}` targets 不能为空",
            path.display()
        )));
    }
    let mut seen = BTreeSet::new();
    for target in &receipt.targets {
        validate_required_target(target)?;
        if !seen.insert(target) {
            return Err(CliError::new(format!(
                "release receipt `{}` 重复声明 target `{target}`",
                path.display()
            )));
        }
    }
    validate_workspace_relative_path(
        &receipt.runtime_config_source,
        "release receipt runtime_config_source",
    )?;
    if receipt.runtime_config_sha256.len() != 64
        || !receipt
            .runtime_config_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(CliError::new(format!(
            "release receipt `{}` runtime_config_sha256 非法",
            path.display()
        )));
    }
    validate_http_url(&receipt.updater_feed, "release receipt updater_feed")?;
    if let Some(notes_source) = receipt.notes_source.as_deref() {
        validate_workspace_relative_path(notes_source, "release receipt notes_source")?;
    }
    Ok(())
}

fn receipt_matches_configuration(
    receipt: &ReleaseReceipt,
    app_key: &str,
    app: &AppConfig,
    configured: &ResolvedReleaseConfig,
) -> bool {
    let build_number_matches = match configured.build_number {
        ResolvedBuildNumber::BuildDatetime => true,
        ResolvedBuildNumber::Literal(value) => receipt.build_number == value,
    };
    receipt.app_key == app_key
        && receipt.package == app.package
        && receipt.channel == configured.channel
        && receipt.version == configured.version.to_string()
        && receipt.version_source == configured.version_source
        && receipt.build_number_source == configured.build_number_source
        && receipt.runtime_config_source == configured.runtime_config_source
        && receipt.runtime_config_sha256 == configured.runtime_config_sha256
        && receipt.updater_feed == configured.updater_feed
        && receipt.notes_source == configured.notes_source
        && build_number_matches
}

fn release_targets_complete(
    root: &Path,
    app_key: &str,
    app: &AppConfig,
    release: &ValidatedRelease,
) -> bool {
    load_release_artifacts(root, app_key, app, release).is_ok()
}

fn write_release_receipt_atomic(path: &Path, receipt: &ReleaseReceipt) -> CliResult<()> {
    create_parent(path)?;
    let mut contents = serde_json::to_vec_pretty(receipt)
        .map_err(|error| CliError::new(format!("无法生成 release receipt: {error}")))?;
    contents.push(b'\n');
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CliError::new(format!("系统时间早于 Unix 元年: {error}")))?
        .as_nanos();
    let temporary = path.with_extension(format!("json.{}-{suffix}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                CliError::new(format!(
                    "无法创建 release receipt 临时文件 `{}`: {error}",
                    temporary.display()
                ))
            })?;
        file.write_all(&contents).map_err(|error| {
            CliError::new(format!(
                "无法写入 release receipt 临时文件 `{}`: {error}",
                temporary.display()
            ))
        })?;
        file.sync_all().map_err(|error| {
            CliError::new(format!(
                "无法同步 release receipt 临时文件 `{}`: {error}",
                temporary.display()
            ))
        })?;
        fs::rename(&temporary, path).map_err(|error| {
            CliError::new(format!(
                "无法原子写入 release receipt `{}`: {error}",
                path.display()
            ))
        })?;
        if cfg!(target_os = "windows") {
            return Ok(());
        }
        let parent = path
            .parent()
            .ok_or_else(|| CliError::new("release receipt 路径缺少父目录"))?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                CliError::new(format!(
                    "无法同步 release receipt 目录 `{}`: {error}",
                    parent.display()
                ))
            })
    })();
    if result.is_err() && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn build_datetime_number(
    now: chrono::DateTime<FixedOffset>,
    previous_build_number: Option<u64>,
) -> CliResult<u64> {
    let current = now
        .format("%y%m%d%H%M%S")
        .to_string()
        .parse::<u64>()
        .map_err(|error| CliError::new(format!("无法生成本机时间 build number: {error}")))?;
    let next = previous_build_number
        .map(|value| {
            value
                .checked_add(1)
                .ok_or_else(|| CliError::new("本地 build number 已达到 u64 上限"))
        })
        .transpose()?;
    Ok(next.map_or(current, |value| current.max(value)))
}

/// 按指定 Unix 秒和 UTC offset 生成与 build 相同的本机时间构建号。
///
/// `utc_offset_seconds` 用于在集成测试中固定构建机器的本机时区；生产构建直接读取操作系统
/// 本机时区。输出采用 24 小时制 `yyMMddHHmmss`，并继续保证相对上一个构建号严格递增。
///
/// # Errors
///
/// UTC offset 或 Unix 秒超出 Chrono 支持范围，或上一个本地构建号已达到 `u64` 上限时返回错误。
#[allow(dead_code)]
pub fn inspect_build_datetime_number(
    unix_seconds: i64,
    utc_offset_seconds: i32,
    previous_build_number: Option<u64>,
) -> CliResult<u64> {
    let offset = FixedOffset::east_opt(utc_offset_seconds)
        .ok_or_else(|| CliError::new("UTC offset 超出 Chrono 支持范围"))?;
    let now = offset
        .timestamp_opt(unix_seconds, 0)
        .single()
        .ok_or_else(|| CliError::new("Unix 秒超出本机时间范围"))?;
    build_datetime_number(now, previous_build_number)
}

/// 校验用户可见名称是否能安全进入跨平台分发文件名。
///
/// # Errors
///
/// 名称为空、包含控制字符、路径分隔符、Windows 禁止字符、保留设备名或尾随点/空格时
/// 返回错误。合法 Unicode、中文、大小写和内部空格会原样保留。
pub fn validate_display_name(value: &str) -> CliResult<()> {
    let invalid_windows_character = value.chars().any(|character| {
        matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        )
    });
    let device_name = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved_device = matches!(
        device_name.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$"
    ) || device_name
        .strip_prefix("COM")
        .or_else(|| device_name.strip_prefix("LPT"))
        .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'));
    if value.trim().is_empty()
        || matches!(value, "." | "..")
        || value.ends_with('.')
        || value.ends_with(' ')
        || invalid_windows_character
        || value.chars().any(char::is_control)
        || reserved_device
    {
        return Err(CliError::new(format!(
            "display_name `{value}` 不能安全作为跨平台分发文件名"
        )));
    }
    Ok(())
}

fn validate_safe_component(value: &str, label: &str) -> CliResult<()> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(CliError::new(format!("{label} `{value}` 不是安全路径分量")));
    }
    Ok(())
}

fn validate_channel_name(value: &str) -> CliResult<()> {
    validate_safe_component(value, "release channel")?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(CliError::new(format!(
            "release channel `{value}` 只能包含 ASCII 字母、数字、点、短横线和下划线"
        )));
    }
    Ok(())
}

fn validate_app_id(value: &str) -> CliResult<()> {
    if value.is_empty()
        || value.len() > 255
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(CliError::new(format!("app_id `{value}` 格式不安全")));
    }
    Ok(())
}

fn validate_file_name(value: &str) -> CliResult<()> {
    validate_safe_component(value, "artifact file_name")?;
    let path = Path::new(value);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err(CliError::new(format!(
            "artifact file_name `{value}` 不安全"
        )));
    }
    Ok(())
}

fn validate_required_target(target: &str) -> CliResult<()> {
    target_platform(target)?;
    target_arch_alias(target).map(|_| ())
}

fn required_artifact_kinds(target: &str) -> CliResult<Vec<ArtifactKind>> {
    match target_platform(target)? {
        BuildTargetPlatform::MacOs => Ok(vec![ArtifactKind::MacosAppZip, ArtifactKind::MacosDmg]),
        BuildTargetPlatform::Windows => Ok(vec![
            ArtifactKind::WindowsSetupExe,
            ArtifactKind::WindowsZip,
        ]),
    }
}

fn updater_manifest_artifact_kind(kind: ArtifactKind) -> Option<&'static str> {
    match kind {
        ArtifactKind::MacosAppZip => Some("macos_app_zip"),
        ArtifactKind::WindowsZip => Some("windows_update_zip"),
        ArtifactKind::MacosDmg | ArtifactKind::WindowsSetupExe => None,
    }
}

fn target_arch_alias(target: &str) -> CliResult<&'static str> {
    match target {
        "aarch64-apple-darwin" => Ok("aarch64"),
        "x86_64-apple-darwin" => Ok("x86_64"),
        "x86_64-pc-windows-msvc" => Ok("x86_64"),
        "aarch64-pc-windows-msvc" => Ok("aarch64"),
        other => Err(CliError::new(format!(
            "当前只支持 macOS 与 Windows target，收到 `{other}`"
        ))),
    }
}

fn host_can_build(target: &str) -> bool {
    (env::consts::OS == "macos" && target.ends_with("-apple-darwin"))
        || (env::consts::OS == "windows" && is_windows_target(target))
}

fn terminal_is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn ensure_macos() -> CliResult<()> {
    if env::consts::OS != "macos" {
        return Err(CliError::new("当前宿主不能构建 macOS targets"));
    }
    Ok(())
}

fn ensure_supported_build_host() -> CliResult<()> {
    if matches!(env::consts::OS, "macos" | "windows") {
        Ok(())
    } else {
        Err(CliError::new("当前宿主不能构建桌面 targets"))
    }
}

fn ensure_app_exists(path: &Path, package: &str) -> CliResult<()> {
    if path.is_dir() {
        return Ok(());
    }
    Err(CliError::new(format!(
        "cargo-bundle 原始 .app 缺失：{}；产物必须按 package `{package}` 定位",
        path.display()
    )))
}

fn artifact_kind_name(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::MacosAppZip => "macos_app_zip",
        ArtifactKind::MacosDmg => "macos_dmg",
        ArtifactKind::WindowsSetupExe => "windows_setup_exe",
        ArtifactKind::WindowsZip => "windows_update_zip",
    }
}

fn artifact_content_type(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::MacosAppZip => "application/zip",
        ArtifactKind::MacosDmg => "application/x-apple-diskimage",
        ArtifactKind::WindowsSetupExe => "application/vnd.microsoft.portable-executable",
        ArtifactKind::WindowsZip => "application/zip",
    }
}

fn safe_file_name(path: &Path) -> CliResult<String> {
    let value = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::new(format!("路径缺少 UTF-8 文件名：{}", path.display())))?
        .to_owned();
    validate_file_name(&value)?;
    Ok(value)
}

fn create_parent(path: &Path) -> CliResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::new(format!("路径缺少父目录：{}", path.display())))?;
    fs::create_dir_all(parent)
        .map_err(|error| CliError::new(format!("无法创建 `{}`: {error}", parent.display())))
}

fn remove_existing_file(path: &Path) -> CliResult<()> {
    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            CliError::new(format!("无法删除旧产物 `{}`: {error}", path.display()))
        })?;
    }
    Ok(())
}

fn sha256_file(path: &Path) -> CliResult<String> {
    let bytes = fs::read(path)
        .map_err(|error| CliError::new(format!("无法读取 `{}`: {error}", path.display())))?;
    Ok(sha256_hex(&bytes))
}

/// 为最终构建产物写入标准 `<文件名>.sha256` 旁车文件。
///
/// 文件内容使用小写 SHA-256、两个空格、原始文件名和 LF 换行，可直接供常见校验工具读取。
/// 该函数使用 Rust 内置摘要实现，不依赖宿主安装 `shasum` 或 `sha256sum`。
///
/// # Errors
///
/// 产物不存在或不可读、路径缺少安全的 UTF-8 文件名，或旁车文件无法写入时返回错误。
#[allow(dead_code)]
pub fn write_sha256_sidecar(path: &Path) -> CliResult<PathBuf> {
    let sha256 = sha256_file(path)?;
    write_sha256_sidecar_with_digest(path, &sha256)
}

fn write_sha256_sidecar_with_digest(path: &Path, sha256: &str) -> CliResult<PathBuf> {
    let file_name = safe_file_name(path)?;
    let checksum_path = path.with_file_name(format!("{file_name}.sha256"));
    fs::write(&checksum_path, sha256_sidecar_contents(&file_name, sha256)).map_err(|error| {
        CliError::new(format!(
            "无法写入 SHA-256 文件 `{}`: {error}",
            checksum_path.display()
        ))
    })?;
    Ok(checksum_path)
}

fn sha256_sidecar_contents(file_name: &str, sha256: &str) -> String {
    format!("{sha256}  {file_name}\n")
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("写入 String 不会失败");
        output
    })
}

fn object_key<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    parts
        .into_iter()
        .flat_map(|part| part.split('/'))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn public_object_url(target: &PublishTarget, key: &str) -> String {
    format!("{}/{}", target.public_base_url.trim_end_matches('/'), key)
}

fn unix_now() -> CliResult<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CliError::new(format!("系统时间早于 Unix 元年: {error}")))?
        .as_secs();
    i64::try_from(seconds).map_err(|_| CliError::new("Unix 秒超出 i64"))
}

fn default_check_interval() -> String {
    "15m".to_owned()
}

fn default_check_jitter() -> String {
    "1m".to_owned()
}

fn default_offline_grace_period() -> String {
    "24h".to_owned()
}

fn default_mandatory_restart_delay() -> String {
    "15m".to_owned()
}

fn default_health_timeout() -> String {
    "2m".to_owned()
}

fn default_launch_after_install() -> bool {
    true
}

fn rustc_host_target() -> CliResult<String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|error| CliError::new(format!("无法执行 `rustc -vV`: {error}")))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "`rustc -vV` 执行失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let host = stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or_else(|| CliError::new("`rustc -vV` 没有返回 host target"))?;
    validate_required_target(host)?;
    Ok(host.to_owned())
}

fn default_start_menu_shortcut() -> bool {
    true
}

pub(super) fn run_doctor(fix: bool) -> CliResult<()> {
    if env::consts::OS == "macos" {
        ensure_macos()?;
        ensure_rust_toolchain(fix)?;
        let target = rustc_host_target()?;
        ensure_macos_dependencies(&target, true, true, true, fix)?;
        println!("doctor: macOS 构建依赖可用");
    } else if env::consts::OS == "windows" {
        ensure_rust_toolchain(fix)?;
        let target = rustc_host_target()?;
        ensure_windows_dependencies(&target, true, fix)?;
        println!("doctor: Windows SDK 与 Inno Setup 构建依赖可用");
    } else {
        return Err(CliError::new("当前宿主暂不支持桌面发布构建"));
    }
    Ok(())
}

fn ensure_macos_dependencies(
    target: &str,
    code_signing: bool,
    developer_id: bool,
    notarize: bool,
    fix: bool,
) -> CliResult<()> {
    ensure_macos()?;
    ensure_rust_toolchain(fix)?;
    ensure_rust_target_installed(target, fix)?;
    ensure_xcode_command_line_tools(code_signing, developer_id, notarize, fix)?;
    ensure_cargo_tool("cargo-bundle", "cargo-bundle", fix)?;
    ensure_homebrew(fix)?;
    ensure_brew_formula("create-dmg", fix)
}

fn ensure_windows_dependencies(target: &str, authenticode: bool, fix: bool) -> CliResult<()> {
    ensure_rust_toolchain(fix)?;
    ensure_rust_target_installed(target, fix)?;
    ensure_windows_sdk(authenticode, fix)?;
    ensure_inno_setup(fix)
}

fn ensure_rust_toolchain(fix: bool) -> CliResult<()> {
    activate_rust_tool_path()?;
    if command_exists("cargo") && command_exists("rustup") {
        return Ok(());
    }
    let manual = if env::consts::OS == "windows" {
        "从 https://rustup.rs 下载并运行 rustup-init.exe -y"
    } else {
        RUSTUP_INSTALL_COMMAND
    };
    require_interactive_repair("Rust 工具链", manual, fix)?;
    if env::consts::OS == "windows" {
        let url = format!(
            "https://static.rust-lang.org/rustup/dist/{}/rustup-init.exe",
            windows_host_rust_target()
        );
        let installer = managed_tools_root()?.join("downloads/rustup-init.exe");
        download_official_file(&url, &installer, "Rustup 安装器")?;
        run_status("安装 Rust 工具链", Command::new(&installer).arg("-y"))?;
    } else {
        run_status(
            "安装 Rust 工具链",
            Command::new("sh").args(["-c", RUSTUP_INSTALL_COMMAND]),
        )?;
    }
    activate_rust_tool_path()?;
    if command_exists("cargo") && command_exists("rustup") {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "Rust 工具链安装完成后仍无法找到 cargo/rustup；请运行 `{manual}`，然后重新执行 `nexora build`"
        )))
    }
}

fn ensure_rust_target_installed(target: &str, fix: bool) -> CliResult<()> {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map_err(|error| CliError::new(format!("无法查询已安装 Rust target: {error}")))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "`rustup target list --installed` 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|installed| installed.trim() == target)
    {
        return Ok(());
    }
    let manual = format!("rustup target add {target}");
    require_repair("Rust target", &manual, fix)?;
    run_status(
        &format!("安装 Rust target {target}"),
        Command::new("rustup").args(["target", "add", target]),
    )?;
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map_err(|error| CliError::new(format!("无法复检 Rust target: {error}")))?;
    if output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|installed| installed.trim() == target)
    {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "执行 `{manual}` 后仍未检测到 target；请检查输出并重新执行 `nexora build`"
        )))
    }
}

fn ensure_cargo_tool(command: &str, crate_name: &str, fix: bool) -> CliResult<()> {
    if command_exists(command) {
        return Ok(());
    }
    let manual = format!("cargo install {crate_name}");
    require_repair(command, &manual, fix)?;
    run_status(
        &format!("安装 {command}"),
        Command::new("cargo").args(["install", crate_name]),
    )?;
    require_command_with_guidance(command, &manual)
}

fn ensure_homebrew(fix: bool) -> CliResult<()> {
    activate_homebrew_path()?;
    if command_exists("brew") {
        return Ok(());
    }
    require_interactive_repair("Homebrew", HOMEBREW_INSTALL_COMMAND, fix)?;
    run_status(
        "安装 Homebrew",
        Command::new("/bin/bash").args(["-c", HOMEBREW_INSTALL_COMMAND]),
    )?;
    activate_homebrew_path()?;
    require_command_with_guidance("brew", HOMEBREW_INSTALL_COMMAND)
}

fn ensure_brew_formula(formula: &str, fix: bool) -> CliResult<()> {
    if command_exists(formula) {
        return Ok(());
    }
    let manual = format!("brew install {formula}");
    require_repair(formula, &manual, fix)?;
    run_status(
        &format!("安装 {formula}"),
        Command::new("brew").args(["install", formula]),
    )?;
    require_command_with_guidance(formula, &manual)
}

fn ensure_xcode_command_line_tools(
    code_signing: bool,
    developer_id: bool,
    notarize: bool,
    fix: bool,
) -> CliResult<()> {
    let mut commands = vec!["ditto", "plutil"];
    if code_signing {
        commands.push("codesign");
    }
    if developer_id {
        commands.push("security");
    }
    if notarize {
        commands.push("xcrun");
    }
    let commands_available = commands.iter().all(|command| command_exists(command));
    let developer_directory_available = Command::new("xcode-select")
        .arg("-p")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let notarization_tools_available = !notarize
        || ["notarytool", "stapler"].iter().all(|tool| {
            Command::new("xcrun")
                .args(["--find", tool])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        });
    if commands_available && developer_directory_available && notarization_tools_available {
        return Ok(());
    }
    let manual = "xcode-select --install";
    require_interactive_repair("Xcode Command Line Tools", manual, fix)?;
    let status = Command::new("xcode-select")
        .arg("--install")
        .status()
        .map_err(|error| {
            CliError::new(format!("无法启动 Xcode Command Line Tools 安装器: {error}"))
        })?;
    Err(CliError::new(format!(
        "已启动 Xcode Command Line Tools 安装器（状态 {status}）；安装完成后重新执行 `nexora build`。如果已安装但缺少 notarytool/stapler，请安装完整 Xcode 并选择其 Developer 目录"
    )))
}

fn ensure_inno_setup(fix: bool) -> CliResult<()> {
    if compatible_inno_setup_compiler().is_ok() {
        return Ok(());
    }
    let manual = inno_setup_install_command();
    require_repair(&format!("Inno Setup {INNO_SETUP_VERSION}"), &manual, fix)?;
    if !command_exists("winget") {
        return Err(CliError::new(format!(
            "系统没有 winget；请安装后运行 {manual}，再重新执行 nexora build"
        )));
    }
    run_status(
        &format!("安装 Inno Setup {INNO_SETUP_VERSION}"),
        Command::new("winget").args([
            "install",
            "--source",
            "winget",
            "--exact",
            "--id",
            INNO_SETUP_WINGET_PACKAGE,
            "--version",
            INNO_SETUP_VERSION,
            "--scope",
            "user",
            "--silent",
            "--force",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ]),
    )?;
    compatible_inno_setup_compiler().map(|_| ()).map_err(|error| {
        CliError::new(format!(
            "执行 {manual} 后 Inno Setup 仍不可用：{error}；请检查安装输出并重新执行 nexora build"
        ))
    })
}

fn compatible_inno_setup_compiler() -> CliResult<PathBuf> {
    let candidates = inno_setup_compiler_candidates();
    if candidates.is_empty() {
        return Err(CliError::new(format!(
            "没有找到 Inno Setup {INNO_SETUP_VERSION} 的 ISCC.exe；请运行 {}",
            inno_setup_install_command()
        )));
    }
    let expected = Version::parse(INNO_SETUP_VERSION)
        .map_err(|error| CliError::new(format!("内置 Inno Setup 版本无效: {error}")))?;
    let mut detected = Vec::new();
    for path in candidates {
        match inno_setup_compiler_version(&path) {
            Ok(actual) if actual == expected => return Ok(path),
            Ok(actual) => detected.push(format!("{} ({actual})", path.display())),
            Err(error) => detected.push(format!("{} ({error})", path.display())),
        }
    }
    Err(CliError::new(format!(
        "找到 ISCC.exe，但没有精确版本 {expected}：{}；请运行 {}",
        detected.join("、"),
        inno_setup_install_command()
    )))
}

fn inno_setup_compiler_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if command_exists("ISCC.exe") {
        candidates.push(PathBuf::from("ISCC.exe"));
    }
    for (variable, suffix) in [
        ("LOCALAPPDATA", "Programs/Inno Setup 6/ISCC.exe"),
        ("ProgramFiles(x86)", "Inno Setup 6/ISCC.exe"),
        ("ProgramFiles", "Inno Setup 6/ISCC.exe"),
    ] {
        if let Some(root) = env::var_os(variable) {
            let candidate = PathBuf::from(root).join(suffix);
            if candidate.is_file() && !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn inno_setup_compiler_version(path: &Path) -> CliResult<Version> {
    let output = Command::new(path)
        .arg("-")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            CliError::new(format!(
                "无法读取 Inno Setup 编译器引擎版本 {}: {error}",
                path.display()
            ))
        })?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_inno_setup_engine_version(&text).ok_or_else(|| {
        CliError::new(format!(
            "无法从 ISCC.exe 编译器引擎输出解析 Inno Setup 版本：{}",
            path.display()
        ))
    })
}

fn parse_inno_setup_engine_version(output: &str) -> Option<Version> {
    const PREFIX: &str = "Compiler engine version: Inno Setup ";

    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix(PREFIX)
            .and_then(|version| Version::parse(version.trim()).ok())
    })
}

fn inno_setup_install_command() -> String {
    format!(
        "winget install --source winget --exact --id {INNO_SETUP_WINGET_PACKAGE} --version {INNO_SETUP_VERSION} --scope user --silent --force --accept-package-agreements --accept-source-agreements"
    )
}
fn ensure_windows_sdk(authenticode: bool, fix: bool) -> CliResult<()> {
    let mut tools = vec!["rc.exe", "fxc.exe"];
    if authenticode {
        tools.push("signtool.exe");
    }
    if tools.iter().all(|tool| windows_sdk_tool(tool).is_ok()) {
        return Ok(());
    }
    let manual = format!(
        "winget install --source winget --exact --id {WINDOWS_SDK_WINGET_PACKAGE} --accept-package-agreements --accept-source-agreements"
    );
    require_interactive_repair("Windows SDK", &manual, fix)?;
    if command_exists("winget") {
        run_status(
            "安装 Windows SDK",
            Command::new("winget").args([
                "install",
                "--source",
                "winget",
                "--exact",
                "--id",
                WINDOWS_SDK_WINGET_PACKAGE,
                "--accept-package-agreements",
                "--accept-source-agreements",
            ]),
        )?;
        if tools.iter().all(|tool| windows_sdk_tool(tool).is_ok()) {
            return Ok(());
        }
        return Err(CliError::new(
            "Windows SDK 安装已完成，但所需工具尚不可见；请完成安装器或系统重启，然后重新执行 `nexora build`",
        ));
    }
    let _ = Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", WINDOWS_SDK_DOWNLOAD_URL])
        .status();
    Err(CliError::new(format!(
        "系统没有 winget，已尝试打开 Windows SDK 官方下载页；请安装后重新执行 `nexora build`。命令：`{manual}`；官方下载：{WINDOWS_SDK_DOWNLOAD_URL}"
    )))
}

fn require_repair(label: &str, manual: &str, fix: bool) -> CliResult<()> {
    require_repair_for_terminal(label, manual, fix, terminal_is_interactive())
}

fn require_repair_for_terminal(
    label: &str,
    manual: &str,
    fix: bool,
    interactive: bool,
) -> CliResult<()> {
    if !fix {
        return Err(CliError::new(format!(
            "缺少或不兼容的 {label}；请运行 `{manual}`，或执行 `nexora doctor --fix`"
        )));
    }
    if !interactive {
        return Err(CliError::new(format!(
            "非交互环境缺少或不兼容的 {label}；不会自动安装，请运行 `{manual}` 后重新执行 `nexora build`"
        )));
    }
    Ok(())
}

fn require_interactive_repair(label: &str, manual: &str, fix: bool) -> CliResult<()> {
    require_repair(label, manual, fix)
}

fn require_command_with_guidance(command: &str, manual: &str) -> CliResult<()> {
    if command_exists(command) {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "执行 `{manual}` 后仍缺少命令 `{command}`；请检查安装输出和当前 PATH，然后重新执行 `nexora build`"
        )))
    }
}

fn download_official_file(url: &str, destination: &Path, label: &str) -> CliResult<()> {
    create_parent(destination)?;
    let response = reqwest::blocking::get(url)
        .map_err(|error| CliError::new(format!("下载{label} `{url}` 失败: {error}")))?;
    if !response.status().is_success() {
        return Err(CliError::new(format!(
            "下载{label} `{url}` 返回 HTTP {}",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .map_err(|error| CliError::new(format!("读取{label}响应失败: {error}")))?;
    fs::write(destination, bytes).map_err(|error| {
        CliError::new(format!(
            "无法写入{label} `{}`: {error}",
            destination.display()
        ))
    })
}

fn windows_host_rust_target() -> &'static str {
    if env::consts::ARCH == "aarch64" {
        "aarch64-pc-windows-msvc"
    } else {
        "x86_64-pc-windows-msvc"
    }
}

fn managed_tools_root() -> CliResult<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Nexora/tools"))
        .ok_or_else(|| CliError::new("Windows 缺少 LOCALAPPDATA，无法定位 Nexora 用户级工具目录"))
}

fn activate_rust_tool_path() -> CliResult<()> {
    let cargo_home = env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| {
        let variable = if env::consts::OS == "windows" {
            "USERPROFILE"
        } else {
            "HOME"
        };
        env::var_os(variable)
            .map(PathBuf::from)
            .map(|path| path.join(".cargo"))
    });
    if let Some(path) = cargo_home.map(|path| path.join("bin")) {
        prepend_process_paths([path])?;
    }
    Ok(())
}

fn activate_homebrew_path() -> CliResult<()> {
    let paths = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]
    .into_iter()
    .filter(|path| path.is_dir())
    .collect::<Vec<_>>();
    prepend_process_paths(paths)
}

fn prepend_process_paths(paths: impl IntoIterator<Item = PathBuf>) -> CliResult<()> {
    let mut combined = paths
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();
    if let Some(existing) = env::var_os("PATH") {
        combined.extend(env::split_paths(&existing));
    }
    let mut unique = Vec::new();
    for path in combined {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    let joined = env::join_paths(unique)
        .map_err(|error| CliError::new(format!("无法更新当前构建进程 PATH: {error}")))?;
    unsafe { env::set_var("PATH", joined) };
    Ok(())
}

fn windows_sdk_tool(name: &str) -> CliResult<PathBuf> {
    if command_exists(name) {
        return Ok(PathBuf::from(name));
    }
    let mut bin_roots = Vec::new();
    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Some(root) = env::var_os(variable) else {
            continue;
        };
        let bin = PathBuf::from(root)
            .join("Windows Kits")
            .join("10")
            .join("bin");
        if !bin_roots.contains(&bin) && bin.is_dir() {
            bin_roots.push(bin);
        }
    }
    let preferred_arch = if env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    let mut candidates = Vec::new();
    for bin_root in bin_roots {
        let mut versions = fs::read_dir(&bin_root)
            .map_err(|error| {
                CliError::new(format!(
                    "无法读取 Windows SDK 目录 `{}`: {error}",
                    bin_root.display()
                ))
            })?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .collect::<Vec<_>>();
        versions.sort_by_key(|entry| std::cmp::Reverse(entry.file_name()));
        for version in versions {
            for arch in [preferred_arch, "x64"] {
                let candidate = version.path().join(arch).join(name);
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            CliError::new(format!(
                "缺少 Windows SDK 工具 `{name}`；请安装 Windows 10/11 SDK（Desktop C++ 工具）"
            ))
        })
}

fn prepend_windows_sdk_path(command: &mut Command) -> CliResult<()> {
    let mut paths = Vec::new();
    for tool in ["rc.exe", "fxc.exe"] {
        let path = windows_sdk_tool(tool)?;
        if let Some(parent) = path.parent()
            && !paths.iter().any(|existing| existing == parent)
        {
            paths.push(parent.to_path_buf());
        }
    }
    if let Some(existing) = env::var_os("PATH") {
        paths.extend(env::split_paths(&existing));
    }
    let joined = env::join_paths(paths)
        .map_err(|error| CliError::new(format!("无法构造 Windows SDK PATH: {error}")))?;
    command.env("PATH", joined);
    Ok(())
}

pub(super) fn run_updater_keygen(
    app: &str,
    key_id: Option<String>,
    private_key_file: Option<PathBuf>,
) -> CliResult<()> {
    validate_safe_component(app, "app key")?;
    let key_id = key_id.unwrap_or_else(|| format!("{app}-main"));
    validate_safe_component(&key_id, "key id")?;
    let mut seed = [0_u8; 32];
    getrandom::fill(&mut seed)
        .map_err(|error| CliError::new(format!("无法生成 Ed25519 私钥: {error}")))?;
    let signing = SigningKey::from_bytes(&seed);
    let private_value = format!("{key_id}:ed25519:{}", STANDARD.encode(seed));
    let public_value = format!(
        "{key_id}:ed25519:{}",
        STANDARD.encode(signing.verifying_key().to_bytes())
    );
    if let Some(path) = private_key_file {
        create_parent(&path)?;
        fs::write(&path, format!("{private_value}\n")).map_err(|error| {
            CliError::new(format!("无法写入私钥文件 `{}`: {error}", path.display()))
        })?;
        println!("私钥已写入：{}", path.display());
    } else {
        println!("私钥（仅本次显示，请安全保存）：{private_value}");
    }
    println!("trusted_public_keys = [\"{public_value}\"]");
    Ok(())
}

fn require_command(command: &str) -> CliResult<()> {
    if command_exists(command) {
        Ok(())
    } else {
        Err(CliError::new(format!("缺少命令 `{command}`")))
    }
}

fn command_exists(command: &str) -> bool {
    if env::consts::OS == "windows" {
        Command::new("where.exe")
            .arg(command)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    } else {
        Command::new("sh")
            .args(["-c", "command -v -- \"$1\" >/dev/null 2>&1", "sh", command])
            .status()
            .is_ok_and(|status| status.success())
    }
}

fn run_status(name: &str, command: &mut Command) -> CliResult<()> {
    let status = command
        .status()
        .map_err(|error| CliError::new(format!("无法执行 {name}: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::new(format!("{name} 失败，退出状态 {status}")))
    }
}

struct S3Credentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    access_key_source: String,
    secret_key_source: String,
    session_token_source: Option<String>,
}

impl S3Credentials {
    fn from_env(channel: &str) -> CliResult<Self> {
        let (access_key_id, access_key_source) =
            required_credential_env_value(channel, "ACCESS_KEY_ID")?;
        let (secret_access_key, secret_key_source) =
            required_credential_env_value(channel, "SECRET_ACCESS_KEY")?;
        let session_token = credential_env_value(channel, "SESSION_TOKEN")?;
        Ok(Self {
            access_key_id,
            secret_access_key,
            session_token: session_token.as_ref().map(|(value, _)| value.clone()),
            access_key_source,
            secret_key_source,
            session_token_source: session_token.map(|(_, source)| source),
        })
    }
}

fn required_credential_env_value(channel: &str, field: &str) -> CliResult<(String, String)> {
    credential_env_value(channel, field)?.ok_or_else(|| {
        let names = credential_env_names(channel, field);
        CliError::new(format!(
            "缺少发布凭据字段 `{field}`；依次检查了 `{}`",
            names.join("`、`")
        ))
    })
}

fn credential_env_value(channel: &str, field: &str) -> CliResult<Option<(String, String)>> {
    for name in credential_env_names(channel, field) {
        if let Some(value) = optional_env_value(&name)? {
            return Ok(Some((value, name)));
        }
    }
    Ok(None)
}

fn credential_env_names(channel: &str, field: &str) -> [String; 3] {
    [
        format!("NEXORA_PUBLISH_{}_{field}", channel.to_ascii_uppercase()),
        format!("NEXORA_PUBLISH_{field}"),
        format!("AWS_{field}"),
    ]
}

fn optional_env_value(name: &str) -> CliResult<Option<String>> {
    match env::var(name) {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(CliError::new(format!(
            "发布凭据环境变量 `{name}` 不是有效 Unicode"
        ))),
    }
}

fn upload_object(
    client: &reqwest::blocking::Client,
    target: &PublishTarget,
    credentials: &S3Credentials,
    upload: &Upload,
) -> CliResult<()> {
    let body = upload.source.bytes()?;
    let url = s3_object_url(target, &upload.key)?;
    let signed = signed_s3_put(target, credentials, &url, &body)?;
    let mut request = client
        .put(url)
        .header("authorization", signed.authorization)
        .header("host", signed.host)
        .header("x-amz-content-sha256", signed.payload_sha256)
        .header("x-amz-date", signed.amz_date)
        .header("content-type", upload.content_type)
        .header("cache-control", upload.cache_control)
        .body(body);
    if upload.immutable {
        request = request.header("if-none-match", "*");
    }
    if let Some(token) = signed.session_token {
        request = request.header("x-amz-security-token", token);
    }
    let response = request
        .send()
        .map_err(|error| CliError::new(format!("上传 `{}` 失败: {error}", upload.key)))?;
    if !response.status().is_success() {
        return Err(CliError::new(format!(
            "上传 `{}` 失败，HTTP {}",
            upload.key,
            response.status()
        )));
    }
    println!("OBJECT: {}", upload.key);
    Ok(())
}

#[derive(Debug)]
struct SignedPut {
    authorization: String,
    host: String,
    payload_sha256: String,
    amz_date: String,
    session_token: Option<String>,
}

fn signed_s3_put(
    target: &PublishTarget,
    credentials: &S3Credentials,
    url: &url::Url,
    body: &[u8],
) -> CliResult<SignedPut> {
    let region = target.region.as_deref().unwrap_or("us-east-1");
    let now = Utc::now();
    let date = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let host = host_header(url)?;
    let payload_sha256 = sha256_hex(body);
    let mut canonical_headers =
        format!("host:{host}\nx-amz-content-sha256:{payload_sha256}\nx-amz-date:{amz_date}\n");
    let mut signed_headers = "host;x-amz-content-sha256;x-amz-date".to_owned();
    if let Some(token) = &credentials.session_token {
        canonical_headers.push_str(&format!("x-amz-security-token:{token}\n"));
        signed_headers.push_str(";x-amz-security-token");
    }
    let canonical_request = format!(
        "PUT\n{}\n\n{canonical_headers}\n{signed_headers}\n{payload_sha256}",
        url.path()
    );
    let scope = format!("{date}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let signing_key = s3_signing_key(&credentials.secret_access_key, &date, region)?;
    let signature = hex_lower(&hmac_sha256(&signing_key, string_to_sign.as_bytes())?);
    Ok(SignedPut {
        authorization: format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            credentials.access_key_id
        ),
        host,
        payload_sha256,
        amz_date,
        session_token: credentials.session_token.clone(),
    })
}

fn s3_signing_key(secret: &str, date: &str, region: &str) -> CliResult<Vec<u8>> {
    let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes())?;
    let region_key = hmac_sha256(&date_key, region.as_bytes())?;
    let service_key = hmac_sha256(&region_key, b"s3")?;
    hmac_sha256(&service_key, b"aws4_request")
}

fn hmac_sha256(key: &[u8], value: &[u8]) -> CliResult<Vec<u8>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|error| CliError::new(format!("无法创建 HMAC-SHA256: {error}")))?;
    mac.update(value);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn s3_object_url(target: &PublishTarget, key: &str) -> CliResult<url::Url> {
    let mut url = url::Url::parse(&target.endpoint)
        .map_err(|error| CliError::new(format!("publish endpoint 无效: {error}")))?;
    if target.force_path_style {
        let path = object_key([url.path().trim_matches('/'), target.bucket.as_str(), key]);
        url.set_path(&path);
        return Ok(url);
    }
    let host = url
        .host_str()
        .ok_or_else(|| CliError::new("publish endpoint 缺少 host"))?;
    url.set_host(Some(&format!("{}.{}", target.bucket, host)))
        .map_err(|_| CliError::new("无法构造 virtual-hosted S3 URL"))?;
    url.set_path(key);
    Ok(url)
}

fn host_header(url: &url::Url) -> CliResult<String> {
    let host = url
        .host_str()
        .ok_or_else(|| CliError::new("S3 URL 缺少 host"))?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

fn verify_public_bytes(
    client: &reqwest::blocking::Client,
    url: &str,
    expected: &[u8],
    label: &str,
) -> CliResult<()> {
    let response = client
        .get(url)
        .send()
        .map_err(|error| CliError::new(format!("匿名验证 {label} 失败: {error}")))?;
    if !response.status().is_success() {
        return Err(CliError::new(format!(
            "匿名验证 {label} 返回 HTTP {}",
            response.status()
        )));
    }
    let actual = response
        .bytes()
        .map_err(|error| CliError::new(format!("读取匿名 {label} 响应失败: {error}")))?;
    if actual.as_ref() != expected {
        return Err(CliError::new(format!("匿名验证 {label} 内容不一致")));
    }
    Ok(())
}

fn verify_public_sha256(
    client: &reqwest::blocking::Client,
    verification: &Verification,
) -> CliResult<()> {
    let response = client
        .get(&verification.url)
        .send()
        .map_err(|error| CliError::new(format!("匿名验证 {} 失败: {error}", verification.label)))?;
    if !response.status().is_success() {
        return Err(CliError::new(format!(
            "匿名验证 {} 返回 HTTP {}",
            verification.label,
            response.status()
        )));
    }
    let bytes = response.bytes().map_err(|error| {
        CliError::new(format!("读取匿名 {} 响应失败: {error}", verification.label))
    })?;
    if sha256_hex(&bytes) != verification.expected_sha256 {
        return Err(CliError::new(format!(
            "匿名验证 {} SHA-256 不一致",
            verification.label
        )));
    }
    Ok(())
}

/// 为集成测试返回配置驱动的构建计划快照，不执行任何构建命令。
///
/// # Errors
///
/// 配置无法读取、校验失败、app 不存在或当前宿主没有可构建 target 时返回错误。
#[allow(dead_code)]
pub fn inspect_build_plans(
    config_path: impl AsRef<Path>,
    app_key: &str,
) -> CliResult<Vec<serde_json::Value>> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let channel = project.default_release_channel(app_key, app)?;
    inspect_build_plans_for_channel(config_path, app_key, &channel)
}

/// 为集成测试返回指定 channel 的构建计划快照，不执行任何构建命令。
///
/// # Errors
///
/// 配置无法读取、校验失败、app/channel 不存在或当前宿主没有可构建 target 时返回错误。
#[allow(dead_code)]
pub fn inspect_build_plans_for_channel(
    config_path: impl AsRef<Path>,
    app_key: &str,
    channel: &str,
) -> CliResult<Vec<serde_json::Value>> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let configured = project.resolved_release(app_key, app, channel, None)?;
    let build_number = match configured.build_number {
        ResolvedBuildNumber::Literal(value) => value,
        ResolvedBuildNumber::BuildDatetime => {
            build_datetime_number(Local::now().fixed_offset(), None)?
        }
    };
    let targets = if app.targets.required.is_empty() {
        vec![rustc_host_target()?]
    } else {
        app.targets.required.clone()
    };
    let release = configured.validated_release(build_number, targets.clone());
    project
        .build_plans(app_key, &release, &targets, None)?
        .into_iter()
        .map(|plan| {
            Ok(serde_json::json!({
                "app_key": plan.app_key,
                "package": plan.package,
                "display_name": plan.display_name,
                "app_path": inspect_path(&plan.app_path),
                "app_zip_path": inspect_path(&plan.app_zip_path),
                "dmg_path": inspect_path(&plan.dmg_path),
                "setup_path": inspect_path(&plan.setup_path),
                "artifact_path": inspect_path(&plan.artifact_path),
                "target": plan.target,
                "platform": format!("{:?}", plan.platform),
                "version": plan.release.version,
                "build_number": plan.release.build_number,
                "version_source": version_source_name(plan.release.version_source),
                "build_number_source": build_number_source_name(plan.release.build_number_source),
                "channel": plan.release.channel,
                "runtime_config_source": plan.release.runtime_config_source,
                "runtime_config_sha256": plan.release.runtime_config_sha256,
                "updater_feed": plan.release.updater_feed,
                "notes_source": plan.release.notes_source,
                "signing": format!("{:?}", plan.signing),
                "notarize": plan.notarize,
            }))
        })
        .collect()
}

fn inspect_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 为集成测试按真实 build 规则创建或复用 release receipt，并返回其 JSON 快照。
///
/// 该入口会原子写入 `dist/<app>/<channel>/release.json`，但不会执行任何 target 构建命令。
///
/// # Errors
///
/// 配置、Cargo package、现有收据或原子写入不合法时返回错误。
#[allow(dead_code)]
pub fn inspect_prepare_release_receipt(
    config_path: impl AsRef<Path>,
    app_key: &str,
) -> CliResult<serde_json::Value> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let channel = project.default_release_channel(app_key, app)?;
    inspect_prepare_release_receipt_for_channel(config_path, app_key, &channel)
}

/// 为集成测试按真实 build 规则创建或复用指定 channel 的 release receipt。
///
/// # Errors
///
/// 配置、Cargo package、现有收据或原子写入不合法时返回错误。
#[allow(dead_code)]
pub fn inspect_prepare_release_receipt_for_channel(
    config_path: impl AsRef<Path>,
    app_key: &str,
    channel: &str,
) -> CliResult<serde_json::Value> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let package_version = cargo_package_version(&project.root, &app.package, false)?;
    let configured = project.resolved_release(app_key, app, channel, Some(package_version))?;
    let targets = if app.targets.required.is_empty() {
        vec![rustc_host_target()?]
    } else {
        app.targets.required.clone()
    };
    let receipt = project.prepare_build_receipt(app_key, app, &configured, &targets)?;
    serde_json::to_value(receipt)
        .map_err(|error| CliError::new(format!("无法序列化 release receipt: {error}")))
}

/// 为集成测试按真实构建规则冻结并校验指定 app/channel 的更新日志。
///
/// 返回冻结路径、字节数和 SHA-256；该入口不会执行 target 构建、签名或归档命令。
///
/// # Errors
///
/// 配置缺少日志、路径越界或不存在、文件不是普通文件、不可读、超过 1 MiB、不是 UTF-8，
/// 或无法写入版本化 dist 目录时返回错误。
#[allow(dead_code)]
pub fn inspect_freeze_release_notes(
    config_path: impl AsRef<Path>,
    app_key: &str,
    channel: &str,
) -> CliResult<Option<serde_json::Value>> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let configured = project.resolved_release(app_key, app, channel, None)?;
    let build_number = match configured.build_number {
        ResolvedBuildNumber::BuildDatetime => {
            build_datetime_number(Local::now().fixed_offset(), None)?
        }
        ResolvedBuildNumber::Literal(value) => value,
    };
    let targets = if app.targets.required.is_empty() {
        vec![rustc_host_target()?]
    } else {
        app.targets.required.clone()
    };
    let release = configured.validated_release(build_number, targets);
    freeze_release_notes(&project.root, app_key, app, &release).map(|notes| {
        notes.map(|notes| {
            serde_json::json!({
                "path": inspect_path(&notes.path),
                "size": notes.metadata.size,
                "sha256": notes.metadata.sha256,
            })
        })
    })
}

/// 为集成测试按真实打包路径写入指定 target 的发布元数据和冻结日志。
///
/// macOS 使用 `.app/Contents/Resources` 共用写入逻辑，Windows 使用 Setup 与 update ZIP
/// 共用的 payload staging 逻辑；该入口不执行编译、签名或归档命令。
///
/// # Errors
///
/// 配置、发布身份、日志冻结、target 选择或资源写入失败时返回错误。
#[allow(dead_code)]
pub fn inspect_release_resources(
    config_path: impl AsRef<Path>,
    app_key: &str,
    target: &str,
) -> CliResult<serde_json::Value> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let channel = project.default_release_channel(app_key, app)?;
    let configured = project.resolved_release(app_key, app, &channel, None)?;
    let build_number = match configured.build_number {
        ResolvedBuildNumber::BuildDatetime => {
            build_datetime_number(Local::now().fixed_offset(), None)?
        }
        ResolvedBuildNumber::Literal(value) => value,
    };
    let targets = if app.targets.required.is_empty() {
        vec![target.to_owned()]
    } else {
        app.targets.required.clone()
    };
    if !targets.iter().any(|configured| configured == target) {
        return Err(CliError::new(format!(
            "app `{app_key}` 不包含 target `{target}`"
        )));
    }
    let release = configured.validated_release(build_number, targets.clone());
    let frozen_notes = freeze_release_notes(&project.root, app_key, app, &release)?;
    let mut plans = project.build_plans(app_key, &release, &targets, frozen_notes.as_ref())?;
    let plan = plans
        .drain(..)
        .find(|plan| plan.target == target)
        .ok_or_else(|| CliError::new(format!("app `{app_key}` 不包含 target `{target}`")))?;
    let directory = match plan.platform {
        BuildTargetPlatform::MacOs => {
            let directory = plan.app_path.join("Contents/Resources");
            write_release_resources_to_directory(&plan, &directory)?;
            directory
        }
        BuildTargetPlatform::Windows => {
            create_parent(&plan.app_path)?;
            fs::write(&plan.app_path, b"test executable")
                .map_err(|error| CliError::new(format!("无法写入测试主程序: {error}")))?;
            stage_windows_update_payload(&plan, None)?
        }
    };
    let metadata: ApplicationReleaseMetadata = serde_json::from_slice(
        &fs::read(directory.join(RELEASE_METADATA_FILE_NAME))
            .map_err(|error| CliError::new(format!("无法读取测试发布元数据: {error}")))?,
    )
    .map_err(|error| CliError::new(format!("无法解析测试发布元数据: {error}")))?;
    let notes_sha256 = if metadata.notes.is_some() {
        Some(sha256_file(&directory.join(RELEASE_NOTES_FILE_NAME))?)
    } else {
        None
    };
    Ok(serde_json::json!({
        "directory": inspect_path(&directory),
        "metadata": metadata,
        "notes_sha256": notes_sha256,
    }))
}

/// 为集成测试执行与 build/publish 相同的 app 选择规则，不触发交互菜单。
///
/// # Errors
///
/// 配置无效、app 不存在或非交互多 app 选择不明确时返回错误。
#[allow(dead_code)]
pub fn inspect_app_selection(
    config_path: impl AsRef<Path>,
    app: Option<&str>,
    all: bool,
) -> CliResult<Vec<String>> {
    let requested = app.map(|app| vec![app.to_owned()]).unwrap_or_default();
    ProjectDocument::load(config_path.as_ref().to_path_buf())?.select_many(&requested, all, false)
}

/// 为集成测试执行 build/publish 的 app/channel 选择规则，不触发交互菜单。
///
/// # Errors
///
/// 配置无效、app/channel 不存在或非交互多 app 选择不明确时返回错误。
#[allow(dead_code)]
pub fn inspect_release_selection(
    config_path: impl AsRef<Path>,
    apps: &[&str],
    all_apps: bool,
    channels: &[&str],
    all_channels: bool,
) -> CliResult<Vec<serde_json::Value>> {
    let requested_apps = apps.iter().map(|app| (*app).to_owned()).collect::<Vec<_>>();
    let requested_channels = channels
        .iter()
        .map(|channel| (*channel).to_owned())
        .collect::<Vec<_>>();
    ProjectDocument::load(config_path.as_ref().to_path_buf())?
        .select_release_targets(
            &requested_apps,
            all_apps,
            &requested_channels,
            all_channels,
            false,
        )?
        .into_iter()
        .map(|(app_key, channel)| Ok(serde_json::json!({ "app": app_key, "channel": channel })))
        .collect()
}

/// 为集成测试执行 publish 的本地产物完整性校验并返回产物 kind。
///
/// # Errors
///
/// 配置、artifact.json、文件大小、摘要或 receipt target 完整性不合法时返回错误。
#[allow(dead_code)]
pub fn inspect_release_artifacts(
    config_path: impl AsRef<Path>,
    app_key: &str,
) -> CliResult<Vec<String>> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let channel = project.default_release_channel(app_key, app)?;
    inspect_release_artifacts_for_channel(config_path, app_key, &channel)
}

/// 为集成测试执行指定 channel 的本地产物完整性校验并返回产物 kind。
///
/// # Errors
///
/// 配置、artifact.json、文件大小、摘要或 receipt target 完整性不合法时返回错误。
#[allow(dead_code)]
pub fn inspect_release_artifacts_for_channel(
    config_path: impl AsRef<Path>,
    app_key: &str,
    channel: &str,
) -> CliResult<Vec<String>> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let release = project.release_from_receipt(app_key, app, channel)?;
    load_release_artifacts(&project.root, app_key, app, &release).map(|artifacts| {
        artifacts
            .into_iter()
            .map(|artifact| artifact_kind_name(artifact.kind).to_owned())
            .collect()
    })
}

/// 为集成测试返回 publish 将写入 channel 根目录的 branded 产物与 checksum key。
///
/// # Errors
///
/// 配置或本地产物预检失败时返回错误。
#[allow(dead_code)]
pub fn inspect_channel_artifact_keys(
    config_path: impl AsRef<Path>,
    app_key: &str,
) -> CliResult<Vec<String>> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let channel = project.default_release_channel(app_key, app)?;
    let release = project.release_from_receipt(app_key, app, &channel)?;
    let artifacts = load_release_artifacts(&project.root, app_key, app, &release)?;
    let prefix = object_key([
        app.object_prefix.as_str(),
        app_key,
        release.channel.as_str(),
    ]);
    channel_artifact_uploads(&artifacts, &prefix)
        .map(|uploads| uploads.into_iter().map(|upload| upload.key).collect())
}

/// 为集成测试返回某个 channel 按字段合并后的有效发布目标，不访问对象存储。
///
/// # Errors
///
/// 配置无效、app 不存在或引用的逻辑发布目标不存在时返回错误。
#[allow(dead_code)]
pub fn inspect_effective_publish_target(
    config_path: impl AsRef<Path>,
    app_key: &str,
    channel: &str,
) -> CliResult<serde_json::Value> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let target = project
        .config
        .publish
        .targets
        .get(&app.publish_target)
        .ok_or_else(|| CliError::new(format!("不存在发布目标 `{}`", app.publish_target)))?
        .for_channel(channel);
    validate_publish_target(&target)?;
    Ok(serde_json::json!({
        "provider": target.provider,
        "endpoint": target.endpoint,
        "bucket": target.bucket,
        "region": target.region,
        "force_path_style": target.force_path_style,
        "public_base_url": target.public_base_url,
        "allow_insecure_http": target.allow_insecure_http,
    }))
}

/// 为集成测试执行指定 channel 的逐字段凭据选择，并仅返回来源。
///
/// # Errors
///
/// 任一必填字段缺失或环境变量不是有效 Unicode 时返回错误。
#[allow(dead_code)]
pub fn inspect_credential_selection(channel: &str) -> CliResult<serde_json::Value> {
    let credentials = S3Credentials::from_env(channel)?;
    Ok(serde_json::json!({
        "access_key_source": credentials.access_key_source,
        "secret_key_source": credentials.secret_key_source,
        "session_token_source": credentials.session_token_source,
        "has_session_token": credentials.session_token.is_some(),
    }))
}

/// 为集成测试返回各宿主缺失构建依赖时使用的官方安装指引。
///
/// 返回值只包含公开命令和下载入口，不检测或修改当前机器。
///
/// # Errors
///
/// `platform` 不是 `macos` 或 `windows` 时返回错误。
#[allow(dead_code)]
pub fn inspect_build_dependency_guidance(platform: &str) -> CliResult<serde_json::Value> {
    match platform {
        "macos" => Ok(serde_json::json!({
            "rustup": RUSTUP_INSTALL_COMMAND,
            "rust_target": "rustup target add <target>",
            "cargo_bundle": "cargo install cargo-bundle",
            "homebrew": HOMEBREW_INSTALL_COMMAND,
            "create_dmg": "brew install create-dmg",
            "xcode_command_line_tools": "xcode-select --install",
            "non_interactive": "fail_with_commands",
        })),
        "windows" => Ok(serde_json::json!({
            "rustup": "https://rustup.rs / rustup-init.exe -y",
            "rust_target": "rustup target add <target>",
            "inno_setup_version": INNO_SETUP_VERSION,
            "inno_setup_install": inno_setup_install_command(),
            "windows_sdk": format!("winget install --source winget --exact --id {WINDOWS_SDK_WINGET_PACKAGE} --accept-package-agreements --accept-source-agreements"),
            "windows_sdk_download": WINDOWS_SDK_DOWNLOAD_URL,
            "non_interactive": "fail_with_commands",
        })),
        other => Err(CliError::new(format!("不支持构建依赖平台 `{other}`"))),
    }
}

/// 为集成测试评估 ISCC 编译器引擎版本与当前终端修复策略，不执行任何外部命令。
///
/// 返回 `ready` 表示引擎版本与框架固定版本一致；返回 `install` 表示交互式
/// `--fix` 可以继续执行 winget 安装。
///
/// # Errors
///
/// 输出缺少版本、版本不匹配且未启用修复，或非交互环境请求自动修复时返回精确安装指引。
#[allow(dead_code)]
pub fn inspect_inno_setup_requirement(
    engine_output: Option<&str>,
    fix: bool,
    interactive: bool,
) -> CliResult<String> {
    let expected = Version::parse(INNO_SETUP_VERSION)
        .map_err(|error| CliError::new(format!("内置 Inno Setup 版本无效: {error}")))?;
    if engine_output
        .and_then(parse_inno_setup_engine_version)
        .is_some_and(|actual| actual == expected)
    {
        return Ok("ready".to_owned());
    }
    let manual = inno_setup_install_command();
    require_repair_for_terminal(
        &format!("Inno Setup {INNO_SETUP_VERSION}"),
        &manual,
        fix,
        interactive,
    )?;
    Ok("install".to_owned())
}

/// 启动真实 `ISCC.exe` 并读取其编译器引擎版本，供 Windows 宿主回归测试验证探测链路。
///
/// 命令使用标准输入模式且立即发送 EOF，不创建临时脚本或安装产物。
///
/// # Errors
///
/// 文件不存在、编译器无法启动，或输出缺少精确引擎版本时返回错误。
#[cfg(windows)]
#[allow(dead_code)]
pub fn inspect_inno_setup_compiler_version(path: impl AsRef<Path>) -> CliResult<String> {
    inno_setup_compiler_version(path.as_ref()).map(|version| version.to_string())
}

/// 为集成测试返回稳定 app_key 对象布局和公开 URL，不访问对象存储。
///
/// # Errors
///
/// 配置、receipt、target 或 target triple 无效时返回错误。
#[allow(dead_code)]
pub fn inspect_publish_object_layout(
    config_path: impl AsRef<Path>,
    app_key: &str,
    target_triple: &str,
    artifact_name: &str,
) -> CliResult<serde_json::Value> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let channel = project.default_release_channel(app_key, app)?;
    let release = project.release_from_receipt(app_key, app, &channel)?;
    let target = project.config.publish.targets[&app.publish_target].for_channel(&release.channel);
    let channel_prefix = object_key([
        app.object_prefix.as_str(),
        app_key,
        release.channel.as_str(),
    ]);
    let versioned_key = object_key([
        channel_prefix.as_str(),
        release.version.to_string().as_str(),
        release.build_number.to_string().as_str(),
        target_arch_alias(target_triple)?,
        artifact_name,
    ]);
    let channel_key = object_key([channel_prefix.as_str(), artifact_name]);
    let latest_key = object_key([channel_prefix.as_str(), "latest.json"]);
    let sequence_key = object_key([channel_prefix.as_str(), "manifests", "42.json"]);
    Ok(serde_json::json!({
        "latest_key": latest_key,
        "sequence_key": sequence_key,
        "channel_key": channel_key,
        "versioned_key": versioned_key,
        "channel_url": public_object_url(&target, &channel_key),
        "versioned_url": public_object_url(&target, &versioned_key),
    }))
}

/// 为集成测试读取并校验发布私钥，返回与可信公钥匹配的 key id。
///
/// # Errors
///
/// 私钥来源不可用、格式非法、key id 未信任或派生公钥不匹配时返回错误。
#[allow(dead_code)]
pub fn inspect_signing_key(config_path: impl AsRef<Path>, app_key: &str) -> CliResult<String> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let trusted = parse_trusted_keys(&app.updater.trusted_public_keys)?;
    read_signing_key(&project, app_key, app, &trusted).map(|(key_id, _)| key_id)
}

/// 为集成测试返回 Windows Inno Setup 脚本和 ISCC 参数快照，不执行编译或签名命令。
///
/// # Errors
///
/// 配置无效、app 不存在、没有 Windows target 或 Windows 打包配置不完整时返回错误。
#[allow(dead_code)]
pub fn inspect_windows_installer_sources(
    config_path: impl AsRef<Path>,
    app_key: &str,
) -> CliResult<serde_json::Value> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let channel = project.default_release_channel(app_key, app)?;
    let configured = project.resolved_release(app_key, app, &channel, None)?;
    let build_number = match configured.build_number {
        ResolvedBuildNumber::Literal(value) => value,
        ResolvedBuildNumber::BuildDatetime => {
            build_datetime_number(Local::now().fixed_offset(), None)?
        }
    };
    let configured_targets = if app.targets.required.is_empty() {
        vec![rustc_host_target()?]
    } else {
        app.targets.required.clone()
    };
    let release = configured.validated_release(build_number, configured_targets.clone());
    let brand_assets = project.brand_assets(app_key, app)?;
    let publish_target = &project.config.publish.targets[&app.publish_target];
    let target = configured_targets
        .iter()
        .find(|target| is_windows_target(target))
        .ok_or_else(|| CliError::new(format!("app `{app_key}` 没有 Windows target")))?
        .clone();
    let arch = target_arch_alias(&target)?;
    let release_dir = project
        .root
        .join(DIST_DIRECTORY)
        .join(app_key)
        .join(&release.channel)
        .join(release.version.to_string())
        .join(release.build_number.to_string())
        .join(&target);
    let artifact_stem = format!("{}-{arch}", app.display_name);
    let plan = BuildPlan {
        project_root: project.root.clone(),
        app_key: app_key.to_owned(),
        package: app.package.clone(),
        app_id: app.app_id.clone(),
        updater_app_id: app
            .updater
            .app_id
            .clone()
            .unwrap_or_else(|| app.app_id.clone()),
        display_name: app.display_name.clone(),
        release,
        target: target.clone(),
        platform: BuildTargetPlatform::Windows,
        signing: app.platforms.macos.signing,
        notarize: app.platforms.macos.notarize,
        expected_team_id: None,
        allow_insecure_http: publish_target.allow_insecure_http,
        updater: app.updater.enabled.then(|| app.updater.clone()),
        macos_icon: brand_assets.macos_icon,
        windows_icon: brand_assets.windows_icon,
        windows: Some(windows_build_options(&app.platforms.windows)?),
        app_path: windows_binary_path(&project.root, &target, &app.package),
        app_zip_path: release_dir.join(format!("{artifact_stem}.windows.zip")),
        dmg_path: release_dir.join(format!("{artifact_stem}.dmg")),
        setup_path: release_dir.join(format!("{artifact_stem}.exe")),
        artifact_path: release_dir.join("artifact.json"),
        notes_path: project
            .root
            .join(DIST_DIRECTORY)
            .join(app_key)
            .join("notes.md"),
        notes: None,
    };
    let staging = windows_work_dir(&plan).join("payload");
    let source = windows_installer_source(&plan, &staging)?;
    Ok(serde_json::json!({
        "file_version": source.file_version,
        "iss": source.iss,
        "updater_config": source.updater_config,
    }))
}

/// 在 Windows 集成测试中使用真实 `ISCC.exe` 编译生产路径生成的安装脚本。
///
/// # Errors
///
/// 配置或安装脚本无效、测试 payload 无法写入、`ISCC.exe` 执行失败，或未生成预期安装包时
/// 返回错误。
#[cfg(windows)]
#[allow(dead_code)]
pub fn inspect_compile_windows_installer(
    config_path: impl AsRef<Path>,
    app_key: &str,
    compiler: impl AsRef<Path>,
    output_directory: impl AsRef<Path>,
) -> CliResult<PathBuf> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let mut plan = inspection_windows_build_plan(&project, app_key)?;
    let output_directory = output_directory.as_ref();
    fs::create_dir_all(output_directory).map_err(|error| {
        CliError::new(format!(
            "无法创建 Inno Setup smoke 目录 `{}`: {error}",
            output_directory.display()
        ))
    })?;
    plan.setup_path = output_directory.join("nexora-inno-smoke.exe");
    let staging = output_directory.join("payload");
    fs::create_dir_all(&staging).map_err(|error| {
        CliError::new(format!(
            "无法创建 Inno Setup smoke payload `{}`: {error}",
            staging.display()
        ))
    })?;
    fs::write(
        staging.join(safe_file_name(&plan.app_path)?),
        b"nexora smoke\n",
    )
    .map_err(|error| CliError::new(format!("无法写入 Inno Setup smoke payload: {error}")))?;
    let source = write_windows_installer_source(&plan, &staging)?;
    run_inno_setup(compiler.as_ref(), &source, &plan.setup_path)?;
    Ok(plan.setup_path)
}

/// 为集成测试返回主进程与 updater 独立的 Windows PE 资源脚本。
///
/// # Errors
///
/// 配置、发布 identity 或 Windows 构建选项无效时返回错误。
#[allow(dead_code)]
pub fn inspect_windows_resource_scripts(
    config_path: impl AsRef<Path>,
    app_key: &str,
) -> CliResult<serde_json::Value> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let plan = inspection_windows_build_plan(&project, app_key)?;
    Ok(serde_json::json!({
        "main": windows_resource_script(&plan, WindowsResourceKind::Main)?,
        "updater": windows_resource_script(&plan, WindowsResourceKind::Updater)?,
    }))
}

fn inspection_windows_build_plan(project: &ProjectDocument, app_key: &str) -> CliResult<BuildPlan> {
    let app = project
        .config
        .apps
        .get(app_key)
        .ok_or_else(|| CliError::new(format!("不存在 app `{app_key}`")))?;
    let channel = project.default_release_channel(app_key, app)?;
    let configured = project.resolved_release(app_key, app, &channel, None)?;
    let build_number = match configured.build_number {
        ResolvedBuildNumber::Literal(value) => value,
        ResolvedBuildNumber::BuildDatetime => {
            build_datetime_number(Local::now().fixed_offset(), None)?
        }
    };
    let targets = if app.targets.required.is_empty() {
        vec![rustc_host_target()?]
    } else {
        app.targets.required.clone()
    };
    let release = configured.validated_release(build_number, targets.clone());
    let plans = project.build_plans(app_key, &release, &targets, None)?;
    plans
        .into_iter()
        .find(|plan| plan.platform == BuildTargetPlatform::Windows)
        .ok_or_else(|| CliError::new(format!("app `{app_key}` 没有 Windows target")))
}

/// 在 Windows 集成测试中把主进程与 updater 的独立资源链接进最小 PE executable。
///
/// # Errors
///
/// 配置或资源无效、Windows SDK `rc.exe` 不可用，或 `rustc` 无法链接 PE 时返回错误。
#[cfg(windows)]
#[allow(dead_code)]
pub fn inspect_compile_windows_resource_executables(
    config_path: impl AsRef<Path>,
    app_key: &str,
    output_directory: impl AsRef<Path>,
) -> CliResult<(PathBuf, PathBuf)> {
    let project = ProjectDocument::load(config_path.as_ref().to_path_buf())?;
    let plan = inspection_windows_build_plan(&project, app_key)?;
    let output_directory = output_directory.as_ref();
    fs::create_dir_all(output_directory).map_err(|error| {
        CliError::new(format!(
            "无法创建 PE resource smoke 目录 `{}`: {error}",
            output_directory.display()
        ))
    })?;
    let source = output_directory.join("resource-smoke.rs");
    fs::write(&source, "fn main() {}\n").map_err(|error| {
        CliError::new(format!(
            "无法写入 PE resource smoke 源文件 `{}`: {error}",
            source.display()
        ))
    })?;
    let outputs = [
        (
            WindowsResourceKind::Main,
            output_directory.join(format!("{}.exe", plan.package)),
        ),
        (
            WindowsResourceKind::Updater,
            output_directory.join(format!("{}-updater.exe", plan.package)),
        ),
    ];
    for (kind, output) in &outputs {
        let resource = compile_windows_resource(&plan, *kind)?;
        run_status(
            "rustc PE resource smoke executable",
            Command::new("rustc")
                .arg(&source)
                .args(["--target", &plan.target])
                .arg("-o")
                .arg(output)
                .arg("-C")
                .arg(format!("link-arg={}", resource.display())),
        )?;
    }
    Ok((outputs[0].1.clone(), outputs[1].1.clone()))
}

/// 为集成测试返回 Windows 主程序与 updater sidecar 使用的 MSVC 链接参数。
///
/// 返回值保持实际 `cargo rustc` 命令中的参数顺序，用于验证资源文件、GUI 子系统与
/// Rust `main` 对应的 CRT 入口点会被一起传给链接器。
#[allow(dead_code)]
pub fn inspect_windows_binary_link_args(resource: impl AsRef<Path>) -> Vec<String> {
    windows_binary_link_args(resource.as_ref())
        .into_iter()
        .collect()
}

/// 为集成测试从 Windows payload staging 生成更新 ZIP。
///
/// # Errors
///
/// staging 无法读取、目标文件无法创建，或任一文件无法写入 ZIP 时返回错误。
#[allow(dead_code)]
pub fn inspect_create_windows_update_zip(
    staging: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> CliResult<()> {
    create_windows_update_zip_at(staging.as_ref(), destination.as_ref())
}
