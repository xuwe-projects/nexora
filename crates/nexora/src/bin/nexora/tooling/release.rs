//! `nexora build` 与 `nexora publish` 的配置、打包和发布实现。

use super::{CliError, CliResult};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use clap::{Args, Subcommand};
use dialoguer::{Confirm, Select};
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use hmac::{Hmac, Mac as _};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::IsTerminal as _,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const CONFIG_FILE_NAME: &str = "nexora.toml";
const ARTIFACT_SCHEMA_VERSION: u32 = 1;
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const DIST_DIRECTORY: &str = "dist";
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";
const MUTABLE_CACHE: &str = "no-cache";

/// `nexora build` 仅保留稳定 app 选择器。
#[derive(Args, Debug, Clone)]
pub(crate) struct BuildConfig {
    /// `nexora.toml` 中的 app key；单 app 时可省略。
    #[arg(long)]
    app: Option<String>,
}

/// `nexora publish` 的操作型参数。
#[derive(Args, Debug, Clone)]
pub(crate) struct PublishConfig {
    /// `nexora.toml` 中的 app key；单 app 时可省略。
    #[arg(long)]
    app: Option<String>,
    /// 明确发布全部 app。
    #[arg(long, conflicts_with = "app")]
    all: bool,
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
    release: Option<ReleaseConfig>,
    updater: UpdaterConfigFile,
    targets: TargetConfig,
    platforms: PlatformConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseConfig {
    channel: String,
    version: String,
    build_number: u64,
    #[serde(default = "default_minimum_supported_version")]
    minimum_supported_version: String,
    #[serde(default)]
    signing_key_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdaterConfigFile {
    enabled: bool,
    feed_url: String,
    channels: Vec<String>,
    trusted_public_keys: Vec<String>,
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct MacOsConfig {
    signing: SigningMode,
    notarize: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SigningMode {
    DeveloperId,
    AdHoc,
    None,
}

#[derive(Debug)]
struct ProjectDocument {
    root: PathBuf,
    config: ProjectConfig,
}

#[derive(Debug, Clone)]
struct ValidatedRelease {
    channel: String,
    version: Version,
    build_number: u64,
    minimum_supported_version: Version,
}

#[derive(Debug, Clone)]
struct BuildPlan {
    project_root: PathBuf,
    app_key: String,
    package: String,
    app_id: String,
    display_name: String,
    release: ValidatedRelease,
    target: String,
    signing: SigningMode,
    notarize: bool,
    allow_insecure_http: bool,
    updater: UpdaterConfigFile,
    app_path: PathBuf,
    app_zip_path: PathBuf,
    dmg_path: PathBuf,
    artifact_path: PathBuf,
    notes_source: PathBuf,
    notes_path: PathBuf,
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
    ensure_macos()?;
    let project = ProjectDocument::discover()?;
    let app_key = project.select_one(config.app.as_deref(), terminal_is_interactive())?;
    let plans = project.build_plans(&app_key)?;
    if plans.is_empty() {
        return Err(CliError::new(format!(
            "当前宿主不能构建 app `{app_key}` 的任何 required target"
        )));
    }
    validate_cargo_package(&project.root, &plans[0].package)?;
    for plan in &plans {
        execute_build(plan)?;
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
    let app_keys = project.select_many(config.app.as_deref(), config.all, interactive)?;
    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|error| CliError::new(format!("无法创建 HTTP 客户端: {error}")))?;
    let status = if matches!(config.command, Some(PublishCommand::Yank)) {
        ReleaseStatus::Yanked
    } else {
        ReleaseStatus::Available
    };
    let plans = app_keys
        .iter()
        .map(|app_key| project.publish_plan(app_key, status, &client))
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
            if !app.updater.enabled {
                return Err(CliError::new(format!("app `{app_key}` 未启用 updater")));
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
            self.validated_release(app_key, app)?;
            if app.targets.required.is_empty() {
                return Err(CliError::new(format!(
                    "app `{app_key}` 的 targets.required 不能为空"
                )));
            }
            let mut seen = BTreeSet::new();
            for target in &app.targets.required {
                validate_macos_target(target)?;
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
            let target = &self.config.publish.targets[&app.publish_target];
            let expected_feed = public_object_url(
                target,
                &object_key([
                    app.object_prefix.as_str(),
                    app_key,
                    self.validated_release(app_key, app)?.channel.as_str(),
                    "latest.json",
                ]),
            );
            if app.updater.feed_url != expected_feed {
                return Err(CliError::new(format!(
                    "app `{app_key}` 的 updater.feed_url 与发布 latest.json 地址不一致；期望 `{expected_feed}`"
                )));
            }
        }
        Ok(())
    }

    fn validated_release(&self, app_key: &str, app: &AppConfig) -> CliResult<ValidatedRelease> {
        let release = app.release.as_ref().ok_or_else(|| {
            CliError::new(format!("app `{app_key}` 缺少 [apps.{app_key}.release]"))
        })?;
        if !app
            .updater
            .channels
            .iter()
            .any(|channel| channel == &release.channel)
        {
            return Err(CliError::new(format!(
                "app `{app_key}` 的 release.channel `{}` 不属于 updater.channels",
                release.channel
            )));
        }
        let version = Version::parse(&release.version).map_err(|error| {
            CliError::new(format!(
                "app `{app_key}` 的 release.version `{}` 不是合法 SemVer: {error}",
                release.version
            ))
        })?;
        if release.build_number == 0 {
            return Err(CliError::new(format!(
                "app `{app_key}` 的 release.build_number 必须大于 0"
            )));
        }
        let minimum_supported_version = Version::parse(&release.minimum_supported_version)
            .map_err(|error| {
                CliError::new(format!(
                    "app `{app_key}` 的 release.minimum_supported_version `{}` 不是合法 SemVer: {error}",
                    release.minimum_supported_version
                ))
            })?;
        Ok(ValidatedRelease {
            channel: release.channel.clone(),
            version,
            build_number: release.build_number,
            minimum_supported_version,
        })
    }

    fn select_one(&self, app: Option<&str>, interactive: bool) -> CliResult<String> {
        if let Some(app_key) = app {
            if !self.config.apps.contains_key(app_key) {
                return Err(CliError::new(format!("nexora.toml 不存在 app `{app_key}`")));
            }
            return Ok(app_key.to_owned());
        }
        if self.config.apps.len() == 1 {
            return self
                .config
                .apps
                .keys()
                .next()
                .cloned()
                .ok_or_else(|| CliError::new("没有选择 app"));
        }
        if !interactive {
            return Err(CliError::new(
                "nexora.toml 配置了多个 app；非交互 build 必须提供 `--app`",
            ));
        }
        self.select_many(None, false, true)?
            .into_iter()
            .next()
            .ok_or_else(|| CliError::new("没有选择 app"))
    }

    fn select_many(
        &self,
        app: Option<&str>,
        all: bool,
        interactive: bool,
    ) -> CliResult<Vec<String>> {
        if let Some(app_key) = app {
            if !self.config.apps.contains_key(app_key) {
                return Err(CliError::new(format!("nexora.toml 不存在 app `{app_key}`")));
            }
            return Ok(vec![app_key.to_owned()]);
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
        let index = Select::new()
            .with_prompt("请选择 app")
            .items(&labels)
            .default(0)
            .interact()
            .map_err(|error| CliError::new(format!("无法读取 app 选择: {error}")))?;
        Ok(vec![entries[index].0.clone()])
    }

    fn build_plans(&self, app_key: &str) -> CliResult<Vec<BuildPlan>> {
        let app = &self.config.apps[app_key];
        let release = self.validated_release(app_key, app)?;
        let publish_target = &self.config.publish.targets[&app.publish_target];
        app.targets
            .required
            .iter()
            .filter(|target| host_can_build(target))
            .map(|target| {
                let arch = target_arch_alias(target)?;
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
                    display_name: app.display_name.clone(),
                    release: release.clone(),
                    target: target.clone(),
                    signing: app.platforms.macos.signing,
                    notarize: app.platforms.macos.notarize,
                    allow_insecure_http: publish_target.allow_insecure_http,
                    updater: app.updater.clone(),
                    app_path: self
                        .root
                        .join("target")
                        .join(target)
                        .join("release/bundle/osx")
                        .join(format!("{}.app", app.package)),
                    app_zip_path: release_dir.join(format!("{technical_stem}.app.zip")),
                    dmg_path: release_dir.join(format!("{technical_stem}.dmg")),
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
        status: ReleaseStatus,
        client: &reqwest::blocking::Client,
    ) -> CliResult<PublishPlan> {
        let app = &self.config.apps[app_key];
        let release = self.validated_release(app_key, app)?;
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
            && (payload.app_id != app.app_id || payload.channel != release.channel)
        {
            return Err(CliError::new(format!(
                "远端 latest.json 的 app_id/channel 与 app `{app_key}` 配置不一致"
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
            for kind in [ArtifactKind::MacosAppZip, ArtifactKind::MacosDmg] {
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
                    if kind == ArtifactKind::MacosAppZip {
                        manifest_artifacts.push(ManifestArtifact {
                            target: artifact.target.clone(),
                            url: url.clone(),
                            sha256: artifact.sha256.clone(),
                            size: artifact.size,
                            kind: "macos_app_zip".to_owned(),
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
                app_id: app.app_id.clone(),
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
            app_id: app.app_id.clone(),
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
    write_bundle_updater_config(plan)?;
    build_and_install_sidecar(plan, &executable)?;
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
    let config = BundledUpdaterConfig {
        schema_version: 1,
        app_id: plan.app_id.clone(),
        channel: plan.release.channel.clone(),
        feed_url: plan.updater.feed_url.clone(),
        trusted_public_keys: plan.updater.trusted_public_keys.clone(),
        current_version: plan.release.version.to_string(),
        current_build_number: plan.release.build_number,
        allow_insecure_http: plan.allow_insecure_http,
        health_timeout: plan.updater.health_timeout.clone(),
    };
    let contents = serde_json::to_vec_pretty(&config)
        .map_err(|error| CliError::new(format!("无法生成 updater bundle 配置: {error}")))?;
    fs::write(resources.join("nexora-updater.json"), contents)
        .map_err(|error| CliError::new(format!("无法写入 updater bundle 配置: {error}")))
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
    let artifacts = [
        (ArtifactKind::MacosAppZip, &plan.app_zip_path),
        (ArtifactKind::MacosDmg, &plan.dmg_path),
    ]
    .into_iter()
    .map(|(kind, path)| artifact_entry(kind, path))
    .collect::<CliResult<Vec<_>>>()?;
    let manifest = ArtifactManifest {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        app_id: plan.app_id.clone(),
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
        for required in [ArtifactKind::MacosAppZip, ArtifactKind::MacosDmg] {
            if !kinds.contains(&required) {
                return Err(CliError::new(format!(
                    "`{}` 缺少 {}",
                    path.display(),
                    artifact_kind_name(required)
                )));
            }
        }
        if manifest.artifacts.len() != 2 {
            return Err(CliError::new(format!(
                "`{}` 必须且只能描述 macos_app_zip 与 macos_dmg 两个产物",
                path.display()
            )));
        }
        for artifact in manifest.artifacts {
            validate_file_name(&artifact.file_name)?;
            let expected_suffix = match artifact.kind {
                ArtifactKind::MacosAppZip => ".app.zip",
                ArtifactKind::MacosDmg => ".dmg",
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
    if single_target {
        let dmg = dmgs
            .first()
            .ok_or_else(|| CliError::new("单 target 发布缺少 DMG"))?;
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
    println!("版本：{}", plan.release.version);
    println!("Build：{}", plan.release.build_number);
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

fn validate_cargo_package(root: &Path, package: &str) -> CliResult<()> {
    let output = Command::new("cargo")
        .current_dir(root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|error| CliError::new(format!("无法执行 cargo metadata: {error}")))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "cargo metadata 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| CliError::new(format!("无法解析 cargo metadata: {error}")))?;
    let exists = metadata["packages"]
        .as_array()
        .is_some_and(|packages| packages.iter().any(|item| item["name"] == package));
    if !exists {
        return Err(CliError::new(format!(
            "Cargo workspace 中不存在 package `{package}`"
        )));
    }
    Ok(())
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

fn validate_macos_target(target: &str) -> CliResult<()> {
    target_arch_alias(target).map(|_| ())
}

fn target_arch_alias(target: &str) -> CliResult<&'static str> {
    match target {
        "aarch64-apple-darwin" => Ok("aarch64"),
        "x86_64-apple-darwin" => Ok("x86_64"),
        other => Err(CliError::new(format!(
            "当前只支持 macOS required target，收到 `{other}`"
        ))),
    }
}

fn host_can_build(target: &str) -> bool {
    env::consts::OS == "macos" && target.ends_with("-apple-darwin")
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
    }
}

fn artifact_content_type(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::MacosAppZip => "application/zip",
        ArtifactKind::MacosDmg => "application/x-apple-diskimage",
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

fn default_minimum_supported_version() -> String {
    "0.0.0".to_owned()
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

pub(super) fn run_doctor(fix: bool) -> CliResult<()> {
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
    Command::new("sh")
        .args(["-c", "command -v -- \"$1\" >/dev/null 2>&1", "sh", command])
        .status()
        .is_ok_and(|status| status.success())
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
    project
        .build_plans(app_key)?
        .into_iter()
        .map(|plan| {
            Ok(serde_json::json!({
                "app_key": plan.app_key,
                "package": plan.package,
                "display_name": plan.display_name,
                "app_path": plan.app_path,
                "app_zip_path": plan.app_zip_path,
                "dmg_path": plan.dmg_path,
                "artifact_path": plan.artifact_path,
                "target": plan.target,
                "version": plan.release.version,
                "build_number": plan.release.build_number,
                "channel": plan.release.channel,
                "signing": format!("{:?}", plan.signing),
                "notarize": plan.notarize,
            }))
        })
        .collect()
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
    ProjectDocument::load(config_path.as_ref().to_path_buf())?.select_many(app, all, false)
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
    let release = project.validated_release(app_key, app)?;
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
    let release = project.validated_release(app_key, app)?;
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
