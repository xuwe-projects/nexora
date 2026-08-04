//! `nexora build` 与 `nexora publish` 的配置、打包和发布实现。

use super::{CliError, CliResult};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{TimeZone as _, Utc};
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
    io::{IsTerminal as _, Write as _},
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const CONFIG_FILE_NAME: &str = "nexora.toml";
const ARTIFACT_SCHEMA_VERSION: u32 = 1;
const RELEASE_RECEIPT_SCHEMA_VERSION: u32 = 2;
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const DIST_DIRECTORY: &str = "dist";
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";
const MUTABLE_CACHE: &str = "no-cache";

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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetConfig {
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
    installer: WindowsInstaller,
    #[serde(default)]
    install_scope: WindowsInstallScope,
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
enum WindowsInstaller {
    #[default]
    Nsis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum WindowsInstallScope {
    #[default]
    User,
    Machine,
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
    notes_source: PathBuf,
    notes_path: PathBuf,
}

#[derive(Debug, Clone)]
struct WindowsBuildOptions {
    install_scope: WindowsInstallScope,
    publisher: String,
    signing: WindowsSigningMode,
    signing_thumbprint: Option<String>,
    timestamp_url: Option<String>,
    expected_publisher: Option<String>,
    desktop_shortcut_default: bool,
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
        })
    }
}

impl ResolvedReleaseConfig {
    fn validated_release(&self, build_number: u64) -> ValidatedRelease {
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
    latest_dmg_aliases: Vec<Upload>,
    latest: Upload,
    latest_json: Vec<u8>,
    verify_urls: Vec<Verification>,
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
    for (app_key, channel) in selections {
        let app = &project.config.apps[&app_key];
        let package_version = cargo_package_version(&project.root, &app.package, false)?;
        let configured =
            project.resolved_release(&app_key, app, &channel, Some(package_version))?;
        let targets = app
            .targets
            .required
            .iter()
            .filter(|target| host_can_build(target))
            .cloned()
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Err(CliError::new(format!(
                "当前宿主不能构建 app `{app_key}` channel `{channel}` 的任何 required target"
            )));
        }
        let receipt = project.prepare_build_receipt(&app_key, app, &configured, &targets)?;
        let release = receipt.validated_release(&configured)?;
        let plans = project.build_plans(&app_key, &release, false)?;
        if plans.is_empty() {
            return Err(CliError::new(format!(
                "当前宿主不能构建 app `{app_key}` channel `{channel}` 的任何 required target"
            )));
        }
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

    let credentials = S3Credentials::from_env()?;
    for plan in &plans {
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
        }
        for (app_key, app) in &self.config.apps {
            validate_safe_component(app_key, "app key")?;
            validate_safe_component(&app.package, "package")?;
            validate_app_id(&app.app_id)?;
            validate_display_name(&app.display_name)?;
            validate_safe_component(&app.object_prefix, "object_prefix")?;
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
            if app.targets.required.is_empty() {
                return Err(CliError::new(format!(
                    "app `{app_key}` 的 targets.required 不能为空"
                )));
            }
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
        let target = &self.config.publish.targets[&app.publish_target];
        let updater_feed = public_object_url(
            target,
            &object_key([
                app.object_prefix.as_str(),
                app_key,
                selected_channel,
                "latest.json",
            ]),
        );
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
            if receipt_matches_configuration(receipt, app_key, app, configured, targets)
                && (!matches!(configured.build_number, ResolvedBuildNumber::BuildDatetime)
                    || !release_targets_complete(
                        &self.root,
                        app_key,
                        app,
                        &receipt.validated_release(configured)?,
                    ))
            {
                println!(
                    "复用 release receipt：{} / build {}",
                    receipt.version, receipt.build_number
                );
                return Ok(receipt.clone());
            }
        }

        let previous_build_number = previous.as_ref().map(|receipt| receipt.build_number);
        let build_number = match configured.build_number {
            ResolvedBuildNumber::Literal(value) => value,
            ResolvedBuildNumber::BuildDatetime => {
                build_datetime_number(Utc::now(), previous_build_number)?
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
        if !receipt_matches_configuration(
            &receipt,
            app_key,
            app,
            &configured,
            &app.targets.required,
        ) {
            return Err(CliError::new(format!(
                "`{}` 与当前 app/package/channel/version/source/build_number/targets 配置不一致",
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
        include_all_targets: bool,
    ) -> CliResult<Vec<BuildPlan>> {
        let app = &self.config.apps[app_key];
        let brand_assets = self.brand_assets(app_key, app)?;
        let publish_target = &self.config.publish.targets[&app.publish_target];
        app.targets
            .required
            .iter()
            .filter(|target| include_all_targets || host_can_build(target))
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
                let technical_stem = format!(
                    "{}-{}-{}-{arch}",
                    app.package, release.version, release.build_number
                );
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
                            release_dir.join(format!("{technical_stem}.app.zip"))
                        }
                        BuildTargetPlatform::Windows => {
                            release_dir.join(format!("{technical_stem}.windows.zip"))
                        }
                    },
                    dmg_path: release_dir.join(format!("{technical_stem}.dmg")),
                    setup_path: release_dir.join(format!("{technical_stem}.setup.exe")),
                    artifact_path: release_dir.join("artifact.json"),
                    notes_source: self
                        .root
                        .join("docs/changelog/components")
                        .join(release.version.to_string())
                        .join(&app.package)
                        .join("zh-CN.md"),
                    notes_path: self
                        .root
                        .join(DIST_DIRECTORY)
                        .join(app_key)
                        .join(&release.channel)
                        .join(release.version.to_string())
                        .join(release.build_number.to_string())
                        .join("notes.md"),
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
        let target = self.config.publish.targets[&app.publish_target].clone();
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
        let mut latest_dmg_aliases = Vec::new();
        let mut verify_urls = Vec::new();
        let mut manifest_artifacts = Vec::new();
        if matches!(status, ReleaseStatus::Available) {
            let local_artifacts = load_release_artifacts(&self.root, app_key, app, &release)?;
            let release_prefix = object_key([
                channel_prefix.as_str(),
                "releases",
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
                    let key = object_key([
                        release_prefix.as_str(),
                        artifact.target.as_str(),
                        artifact.file_name.as_str(),
                    ]);
                    let url = public_object_url(&target, &key);
                    immutable_payloads.push(Upload {
                        key,
                        source: UploadSource::File(artifact.path.clone()),
                        content_type: artifact_content_type(kind),
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
                        verify_urls.push(Verification {
                            url,
                            expected_sha256: artifact.sha256.clone(),
                            label: format!("{} updater ZIP", artifact.target),
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
            let notes_url = if notes_path.is_file() {
                let key = object_key([release_prefix.as_str(), "notes.md"]);
                let url = public_object_url(&target, &key);
                immutable_payloads.push(Upload {
                    key,
                    source: UploadSource::File(notes_path),
                    content_type: "text/markdown; charset=utf-8",
                    cache_control: IMMUTABLE_CACHE,
                    immutable: true,
                });
                Some(url)
            } else {
                None
            };
            latest_dmg_aliases = latest_dmg_uploads(
                &local_artifacts,
                &channel_prefix,
                app.targets.required.len() == 1,
            )?;
            for upload in &latest_dmg_aliases {
                let bytes = upload.source.bytes()?;
                verify_urls.push(Verification {
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
                notes_url,
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
                latest_dmg_aliases,
                latest_key,
                latest_url,
                verify_urls,
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
            latest_dmg_aliases,
            latest_key,
            latest_url,
            verify_urls,
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
    latest_dmg_aliases: Vec<Upload>,
    latest_key: String,
    latest_url: String,
    verify_urls: Vec<Verification>,
    signing_key_id: String,
    signing_key: &SigningKey,
    payload: ManifestPayload,
) -> CliResult<PublishPlan> {
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
        required_targets: app.targets.required.clone(),
        immutable_payloads,
        sequence_manifest: Upload {
            key: sequence_key,
            source: UploadSource::Bytes(latest_json.clone()),
            content_type: "application/json; charset=utf-8",
            cache_control: IMMUTABLE_CACHE,
            immutable: true,
        },
        latest_dmg_aliases,
        latest: Upload {
            key: latest_key,
            source: UploadSource::Bytes(latest_json.clone()),
            content_type: "application/json; charset=utf-8",
            cache_control: MUTABLE_CACHE,
            immutable: false,
        },
        latest_json,
        verify_urls,
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
    copy_release_notes(plan)?;
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
    ensure_windows_build_dependencies(plan)?;
    let resource = compile_windows_icon_resource(plan)?;
    build_windows_binary(plan, &plan.package, &resource)?;
    let updater_path = if plan.updater.is_some() {
        let sidecar = format!("{}-updater", plan.package);
        build_windows_binary(plan, &sidecar, &resource)?;
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
    let script = write_windows_nsis_script(plan, &staging)?;
    build_windows_setup(plan, &script)?;
    sign_windows_file(plan, &plan.setup_path)?;
    copy_release_notes(plan)?;
    write_artifact_manifest(plan)?;
    println!("EXE: {}", plan.app_path.display());
    println!("WINDOWS ZIP: {}", plan.app_zip_path.display());
    println!("SETUP: {}", plan.setup_path.display());
    println!("ARTIFACT: {}", plan.artifact_path.display());
    Ok(())
}

fn ensure_build_dependencies(plan: &BuildPlan) -> CliResult<()> {
    require_command("cargo")?;
    require_command("rustup")?;
    run_status(
        "rustup target add",
        Command::new("rustup")
            .current_dir(&plan.project_root)
            .args(["target", "add", &plan.target]),
    )?;
    if !command_exists("cargo-bundle") {
        run_status(
            "cargo install cargo-bundle",
            Command::new("cargo").args(["install", "cargo-bundle"]),
        )?;
    }
    if !command_exists("create-dmg") {
        require_command("brew")?;
        run_status(
            "brew install create-dmg",
            Command::new("brew").args(["install", "create-dmg"]),
        )?;
    }
    require_command("ditto")?;
    require_command("plutil")?;
    if plan.signing != SigningMode::None {
        require_command("codesign")?;
    }
    if plan.notarize {
        require_command("xcrun")?;
    }
    Ok(())
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
        expected_windows_signer_thumbprint: windows
            .and_then(|options| options.signing_thumbprint.clone())
            .or_else(|| {
                windows
                    .filter(|options| options.signing == WindowsSigningMode::Authenticode)
                    .and_then(|_| {
                        env::var("WINDOWS_SIGN_CERTIFICATE_SHA1")
                            .ok()
                            .filter(|value| !value.trim().is_empty())
                    })
            }),
        expected_windows_publisher: windows.and_then(|options| {
            options
                .expected_publisher
                .clone()
                .or_else(|| Some(options.publisher.clone()))
        }),
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
        return Err(CliError::new("当前宿主不能构建 Windows required targets"));
    }
    require_command("cargo")?;
    require_command("rustup")?;
    run_status(
        "rustup target add",
        Command::new("rustup")
            .current_dir(&plan.project_root)
            .args(["target", "add", &plan.target]),
    )?;
    require_command("rc")?;
    let _ = makensis_command()?;
    if plan
        .windows
        .as_ref()
        .is_some_and(|options| options.signing == WindowsSigningMode::Authenticode)
    {
        require_command("signtool")?;
    }
    Ok(())
}

fn windows_work_dir(plan: &BuildPlan) -> PathBuf {
    plan.project_root
        .join(".runtime")
        .join("windows-build")
        .join(&plan.app_key)
        .join(&plan.target)
}

fn compile_windows_icon_resource(plan: &BuildPlan) -> CliResult<PathBuf> {
    validate_ico(&plan.windows_icon)?;
    let work_dir = windows_work_dir(plan);
    fs::create_dir_all(&work_dir)
        .map_err(|error| CliError::new(format!("无法创建 `{}`: {error}", work_dir.display())))?;
    let rc_path = work_dir.join("nexora-icon.rc");
    let res_path = work_dir.join("nexora-icon.res");
    let contents = format!(
        "1 ICON \"{}\"\r\n",
        escape_rc_string(&plan.windows_icon.to_string_lossy())
    );
    fs::write(&rc_path, contents).map_err(|error| {
        CliError::new(format!(
            "无法写入 Windows resource `{}`: {error}",
            rc_path.display()
        ))
    })?;
    run_status(
        "rc Windows resources",
        Command::new("rc")
            .arg("/nologo")
            .arg(format!("/fo{}", res_path.display()))
            .arg(&rc_path),
    )?;
    Ok(res_path)
}

fn build_windows_binary(plan: &BuildPlan, binary: &str, resource: &Path) -> CliResult<()> {
    let mut command = Command::new("cargo");
    command
        .current_dir(&plan.project_root)
        .args(["build", "--release", "--package", &plan.package])
        .args(["--bin", binary, "--target", &plan.target])
        .env("RUSTFLAGS", windows_resource_rustflags(resource));
    run_status("cargo build Windows binary", &mut command)
}

fn windows_resource_rustflags(resource: &Path) -> String {
    let value = format!("-C link-arg={}", resource.display());
    match env::var("RUSTFLAGS")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        Some(existing) => format!("{existing} {value}"),
        None => value,
    }
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
    run_status(
        "signtool sign",
        Command::new("signtool")
            .args(["sign", "/fd", "SHA256", "/s", "My", "/sha1"])
            .arg(&thumbprint)
            .args(["/tr", timestamp_url, "/td", "SHA256"])
            .arg(path),
    )?;
    run_status(
        "signtool verify",
        Command::new("signtool").args(["verify", "/pa"]).arg(path),
    )
}

fn resolve_windows_signing_thumbprint(options: &WindowsBuildOptions) -> CliResult<String> {
    options
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
    Ok(staging)
}

fn create_windows_update_zip(plan: &BuildPlan, staging: &Path) -> CliResult<()> {
    create_parent(&plan.app_zip_path)?;
    remove_existing_file(&plan.app_zip_path)?;
    let script_path = windows_work_dir(plan).join("create-update-zip.ps1");
    let script = r#"
param(
  [Parameter(Mandatory=$true)][string]$source,
  [Parameter(Mandatory=$true)][string]$destination
)
$items = Get-ChildItem -LiteralPath $source -Force
Compress-Archive -LiteralPath $items.FullName -DestinationPath $destination -Force
"#;
    fs::write(&script_path, script).map_err(|error| {
        CliError::new(format!(
            "无法写入 Windows ZIP 脚本 `{}`: {error}",
            script_path.display()
        ))
    })?;
    run_status(
        "Compress-Archive Windows update ZIP",
        Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script_path)
            .arg(staging)
            .arg(&plan.app_zip_path),
    )
}

struct WindowsInstallerSources {
    nsis_script: String,
    updater_config: Option<BundledUpdaterConfig>,
    file_version: String,
}

fn write_windows_nsis_script(plan: &BuildPlan, staging: &Path) -> CliResult<PathBuf> {
    let sources = windows_installer_sources(plan, staging)?;
    let script_path = windows_work_dir(plan).join("setup.nsi");
    let mut contents = vec![0xEF, 0xBB, 0xBF];
    contents.extend_from_slice(sources.nsis_script.as_bytes());
    fs::write(&script_path, contents).map_err(|error| {
        CliError::new(format!(
            "无法写入 NSIS 脚本 `{}`: {error}",
            script_path.display()
        ))
    })?;
    Ok(script_path)
}

fn build_windows_setup(plan: &BuildPlan, script: &Path) -> CliResult<()> {
    create_parent(&plan.setup_path)?;
    remove_existing_file(&plan.setup_path)?;
    let makensis = makensis_command()?;
    run_status(
        "makensis Windows Setup.exe",
        Command::new(makensis).arg("/V2").arg(script),
    )
}

fn windows_installer_sources(
    plan: &BuildPlan,
    staging: &Path,
) -> CliResult<WindowsInstallerSources> {
    let options = windows_options(plan)?;
    let install_dir = match options.install_scope {
        WindowsInstallScope::User => format!("$LOCALAPPDATA\\Programs\\{}", plan.app_id),
        WindowsInstallScope::Machine => format!("$PROGRAMFILES64\\{}", plan.app_id),
    };
    let request_level = match options.install_scope {
        WindowsInstallScope::User => "user",
        WindowsInstallScope::Machine => "admin",
    };
    let default_desktop_shortcut = if options.desktop_shortcut_default {
        "1"
    } else {
        "0"
    };
    let default_launch = if options.launch_after_install_default {
        "1"
    } else {
        "0"
    };
    let file_version = windows_file_version(&plan.release.version, plan.release.build_number)?;
    let source = nsis_path(staging);
    let out_file = nsis_path(&plan.setup_path);
    let display_name = nsis_string(&plan.display_name);
    let publisher = nsis_string(&options.publisher);
    let app_id = nsis_string(&plan.app_id);
    let package = nsis_string(&plan.package);
    let main_exe = nsis_string(&safe_file_name(&plan.app_path)?);
    let minimum_build = options.minimum_windows_build;
    let script = format!(
        r#"!include "MUI2.nsh"
!include "LogicLib.nsh"

Unicode true
Name "{display_name}"
OutFile "{out_file}"
InstallDir "{install_dir}"
RequestExecutionLevel {request_level}
SetCompressor /SOLID lzma
SetCompressorDictSize 32

VIProductVersion "{file_version}"
VIAddVersionKey /LANG=1033 "ProductName" "{display_name}"
VIAddVersionKey /LANG=1033 "CompanyName" "{publisher}"
VIAddVersionKey /LANG=1033 "FileDescription" "{display_name} Setup"
VIAddVersionKey /LANG=1033 "FileVersion" "{file_version}"

Var CreateDesktopShortcut
Var LaunchAfterInstall

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

Function .onInit
  StrCpy $CreateDesktopShortcut {default_desktop_shortcut}
  StrCpy $LaunchAfterInstall {default_launch}
  ReadRegDWORD $1 SHCTX "Software\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ${{If}} $1 < {minimum_build}
    MessageBox MB_ICONSTOP "{display_name} 需要 Windows build {minimum_build} 或更高版本。"
    Abort
  ${{EndIf}}
  ReadRegDWORD $1 SHCTX "Software\{app_id}" "BuildNumber"
  ${{If}} $1 > {build_number}
    MessageBox MB_ICONSTOP "已安装更新版本，Setup 不会执行降级安装。"
    Abort
  ${{EndIf}}
FunctionEnd

Section "Install"
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "{source}\*.*"
  WriteRegStr SHCTX "Software\{app_id}" "InstallDir" "$INSTDIR"
  WriteRegStr SHCTX "Software\{app_id}" "DisplayName" "{display_name}"
  WriteRegDWORD SHCTX "Software\{app_id}" "BuildNumber" {build_number}
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{app_id}" "DisplayName" "{display_name}"
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{app_id}" "Publisher" "{publisher}"
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{app_id}" "DisplayVersion" "{version}"
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{app_id}" "InstallLocation" "$INSTDIR"
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{app_id}" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\{display_name}"
  CreateShortcut "$SMPROGRAMS\{display_name}\{display_name}.lnk" "$INSTDIR\{main_exe}"
  ${{If}} $CreateDesktopShortcut == 1
    CreateShortcut "$DESKTOP\{display_name}.lnk" "$INSTDIR\{main_exe}"
  ${{EndIf}}
  ${{If}} $LaunchAfterInstall == 1
    ExecShell "" "$INSTDIR\{main_exe}"
  ${{EndIf}}
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\{main_exe}"
  Delete "$INSTDIR\{package}-updater.exe"
  Delete "$INSTDIR\nexora-updater.json"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$SMPROGRAMS\{display_name}"
  Delete "$DESKTOP\{display_name}.lnk"
  DeleteRegKey SHCTX "Software\Microsoft\Windows\CurrentVersion\Uninstall\{app_id}"
  DeleteRegKey SHCTX "Software\{app_id}"
  RMDir "$INSTDIR"
SectionEnd
"#,
        build_number = plan.release.build_number,
        version = plan.release.version,
    );
    Ok(WindowsInstallerSources {
        nsis_script: script,
        updater_config: plan
            .updater
            .as_ref()
            .map(|_| bundled_updater_config(plan))
            .transpose()?,
        file_version,
    })
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
    if config.installer != WindowsInstaller::Nsis {
        return Err(CliError::new("Windows 当前只支持 NSIS Setup.exe"));
    }
    let publisher = config
        .publisher
        .clone()
        .ok_or_else(|| CliError::new("Windows Setup.exe 需要 platforms.windows.publisher"))?;
    Ok(WindowsBuildOptions {
        install_scope: config.install_scope,
        publisher,
        signing: config.signing,
        signing_thumbprint: config.signing_thumbprint.clone(),
        timestamp_url: config.timestamp_url.clone(),
        expected_publisher: config.expected_publisher.clone(),
        desktop_shortcut_default: config.desktop_shortcut_default,
        launch_after_install_default: config.launch_after_install_default,
        minimum_windows_build: config.minimum_windows_build.unwrap_or(19045),
    })
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
    target == "x86_64-pc-windows-msvc"
}

fn target_platform(target: &str) -> CliResult<BuildTargetPlatform> {
    if target.ends_with("-apple-darwin") {
        Ok(BuildTargetPlatform::MacOs)
    } else if is_windows_target(target) {
        Ok(BuildTargetPlatform::Windows)
    } else {
        Err(CliError::new(format!(
            "当前只支持 macOS 与 Windows required target，收到 `{target}`"
        )))
    }
}

fn escape_rc_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn nsis_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "$\"")
}

fn nsis_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "$\"")
}

fn makensis_command() -> CliResult<PathBuf> {
    if command_exists("makensis") {
        return Ok(PathBuf::from("makensis"));
    }
    let candidates = [
        PathBuf::from(r"C:\Program Files (x86)\NSIS\makensis.exe"),
        PathBuf::from(r"C:\Program Files\NSIS\makensis.exe"),
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(r"Programs\NSIS\makensis.exe"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| CliError::new("缺少命令 `makensis`，请安装 NSIS 3 Unicode"))
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

fn copy_release_notes(plan: &BuildPlan) -> CliResult<()> {
    if !plan.notes_source.is_file() {
        return Ok(());
    }
    create_parent(&plan.notes_path)?;
    fs::copy(&plan.notes_source, &plan.notes_path).map_err(|error| {
        CliError::new(format!(
            "无法复制 release notes `{}`: {error}",
            plan.notes_source.display()
        ))
    })?;
    Ok(())
}

fn write_artifact_manifest(plan: &BuildPlan) -> CliResult<()> {
    let artifacts = match plan.platform {
        BuildTargetPlatform::MacOs => vec![
            artifact_entry(ArtifactKind::MacosAppZip, &plan.app_zip_path)?,
            artifact_entry(ArtifactKind::MacosDmg, &plan.dmg_path)?,
        ],
        BuildTargetPlatform::Windows => vec![
            artifact_entry(ArtifactKind::WindowsSetupExe, &plan.setup_path)?,
            artifact_entry(ArtifactKind::WindowsZip, &plan.app_zip_path)?,
        ],
    };
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
    for target in &app.targets.required {
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
                ArtifactKind::WindowsSetupExe => ".setup.exe",
                ArtifactKind::WindowsZip => ".windows.zip",
            };
            if !artifact.file_name.ends_with(expected_suffix) {
                return Err(CliError::new(format!(
                    "artifact `{}` 的 kind 与文件扩展名不一致",
                    artifact.file_name
                )));
            }
            let expected_name = format!(
                "{}-{}-{}-{}{}",
                app.package,
                release.version,
                release.build_number,
                target_arch_alias(target)?,
                expected_suffix
            );
            if artifact.file_name != expected_name {
                return Err(CliError::new(format!(
                    "artifact 技术文件名不匹配；期望 `{expected_name}`，实际 `{}`",
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

fn latest_dmg_uploads(
    artifacts: &[LocalArtifact],
    channel_prefix: &str,
    single_target: bool,
) -> CliResult<Vec<Upload>> {
    let dmgs = artifacts
        .iter()
        .filter(|artifact| artifact.kind == ArtifactKind::MacosDmg)
        .collect::<Vec<_>>();
    let mut uploads = dmgs
        .iter()
        .map(|artifact| {
            Ok(Upload {
                key: object_key([
                    channel_prefix,
                    format!("latest-{}.dmg", target_arch_alias(&artifact.target)?).as_str(),
                ]),
                source: UploadSource::File(artifact.path.clone()),
                content_type: "application/x-apple-diskimage",
                cache_control: MUTABLE_CACHE,
                immutable: false,
            })
        })
        .collect::<CliResult<Vec<_>>>()?;
    if single_target && let Some(dmg) = dmgs.first() {
        uploads.push(Upload {
            key: object_key([channel_prefix, "latest.dmg"]),
            source: UploadSource::File(dmg.path.clone()),
            content_type: "application/x-apple-diskimage",
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
    for upload in &plan.latest_dmg_aliases {
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
    upload_object(client, &plan.target, credentials, &plan.sequence_manifest)?;

    let current = read_remote_manifest(client, &plan.latest_url, &plan.trusted_keys)?;
    let current_sequence = current.as_ref().map(|payload| payload.manifest_sequence);
    if current_sequence != plan.observed_sequence {
        return Err(CliError::new(format!(
            "远端 manifest sequence 在发布期间从 {:?} 变为 {:?}；请重新执行 publish",
            plan.observed_sequence, current_sequence
        )));
    }

    for upload in &plan.latest_dmg_aliases {
        upload_object(client, &plan.target, credentials, upload)?;
    }
    upload_object(client, &plan.target, credentials, &plan.latest)?;
    verify_public_bytes(client, &plan.latest_url, &plan.latest_json, "latest.json")?;
    for verification in &plan.verify_urls {
        verify_public_sha256(client, verification)?;
    }
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
    let endpoint = url::Url::parse(&target.endpoint)
        .map_err(|error| CliError::new(format!("publish endpoint 无效: {error}")))?;
    let public = url::Url::parse(&target.public_base_url)
        .map_err(|error| CliError::new(format!("public_base_url 无效: {error}")))?;
    if (endpoint.scheme() == "http" || public.scheme() == "http") && !target.allow_insecure_http {
        return Err(CliError::new(
            "HTTP endpoint/public_base_url 必须显式设置 allow_insecure_http = true",
        ));
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
    Ok(())
}

fn receipt_matches_configuration(
    receipt: &ReleaseReceipt,
    app_key: &str,
    app: &AppConfig,
    configured: &ResolvedReleaseConfig,
    targets: &[String],
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
        && receipt.targets == targets
        && receipt.runtime_config_source == configured.runtime_config_source
        && receipt.runtime_config_sha256 == configured.runtime_config_sha256
        && receipt.updater_feed == configured.updater_feed
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
    now: chrono::DateTime<Utc>,
    previous_build_number: Option<u64>,
) -> CliResult<u64> {
    let current = now
        .format("%y%m%d%H%M%S")
        .to_string()
        .parse::<u64>()
        .map_err(|error| CliError::new(format!("无法生成 UTC build number: {error}")))?;
    let next = previous_build_number
        .map(|value| {
            value
                .checked_add(1)
                .ok_or_else(|| CliError::new("本地 build number 已达到 u64 上限"))
        })
        .transpose()?;
    Ok(next.map_or(current, |value| current.max(value)))
}

/// 按指定 Unix 秒生成与 build 相同的 UTC 构建号，供集成测试验证时间与单调性规则。
///
/// # Errors
///
/// Unix 秒超出 Chrono 范围，或上一个本地构建号已达到 `u64` 上限时返回错误。
#[allow(dead_code)]
pub fn inspect_build_datetime_number(
    unix_seconds: i64,
    previous_build_number: Option<u64>,
) -> CliResult<u64> {
    let now = Utc
        .timestamp_opt(unix_seconds, 0)
        .single()
        .ok_or_else(|| CliError::new("Unix 秒超出 UTC 时间范围"))?;
    build_datetime_number(now, previous_build_number)
}

/// 校验用户可见的 macOS bundle 名称是否能安全作为文件名。
///
/// # Errors
///
/// 名称为空、包含控制字符、路径分隔符、NUL 或路径穿越语义时返回错误。
pub fn validate_display_name(value: &str) -> CliResult<()> {
    if value.trim().is_empty()
        || matches!(value, "." | "..")
        || value.contains('/')
        || value.contains('\\')
        || value.contains(':')
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(CliError::new(format!(
            "display_name `{value}` 不能安全作为 macOS bundle 文件名"
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
        other => Err(CliError::new(format!(
            "当前只支持 macOS 与 Windows required target，收到 `{other}`"
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
        return Err(CliError::new("当前宿主不能构建 macOS required targets"));
    }
    Ok(())
}

fn ensure_supported_build_host() -> CliResult<()> {
    if matches!(env::consts::OS, "macos" | "windows") {
        Ok(())
    } else {
        Err(CliError::new("当前宿主不能构建桌面 required targets"))
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

pub(super) fn run_doctor(fix: bool) -> CliResult<()> {
    if env::consts::OS == "macos" {
        ensure_macos()?;
        for command in ["cargo", "rustup", "ditto", "plutil", "codesign"] {
            require_command(command)?;
        }
        for (command, install) in [
            ("cargo-bundle", vec!["cargo", "install", "cargo-bundle"]),
            ("create-dmg", vec!["brew", "install", "create-dmg"]),
        ] {
            if command_exists(command) {
                continue;
            }
            if !fix {
                return Err(CliError::new(format!("缺少 `{command}`")));
            }
            let mut process = Command::new(install[0]);
            process.args(&install[1..]);
            run_status(&format!("install {command}"), &mut process)?;
        }
        println!("doctor: macOS 构建依赖可用");
    } else if env::consts::OS == "windows" {
        for command in ["cargo", "rustup", "rc"] {
            require_command(command)?;
        }
        let _ = makensis_command()?;
        println!("doctor: Windows 构建依赖可用");
    } else {
        return Err(CliError::new("当前宿主暂不支持桌面发布构建"));
    }
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

#[derive(Debug)]
struct S3Credentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

impl S3Credentials {
    fn from_env() -> CliResult<Self> {
        let access_key_id = env::var("AWS_ACCESS_KEY_ID")
            .or_else(|_| env::var("RUSTFS_ACCESS_KEY_ID"))
            .map_err(|_| CliError::new("缺少 S3 access key 环境变量"))?;
        let secret_access_key = env::var("AWS_SECRET_ACCESS_KEY")
            .or_else(|_| env::var("RUSTFS_SECRET_ACCESS_KEY"))
            .map_err(|_| CliError::new("缺少 S3 secret key 环境变量"))?;
        Ok(Self {
            access_key_id,
            secret_access_key,
            session_token: env::var("AWS_SESSION_TOKEN").ok(),
        })
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
        ResolvedBuildNumber::BuildDatetime => build_datetime_number(Utc::now(), None)?,
    };
    let release = configured.validated_release(build_number);
    project
        .build_plans(app_key, &release, true)?
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
    let receipt =
        project.prepare_build_receipt(app_key, app, &configured, &app.targets.required)?;
    serde_json::to_value(receipt)
        .map_err(|error| CliError::new(format!("无法序列化 release receipt: {error}")))
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
/// 配置、artifact.json、文件大小、摘要或 required target 完整性不合法时返回错误。
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
/// 配置、artifact.json、文件大小、摘要或 required target 完整性不合法时返回错误。
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

/// 为集成测试返回 publish 将创建的 latest DMG alias key。
///
/// # Errors
///
/// 配置或本地产物预检失败时返回错误。
#[allow(dead_code)]
pub fn inspect_latest_dmg_aliases(
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
    latest_dmg_uploads(&artifacts, &prefix, app.targets.required.len() == 1)
        .map(|uploads| uploads.into_iter().map(|upload| upload.key).collect())
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

/// 为集成测试返回 Windows 安装器源文件快照，不执行 NSIS 或签名命令。
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
        ResolvedBuildNumber::BuildDatetime => build_datetime_number(Utc::now(), None)?,
    };
    let release = configured.validated_release(build_number);
    let brand_assets = project.brand_assets(app_key, app)?;
    let publish_target = &project.config.publish.targets[&app.publish_target];
    let target = app
        .targets
        .required
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
    let technical_stem = format!(
        "{}-{}-{}-{arch}",
        app.package, release.version, release.build_number
    );
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
        app_zip_path: release_dir.join(format!("{technical_stem}.windows.zip")),
        dmg_path: release_dir.join(format!("{technical_stem}.dmg")),
        setup_path: release_dir.join(format!("{technical_stem}.setup.exe")),
        artifact_path: release_dir.join("artifact.json"),
        notes_source: project
            .root
            .join("docs/changelog/components")
            .join(app_key)
            .join("zh-CN.md"),
        notes_path: project
            .root
            .join(DIST_DIRECTORY)
            .join(app_key)
            .join("notes.md"),
    };
    let staging = windows_work_dir(&plan).join("payload");
    let sources = windows_installer_sources(&plan, &staging)?;
    Ok(serde_json::json!({
        "file_version": sources.file_version,
        "nsis_script": sources.nsis_script,
        "updater_config": sources.updater_config,
    }))
}
