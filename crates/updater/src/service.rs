//! 更新工作线程、事件流与公共配置。

use std::{
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_channel::{Receiver, Sender};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
#[cfg(not(target_os = "windows"))]
use directories::ProjectDirs;
use reqwest::{Url, blocking::Client};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

use crate::{
    SignedUpdateManifest, TrustedPublicKey, UpdateChannel, UpdateRelease, UpdateTarget, macos,
};

const MAX_APP_ID_BYTES: usize = 255;
const STALE_STAGING_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const BUNDLED_CONFIG_SCHEMA_VERSION: u32 = 1;
const BUNDLED_CONFIG_FILE_NAME: &str = "nexora-updater.json";
const PENDING_RECORD_SCHEMA_VERSION: u32 = 1;
const PENDING_RECORD_FILE_NAME: &str = "pending.json";
const INSTALLING_RECORD_FILE_NAME: &str = "installing.json";
const INSTALL_RESULT_SCHEMA_VERSION: u32 = 1;
const INSTALL_RESULT_FILE_NAME: &str = "last-install-result.json";
const MAX_INSTALL_RESULT_BYTES: u64 = 16 * 1024;

#[derive(Debug, Deserialize)]
struct BundledUpdateConfig {
    schema_version: u32,
    app_id: String,
    channel: UpdateChannel,
    feed_url: String,
    trusted_public_keys: Vec<String>,
    current_version: String,
    current_build_number: u64,
    allow_insecure_http: bool,
    health_timeout: String,
    #[serde(default)]
    expected_team_id: Option<String>,
    #[serde(default)]
    expected_windows_signer_thumbprint: Option<String>,
    #[serde(default)]
    expected_windows_publisher: Option<String>,
    #[serde(default)]
    check_on_launch: bool,
}

fn current_bundled_config_location() -> Result<(PathBuf, PathBuf), UpdateError> {
    #[cfg(target_os = "macos")]
    {
        let app_bundle = macos::current_app_bundle()?;
        let config_path = app_bundle
            .join("Contents/Resources")
            .join(BUNDLED_CONFIG_FILE_NAME);
        Ok((app_bundle, config_path))
    }
    #[cfg(target_os = "windows")]
    {
        let install_dir = crate::windows::current_install_dir()?;
        let config_path = install_dir.join(BUNDLED_CONFIG_FILE_NAME);
        Ok((install_dir, config_path))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(UpdateError::UnsupportedPlatform)
    }
}

/// Windows Authenticode 签名验证所需的发布者约束。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsSignatureConfig {
    /// 期望的签名证书 SHA-1 thumbprint，使用十六进制字符串表示。
    pub signer_thumbprint: String,
    /// 期望的签名证书发布者名称，用于避免仅校验证书链而信任错误主体。
    pub publisher: String,
}

/// 启动一次更新检查所需的应用配置。
///
/// 每个桌面应用可以在初始化阶段创建自己的配置，并把 [`Updater`] 交给 UI 层使用。
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    manifest_url: Url,
    app_id: String,
    current_version: Version,
    current_build_number: u64,
    channel: UpdateChannel,
    trusted_public_keys: Vec<TrustedPublicKey>,
    highest_manifest_sequence: u64,
    expected_team_id: Option<String>,
    windows_signature: Option<WindowsSignatureConfig>,
    request_timeout: Duration,
    health_timeout: Duration,
    app_bundle_path: Option<PathBuf>,
    sidecar_path: Option<PathBuf>,
    cache_dir_override: Option<PathBuf>,
    check_on_launch: bool,
    health_report_on_launch: bool,
}

impl UpdateConfig {
    /// 从当前平台安装目录加载构建时写入的更新配置。
    ///
    /// macOS 从 `.app/Contents/Resources/nexora-updater.json` 读取，Windows 从主 EXE
    /// 同级的 `nexora-updater.json` 读取。签名私钥与对象存储凭据不会出现在该文件中。
    ///
    /// # Errors
    ///
    /// 当前平台不支持、无法确定安装目录、配置文件缺失或格式无效、配置协议版本不受支持、
    /// 更新地址不符合传输安全策略，或可信公钥无效时返回错误。
    pub fn from_current_bundle() -> Result<Self, UpdateError> {
        let (installation_path, config_path) = current_bundled_config_location()?;
        Self::from_bundled_config_path(&config_path, installation_path)
    }

    /// 当当前安装包已启用 updater 时加载配置。
    ///
    /// 构建未启用 updater 且安装目录中没有 `nexora-updater.json` 时返回 `Ok(None)`；
    /// 配置存在但无效时仍返回错误，不会静默降级为无更新模式。
    ///
    /// # Errors
    ///
    /// 当前平台不支持、读取配置失败，或已存在的配置不满足更新安全约束时返回错误。
    pub fn from_current_bundle_if_present() -> Result<Option<Self>, UpdateError> {
        let location = current_bundled_config_location();
        let (installation_path, config_path) = match location {
            Ok(location) => location,
            Err(UpdateError::AppBundleNotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        match fs::metadata(&config_path) {
            Ok(_) => Self::from_bundled_config_path(&config_path, installation_path).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// 从指定 macOS `.app` 加载构建时写入的更新配置。
    ///
    /// 该入口主要用于框架集成测试和显式应用启动器；正常桌面应用应使用
    /// [`Self::from_current_bundle`]，以免把错误的安装目标交给 sidecar。
    ///
    /// # Errors
    ///
    /// bundle 配置缺失、JSON 无效、字段不满足 updater 约束或健康确认超时格式无效时返回错误。
    pub fn from_app_bundle(app_bundle: impl AsRef<Path>) -> Result<Self, UpdateError> {
        let app_bundle = app_bundle.as_ref();
        let config_path = app_bundle
            .join("Contents/Resources")
            .join(BUNDLED_CONFIG_FILE_NAME);
        Self::from_bundled_config_path(&config_path, app_bundle)
    }

    /// 从指定 Windows 安装目录加载构建时写入的更新配置。
    ///
    /// 该目录应同时包含主 EXE、updater sidecar 和 `nexora-updater.json`。正常安装后启动
    /// 使用 [`Self::from_current_bundle`]；本方法主要用于显式启动器和隔离测试。
    ///
    /// # Errors
    ///
    /// 配置文件缺失或格式无效、配置协议版本不受支持、更新地址不符合传输安全策略，
    /// 或可信公钥无效时返回错误。
    pub fn from_windows_install_dir(install_dir: impl AsRef<Path>) -> Result<Self, UpdateError> {
        let install_dir = install_dir.as_ref();
        Self::from_bundled_config_path(&install_dir.join(BUNDLED_CONFIG_FILE_NAME), install_dir)
    }

    fn from_bundled_config_path(
        config_path: &Path,
        installation_path: impl AsRef<Path>,
    ) -> Result<Self, UpdateError> {
        let contents = fs::read_to_string(config_path)?;
        let bundled: BundledUpdateConfig = serde_json::from_str(&contents).map_err(|error| {
            UpdateError::InvalidBundleConfig(format!("{}: {error}", config_path.display()))
        })?;
        if bundled.schema_version != BUNDLED_CONFIG_SCHEMA_VERSION {
            return Err(UpdateError::InvalidBundleConfig(format!(
                "不支持 bundle updater 配置版本 {}",
                bundled.schema_version
            )));
        }
        if bundled.trusted_public_keys.is_empty() {
            return Err(UpdateError::MissingTrustedPublicKeys);
        }
        let health_timeout = parse_bundled_duration(&bundled.health_timeout)?;
        Self::with_transport_policy(
            &bundled.feed_url,
            bundled.app_id,
            &bundled.current_version,
            bundled.current_build_number,
            bundled.channel,
            bundled.allow_insecure_http,
        )?
        .with_trusted_public_keys(&bundled.trusted_public_keys)
        .and_then(|config| {
            let config = config
                .with_health_timeout(health_timeout)
                .with_app_bundle_path(installation_path.as_ref())
                .with_check_on_launch(bundled.check_on_launch);
            let config = if let Some(team_id) = bundled.expected_team_id {
                config.with_expected_team_id(team_id)
            } else {
                config
            };
            config.with_optional_windows_signature(
                bundled.expected_windows_signer_thumbprint,
                bundled.expected_windows_publisher,
            )
        })
    }

    /// 创建应用更新配置。
    ///
    /// `manifest_url` 指向当前通道的 `latest.json`；`app_id` 必须和清单一致，并且是由
    /// ASCII 字母、数字、点、连字符或下划线组成的安全路径分量；
    /// `current_version` 使用 SemVer；`current_build_number` 是当前安装包构建号。
    ///
    /// # Errors
    ///
    /// 当清单地址不是有效 URL、应用标识不安全、HTTP 未显式允许，或当前版本不是有效 SemVer
    /// 时返回错误。
    pub fn new(
        manifest_url: impl AsRef<str>,
        app_id: impl Into<String>,
        current_version: impl AsRef<str>,
        current_build_number: u64,
        channel: UpdateChannel,
    ) -> Result<Self, UpdateError> {
        Self::with_transport_policy(
            manifest_url,
            app_id,
            current_version,
            current_build_number,
            channel,
            false,
        )
    }

    /// 创建允许显式 HTTP 策略的应用更新配置。
    ///
    /// `allow_insecure_http` 只应来自构建配置或管理员覆盖；未显式允许时仅接受 HTTPS 和
    /// loopback HTTP。
    ///
    /// # Errors
    ///
    /// 当 URL、应用标识、版本号或传输安全策略无效时返回错误。
    pub fn with_transport_policy(
        manifest_url: impl AsRef<str>,
        app_id: impl Into<String>,
        current_version: impl AsRef<str>,
        current_build_number: u64,
        channel: UpdateChannel,
        allow_insecure_http: bool,
    ) -> Result<Self, UpdateError> {
        let app_id = app_id.into();
        if !valid_app_id(&app_id) {
            return Err(UpdateError::InvalidAppId);
        }
        let manifest_url = Url::parse(manifest_url.as_ref())
            .map_err(|error| UpdateError::InvalidUrl(error.to_string()))?;
        validate_transport(&manifest_url, allow_insecure_http)?;

        Ok(Self {
            manifest_url,
            app_id,
            current_version: Version::parse(current_version.as_ref())?,
            current_build_number,
            channel,
            trusted_public_keys: Vec::new(),
            highest_manifest_sequence: 0,
            expected_team_id: None,
            windows_signature: None,
            request_timeout: Duration::from_secs(30),
            health_timeout: Duration::from_secs(120),
            app_bundle_path: None,
            sidecar_path: None,
            cache_dir_override: None,
            check_on_launch: false,
            health_report_on_launch: true,
        })
    }

    /// 设置客户端信任的 Ed25519 公钥列表。
    ///
    /// # Errors
    ///
    /// 任一 `key_id:ed25519:BASE64_PUBLIC_KEY` 字符串格式无效时返回错误。
    pub fn with_trusted_public_keys<I, S>(mut self, keys: I) -> Result<Self, UpdateError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.trusted_public_keys = keys
            .into_iter()
            .map(|key| TrustedPublicKey::parse(key.as_ref()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self)
    }

    /// 设置客户端已经接受过的最高清单序号，用于拒绝重放旧清单。
    pub fn with_highest_manifest_sequence(mut self, sequence: u64) -> Self {
        self.highest_manifest_sequence = sequence;
        self
    }

    /// 要求下载后的 `.app` 必须由指定 Apple Team ID 签名。
    ///
    /// 未设置时仍会执行 `codesign --verify --deep --strict`，但不会限制具体签名团队。
    pub fn with_expected_team_id(mut self, team_id: impl Into<String>) -> Self {
        self.expected_team_id = Some(team_id.into());
        self
    }

    /// 设置 Windows Authenticode 签名验证要求。
    ///
    /// `signer_thumbprint` 必须是签名证书的 SHA-1 指纹；`publisher` 必须与证书主体发布者一致。
    /// Windows 更新 ZIP staging 时会使用这些值约束主 EXE 和 updater EXE 的签名身份。
    ///
    /// # Errors
    ///
    /// 当 thumbprint 不是 40 位十六进制 SHA-1 指纹，或 publisher 为空时返回错误。
    pub fn with_windows_signature(
        mut self,
        signer_thumbprint: impl AsRef<str>,
        publisher: impl Into<String>,
    ) -> Result<Self, UpdateError> {
        let publisher = publisher.into();
        if publisher.trim().is_empty() {
            return Err(UpdateError::InvalidBundleConfig(
                "expected_windows_publisher 不能为空".to_owned(),
            ));
        }
        self.windows_signature = Some(WindowsSignatureConfig {
            signer_thumbprint: normalize_windows_thumbprint(signer_thumbprint.as_ref())?,
            publisher,
        });
        Ok(self)
    }

    fn with_optional_windows_signature(
        self,
        signer_thumbprint: Option<String>,
        publisher: Option<String>,
    ) -> Result<Self, UpdateError> {
        match (signer_thumbprint, publisher) {
            (Some(thumbprint), Some(publisher)) => {
                self.with_windows_signature(thumbprint, publisher)
            }
            (None, None) => Ok(self),
            _ => Err(UpdateError::InvalidBundleConfig(
                "expected_windows_signer_thumbprint 与 expected_windows_publisher 必须同时配置"
                    .to_owned(),
            )),
        }
    }

    /// 设置应用启动后是否在后台静默检查一次更新。
    ///
    /// 该开关只控制检查，不会自动下载；发现新版本后仍需由用户确认。
    pub const fn with_check_on_launch(mut self, enabled: bool) -> Self {
        self.check_on_launch = enabled;
        self
    }

    /// 设置检查清单和下载安装包时使用的单次请求超时。
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// 设置 sidecar 等待新版本报告健康的超时时间。
    ///
    /// sidecar 在替换应用并启动新版本后，会等待同一次会话的健康确认文件；超时会触发回滚。
    pub fn with_health_timeout(mut self, timeout: Duration) -> Self {
        self.health_timeout = timeout;
        self
    }

    /// 显式指定当前运行中的应用安装路径。
    ///
    /// macOS 传入 `.app` 路径，Windows 传入主 EXE 所在目录。正常发布环境无需设置；
    /// 该选项主要用于集成测试和非标准应用启动器。
    pub fn with_app_bundle_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.app_bundle_path = Some(path.into());
        self
    }

    /// 显式指定独立 updater sidecar 可执行文件路径。
    ///
    /// 未设置时，macOS 会按当前 `.app/Contents/Helpers/<主程序名>-updater` 推导。生产应用
    /// 应在构建时把 sidecar 放入该位置，测试或 example 可通过本方法传入临时 sidecar。
    pub fn with_sidecar_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.sidecar_path = Some(path.into());
        self
    }

    /// 覆盖 updater 使用的应用级缓存目录。
    ///
    /// 正常应用不应设置该值；该入口用于让集成测试在隔离目录中验证暂存、恢复与清理行为。
    pub fn with_cache_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_dir_override = Some(path.into());
        self
    }

    /// 返回更新清单 URL。
    pub fn manifest_url(&self) -> &Url {
        &self.manifest_url
    }

    /// 返回应用稳定标识。
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// 返回当前语义化版本。
    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    /// 返回当前构建号。
    pub const fn current_build_number(&self) -> u64 {
        self.current_build_number
    }

    /// 返回当前更新通道。
    pub const fn channel(&self) -> UpdateChannel {
        self.channel
    }

    /// 返回应用启动后是否应后台检查更新。
    pub const fn check_on_launch(&self) -> bool {
        self.check_on_launch
    }

    /// 设置主窗口创建后是否报告一次 sidecar 健康状态。
    ///
    /// 生产应用应保持默认值 `true`；该开关仅用于 updater 集成样例和故障回滚测试。
    pub const fn with_health_report_on_launch(mut self, enabled: bool) -> Self {
        self.health_report_on_launch = enabled;
        self
    }

    /// 返回主窗口创建后是否应报告 sidecar 健康状态。
    pub const fn health_report_on_launch(&self) -> bool {
        self.health_report_on_launch
    }

    /// 返回客户端信任的清单签名公钥。
    pub fn trusted_public_keys(&self) -> &[TrustedPublicKey] {
        &self.trusted_public_keys
    }

    /// 返回客户端已经接受过的最高清单序号。
    pub const fn highest_manifest_sequence(&self) -> u64 {
        self.highest_manifest_sequence
    }

    /// 返回 macOS 更新包必须匹配的预期 Apple Team ID。
    ///
    /// `None` 表示仍执行系统签名校验，但不额外限制具体开发团队。
    pub fn expected_team_id(&self) -> Option<&str> {
        self.expected_team_id.as_deref()
    }

    /// 返回 Windows 更新包必须匹配的 Authenticode 签名身份。
    pub fn windows_signature(&self) -> Option<&WindowsSignatureConfig> {
        self.windows_signature.as_ref()
    }

    pub(crate) fn sidecar_path(&self) -> Result<PathBuf, UpdateError> {
        match &self.sidecar_path {
            Some(path) => Ok(path.clone()),
            None => default_sidecar_path(),
        }
    }

    pub(crate) const fn health_timeout(&self) -> Duration {
        self.health_timeout
    }

    fn cache_dir(&self) -> Result<PathBuf, UpdateError> {
        if let Some(path) = &self.cache_dir_override {
            return Ok(path.clone());
        }

        #[cfg(target_os = "windows")]
        {
            let install_dir = self
                .app_bundle_path
                .clone()
                .map(Ok)
                .unwrap_or_else(crate::windows::current_install_dir)?;
            crate::windows::cache_dir_for_install(&install_dir, &self.app_id)
        }

        #[cfg(not(target_os = "windows"))]
        {
            ProjectDirs::from("", "", &self.app_id)
                .map(|directories| directories.cache_dir().join("updater"))
                .ok_or(UpdateError::CacheDirectoryUnavailable)
        }
    }
}

/// 可在线程和 UI 之间共享的更新取消令牌。
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// 请求取消当前更新操作。
    ///
    /// 网络读取会在下一次数据块处理时停止，已完成的暂存目录也会由 updater 清理。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// 返回调用方是否已经请求取消操作。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn ensure_active(&self) -> Result<(), UpdateError> {
        if self.is_cancelled() {
            return Err(UpdateError::Cancelled);
        }

        Ok(())
    }
}

/// 更新工作线程发送给桌面 UI 的状态事件。
#[derive(Debug, Clone)]
pub enum UpdateEvent {
    /// 正在下载并解析 `latest.json`。
    Checking,
    /// 服务端版本不高于当前 `(version, build_number)`。
    UpToDate,
    /// 已发现新版本；仅检查会话会在这里结束，完整会话随后开始下载安装包。
    UpdateAvailable(
        /// 服务端清单中已通过应用、通道、版本和目标平台校验的版本信息。
        UpdateRelease,
    ),
    /// 正在下载安装包，并携带已下载和总字节数。
    Downloading {
        /// 当前已经写入临时文件的字节数。
        downloaded: u64,
        /// 安装包总字节数；服务端未提供时为 `None`。
        total: Option<u64>,
    },
    /// 安装包下载完成，正在校验摘要和 macOS 代码签名。
    Verifying,
    /// 校验完成，正在解压并准备退出后替换应用。
    Staging,
    /// 更新已经暂存完成，可以由用户确认立即重启。
    ReadyToRestart(
        /// 已完成下载、校验和解压，可用于启动安装 helper 的暂存更新。
        StagedUpdate,
    ),
    /// 更新流程失败；消息可以直接展示给用户。
    Failed(
        /// 已转换为中文上下文的用户可见错误消息。
        String,
    ),
    /// 用户主动取消了更新。
    Cancelled,
}

/// 一次后台更新任务的事件接收端和取消入口。
#[derive(Debug)]
pub struct UpdateSession {
    events: Receiver<UpdateEvent>,
    cancellation: CancellationToken,
}

impl UpdateSession {
    /// 返回事件接收器的克隆句柄，供 GPUI 异步任务持续等待状态变化。
    pub fn events(&self) -> Receiver<UpdateEvent> {
        self.events.clone()
    }

    /// 返回当前更新任务的取消令牌。
    pub fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

/// 已完成下载、摘要校验、解压和代码签名验证的 macOS 更新。
#[derive(Debug, Clone)]
pub struct StagedUpdate {
    release: UpdateRelease,
    app_id: String,
    staged_app: PathBuf,
    current_app: PathBuf,
    sidecar_path: PathBuf,
    health_timeout: Duration,
    pending_record: Option<PathBuf>,
    cleanup: Arc<StagingCleanup>,
}

#[derive(Debug)]
struct StagingCleanup {
    staging_root: PathBuf,
    installer_started: AtomicBool,
    retained: AtomicBool,
    cleanup_sender: Option<mpsc::Sender<PathBuf>>,
}

struct InstallHelperRequest<'a> {
    process_id: u32,
    app_id: &'a str,
    current_app: &'a Path,
    staged_app: &'a Path,
    staging_root: &'a Path,
    sidecar_path: &'a Path,
    health_timeout: Duration,
    pending_records: Option<(&'a Path, &'a Path)>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PendingUpdateRecord {
    schema_version: u32,
    app_id: String,
    channel: UpdateChannel,
    version: Version,
    build_number: u64,
    target: String,
    manifest: SignedUpdateManifest,
    staging_root: PathBuf,
    archive_path: PathBuf,
    staged_app: PathBuf,
}

#[derive(Debug, Deserialize)]
struct InstallResultRecord {
    schema_version: u32,
    app_id: String,
    message: String,
}

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if self.installer_started.load(Ordering::Acquire) || self.retained.load(Ordering::Acquire) {
            return;
        }

        let Some(sender) = &self.cleanup_sender else {
            tracing::warn!(
                path = %self.staging_root.display(),
                "更新暂存目录清理线程不可用，将由后续启动回收"
            );
            return;
        };
        if sender.send(self.staging_root.clone()).is_err() {
            tracing::warn!(
                path = %self.staging_root.display(),
                "无法提交更新暂存目录清理任务，将由后续启动回收"
            );
        }
    }
}

impl StagedUpdate {
    /// 返回等待安装的版本信息。
    pub fn release(&self) -> &UpdateRelease {
        &self.release
    }

    /// 原子保留当前已验证更新，供用户下次手动启动应用时直接安装。
    ///
    /// 该操作把普通暂存目录移动到应用级用户缓存的 `pending` 区域，并原子替换待安装记录。
    /// 成功后，即使 [`StagedUpdate`] 或应用进程被释放，缓存也不会由普通暂存清理逻辑删除。
    ///
    /// # Errors
    ///
    /// 缓存目录不可用、暂存路径越界、签名清单缺失，或原子移动和记录写入失败时返回错误。
    pub fn preserve_for_next_launch(&mut self) -> Result<(), UpdateError> {
        if self.pending_record.is_some() {
            return Ok(());
        }

        let cache_dir = self
            .cleanup
            .staging_root
            .parent()
            .and_then(Path::parent)
            .ok_or(UpdateError::InvalidPendingPath)?
            .to_path_buf();
        let staging_base = cache_dir.join("staging");
        ensure_path_within(&self.cleanup.staging_root, &staging_base)?;

        let pending_base = cache_dir.join("pending");
        fs::create_dir_all(&pending_base)?;
        let candidate = pending_base.join(pending_candidate_name(&self.release)?);
        let staged_app =
            remap_staging_path(&self.staged_app, &self.cleanup.staging_root, &candidate)?;
        let archive_path = candidate.join(artifact_archive_file_name(&self.release.artifact.kind)?);
        let verified_manifest = self.release.verified_manifest()?.clone();
        let record = PendingUpdateRecord {
            schema_version: PENDING_RECORD_SCHEMA_VERSION,
            app_id: self.app_id.clone(),
            channel: verified_manifest.payload.channel,
            version: self.release.version.clone(),
            build_number: self.release.build_number,
            target: self.release.artifact.target.clone(),
            manifest: verified_manifest,
            staging_root: relative_to_cache(&candidate, &cache_dir)?,
            archive_path: relative_to_cache(&archive_path, &cache_dir)?,
            staged_app: relative_to_cache(&staged_app, &cache_dir)?,
        };
        fs::rename(&self.cleanup.staging_root, &candidate)?;
        let pending_record = cache_dir.join(PENDING_RECORD_FILE_NAME);
        if let Err(error) = write_pending_record(&cache_dir, &pending_record, &record) {
            if fs::rename(&candidate, &self.cleanup.staging_root).is_err() {
                self.cleanup.retained.store(true, Ordering::Release);
            }
            return Err(error);
        }

        self.staged_app = staged_app;
        self.pending_record = Some(pending_record);
        self.cleanup.retained.store(true, Ordering::Release);
        cleanup_other_pending_roots(&pending_base, &candidate);
        Ok(())
    }

    /// 启动退出后安装 helper。
    ///
    /// helper 会等待当前进程退出，把暂存 `.app` 替换到原安装位置，然后重新打开应用。
    /// 调用成功后，应用必须立即结束当前 GPUI 进程。
    ///
    /// # Errors
    ///
    /// 当 helper 已经启动、helper 文件无法创建或子进程无法启动时返回错误。
    pub fn prepare_restart(&self) -> Result<(), UpdateError> {
        self.cleanup
            .installer_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| UpdateError::InstallerAlreadyStarted)?;
        let claimed_record = match self.claim_pending_record() {
            Ok(record) => record,
            Err(error) => {
                self.cleanup
                    .installer_started
                    .store(false, Ordering::Release);
                return Err(error);
            }
        };
        let result = spawn_install_helper(InstallHelperRequest {
            process_id: std::process::id(),
            app_id: &self.app_id,
            current_app: &self.current_app,
            staged_app: &self.staged_app,
            staging_root: &self.cleanup.staging_root,
            sidecar_path: &self.sidecar_path,
            health_timeout: self.health_timeout,
            pending_records: claimed_record
                .as_ref()
                .map(|(pending, installing)| (pending.as_path(), installing.as_path())),
        });
        if result.is_err() {
            if let Some((pending, installing)) = claimed_record {
                _ = fs::rename(installing, pending);
            }
            self.cleanup
                .installer_started
                .store(false, Ordering::Release);
        }
        result
    }

    fn claim_pending_record(&self) -> Result<Option<(PathBuf, PathBuf)>, UpdateError> {
        let Some(pending) = &self.pending_record else {
            return Ok(None);
        };
        let installing = pending.with_file_name(INSTALLING_RECORD_FILE_NAME);
        fs::rename(pending, &installing).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                UpdateError::InstallerAlreadyStarted
            } else {
                UpdateError::Io(error)
            }
        })?;
        Ok(Some((pending.clone(), installing)))
    }
}

/// 与 UI 框架无关的桌面应用更新器。
#[derive(Debug, Clone)]
pub struct Updater {
    config: UpdateConfig,
}

impl Updater {
    /// 使用指定应用配置创建更新器。
    pub fn new(config: UpdateConfig) -> Self {
        Self { config }
    }

    /// 从应用级缓存恢复一份待安装更新。
    ///
    /// 恢复会重新验证签名信封、应用和通道、目标平台、版本与构建号、缓存路径边界、归档
    /// 大小和 SHA-256，以及 macOS `.app` 代码签名。无记录时返回 `Ok(None)`；记录无效时会
    /// 安全清理专用待安装缓存并同样返回 `Ok(None)`，使调用方可以继续正常网络检查。
    ///
    /// # Errors
    ///
    /// 应用级缓存目录无法确定或无法读取时返回错误。无效待安装内容的具体验证错误会记录到
    /// 日志并在清理后降级为 `Ok(None)`。
    pub fn restore_pending(&self) -> Result<Option<StagedUpdate>, UpdateError> {
        cleanup_stale_staging_roots(&self.config);
        let cache_dir = self.config.cache_dir()?;
        let record_path = cache_dir.join(PENDING_RECORD_FILE_NAME);
        if !record_path.exists() {
            if !cache_dir.join(INSTALLING_RECORD_FILE_NAME).exists() {
                _ = fs::remove_dir_all(cache_dir.join("pending"));
            }
            return Ok(None);
        }

        match restore_pending_inner(&self.config, &cache_dir, &record_path) {
            Ok(staged) => {
                cleanup_other_pending_roots(
                    &cache_dir.join("pending"),
                    &staged.cleanup.staging_root,
                );
                Ok(Some(staged))
            }
            Err(error) => {
                tracing::warn!(error = %error, "待安装更新无效，已放弃恢复");
                discard_pending_cache(&cache_dir);
                Ok(None)
            }
        }
    }

    pub(crate) fn take_install_failure(&self) -> Result<Option<String>, UpdateError> {
        if !cfg!(target_os = "windows") {
            return Ok(None);
        }

        let result_path = self.config.cache_dir()?.join(INSTALL_RESULT_FILE_NAME);
        let metadata = match fs::metadata(&result_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let contents = if metadata.len() <= MAX_INSTALL_RESULT_BYTES {
            fs::read(&result_path)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "安装结果记录过大",
            ))
        };
        let remove_result = fs::remove_file(&result_path);
        if let Err(error) = remove_result
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %result_path.display(), error = %error, "无法删除已消费的安装结果记录");
        }

        let record = match contents.map_err(UpdateError::Io).and_then(|contents| {
            serde_json::from_slice::<InstallResultRecord>(&contents)
                .map_err(UpdateError::InvalidPendingRecord)
        }) {
            Ok(record) => record,
            Err(error) => {
                tracing::warn!(error = %error, "忽略无效的 Windows 安装结果记录");
                return Ok(None);
            }
        };
        if record.schema_version != INSTALL_RESULT_SCHEMA_VERSION
            || record.app_id != self.config.app_id
            || record.message.trim().is_empty()
        {
            tracing::warn!("忽略与当前应用不匹配的 Windows 安装结果记录");
            return Ok(None);
        }
        Ok(Some(record.message))
    }

    /// 在独立工作线程中检查、下载、验证并暂存更新。
    ///
    /// 返回的 [`UpdateSession`] 可直接由 GPUI Entity 持有；关闭弹窗或销毁 Entity 时调用
    /// [`CancellationToken::cancel`] 即可停止后续工作。
    ///
    /// # Errors
    ///
    /// 当操作系统无法创建 updater 工作线程时返回 [`UpdateError::Io`]。
    pub fn start(&self) -> Result<UpdateSession, UpdateError> {
        self.spawn_worker("nexora-updater", run_update)
    }

    /// 在独立工作线程中只检查当前通道是否存在可安装更新。
    ///
    /// 返回的会话只会读取并验证 `latest.json`，发送 [`UpdateEvent::UpToDate`] 或
    /// [`UpdateEvent::UpdateAvailable`] 后结束，不会下载或暂存安装包。桌面 UI 应先使用该
    /// 方法取得用户确认，再把收到的 [`UpdateRelease`] 交给 [`Self::download`]。
    ///
    /// # Errors
    ///
    /// 当操作系统无法创建 updater 检查线程时返回 [`UpdateError::Io`]。
    pub fn check(&self) -> Result<UpdateSession, UpdateError> {
        self.spawn_worker("nexora-update-check", run_check)
    }

    /// 在独立工作线程中下载、验证并暂存一份已经确认的更新。
    ///
    /// `release` 必须来自同一份 [`UpdateConfig`] 执行 [`Self::check`] 后发送的
    /// [`UpdateEvent::UpdateAvailable`]。该阶段不会再次弹出确认 UI；完成后通过
    /// [`UpdateEvent::ReadyToRestart`] 返回可安装的暂存更新。
    ///
    /// # Errors
    ///
    /// 当操作系统无法创建 updater 下载线程时返回 [`UpdateError::Io`]。
    pub fn download(&self, release: UpdateRelease) -> Result<UpdateSession, UpdateError> {
        let (sender, receiver) = async_channel::unbounded();
        let cancellation = CancellationToken::default();
        let worker_cancellation = cancellation.clone();
        let config = self.config.clone();

        thread::Builder::new()
            .name("nexora-update-download".to_owned())
            .spawn(move || {
                run_download(config, release, worker_cancellation, sender);
            })?;

        Ok(UpdateSession {
            events: receiver,
            cancellation,
        })
    }

    fn spawn_worker(
        &self,
        name: &str,
        run: fn(UpdateConfig, CancellationToken, Sender<UpdateEvent>),
    ) -> Result<UpdateSession, UpdateError> {
        let (sender, receiver) = async_channel::unbounded();
        let cancellation = CancellationToken::default();
        let worker_cancellation = cancellation.clone();
        let config = self.config.clone();

        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || run(config, worker_cancellation, sender))?;

        Ok(UpdateSession {
            events: receiver,
            cancellation,
        })
    }
}

fn run_check(config: UpdateConfig, cancellation: CancellationToken, sender: Sender<UpdateEvent>) {
    let result = run_check_inner(&config, &cancellation, &sender);
    send_terminal_error(result, &sender);
}

fn run_update(config: UpdateConfig, cancellation: CancellationToken, sender: Sender<UpdateEvent>) {
    let cleanup_sender = start_staging_cleanup_worker();
    cleanup_stale_staging_roots(&config);
    let result = run_update_inner(&config, &cancellation, &sender, cleanup_sender);
    send_terminal_error(result, &sender);
}

fn run_download(
    config: UpdateConfig,
    release: UpdateRelease,
    cancellation: CancellationToken,
    sender: Sender<UpdateEvent>,
) {
    let cleanup_sender = start_staging_cleanup_worker();
    cleanup_stale_staging_roots(&config);
    let result = build_client(&config)
        .and_then(|client| {
            download_and_stage(
                &client,
                &config,
                release,
                &cancellation,
                &sender,
                cleanup_sender,
            )
        })
        .and_then(|staged| send_event(&sender, UpdateEvent::ReadyToRestart(staged)));
    send_terminal_error(result, &sender);
}

fn run_update_inner(
    config: &UpdateConfig,
    cancellation: &CancellationToken,
    sender: &Sender<UpdateEvent>,
    cleanup_sender: Option<mpsc::Sender<PathBuf>>,
) -> Result<(), UpdateError> {
    let Some((client, release)) = check_release(config, cancellation, sender)? else {
        return Ok(());
    };

    send_event(sender, UpdateEvent::UpdateAvailable(release.clone()))?;
    let staged = download_and_stage(
        &client,
        config,
        release,
        cancellation,
        sender,
        cleanup_sender,
    )?;
    send_event(sender, UpdateEvent::ReadyToRestart(staged))
}

fn run_check_inner(
    config: &UpdateConfig,
    cancellation: &CancellationToken,
    sender: &Sender<UpdateEvent>,
) -> Result<(), UpdateError> {
    let Some((_, release)) = check_release(config, cancellation, sender)? else {
        return Ok(());
    };
    send_event(sender, UpdateEvent::UpdateAvailable(release))
}

fn check_release(
    config: &UpdateConfig,
    cancellation: &CancellationToken,
    sender: &Sender<UpdateEvent>,
) -> Result<Option<(Client, UpdateRelease)>, UpdateError> {
    send_event(sender, UpdateEvent::Checking)?;
    cancellation.ensure_active()?;

    let client = build_client(config)?;
    let manifest_text = client
        .get(config.manifest_url.clone())
        .send()?
        .error_for_status()?
        .text()?;
    cancellation.ensure_active()?;

    let envelope: SignedUpdateManifest =
        serde_json::from_str(&manifest_text).map_err(UpdateError::InvalidManifest)?;
    let manifest = envelope.verify(config.trusted_public_keys())?;
    let target = UpdateTarget::current()?;
    let Some(release) = manifest.select_update(config, target)? else {
        send_event(sender, UpdateEvent::UpToDate)?;
        return Ok(None);
    };

    Ok(Some((client, release.with_verified_manifest(envelope))))
}

fn build_client(config: &UpdateConfig) -> Result<Client, UpdateError> {
    Client::builder()
        .timeout(config.request_timeout)
        .user_agent(format!(
            "{}/{} ({})",
            config.app_id, config.current_version, config.current_build_number
        ))
        .build()
        .map_err(UpdateError::from)
}

fn send_terminal_error(result: Result<(), UpdateError>, sender: &Sender<UpdateEvent>) {
    if let Err(error) = result {
        let event = if matches!(error, UpdateError::Cancelled) {
            UpdateEvent::Cancelled
        } else {
            UpdateEvent::Failed(error.to_string())
        };
        _ = sender.send_blocking(event);
    }
}

fn download_and_stage(
    client: &Client,
    config: &UpdateConfig,
    release: UpdateRelease,
    cancellation: &CancellationToken,
    sender: &Sender<UpdateEvent>,
    cleanup_sender: Option<mpsc::Sender<PathBuf>>,
) -> Result<StagedUpdate, UpdateError> {
    let staging_root = create_staging_root(config, &release)?;
    let archive_path = staging_root.join(artifact_archive_file_name(&release.artifact.kind)?);
    let extract_path = staging_root.join("extracted");
    fs::create_dir_all(&extract_path)?;

    let result = (|| {
        let artifact_url = config
            .manifest_url
            .join(&release.artifact.url)
            .map_err(|error| UpdateError::InvalidUrl(error.to_string()))?;
        let mut response = client.get(artifact_url).send()?.error_for_status()?;
        let total = response.content_length().or(Some(release.artifact.size));
        let mut archive = File::create(&archive_path)?;
        let mut hasher = Sha256::new();
        let mut downloaded = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];

        loop {
            cancellation.ensure_active()?;
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }

            archive.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            downloaded += read as u64;
            send_event(sender, UpdateEvent::Downloading { downloaded, total })?;
        }
        archive.sync_all()?;
        if downloaded != release.artifact.size {
            return Err(UpdateError::SizeMismatch {
                expected: release.artifact.size,
                actual: downloaded,
            });
        }

        send_event(sender, UpdateEvent::Verifying)?;
        if !supported_artifact_kind(&release.artifact.kind) {
            return Err(UpdateError::UnsupportedArtifactKind(
                release.artifact.kind.clone(),
            ));
        }
        let digest = hasher.finalize();
        let actual_sha256 = format_digest(&digest);
        if !actual_sha256.eq_ignore_ascii_case(release.artifact.sha256.trim()) {
            return Err(UpdateError::ChecksumMismatch {
                expected: release.artifact.sha256.clone(),
                actual: actual_sha256,
            });
        }

        cancellation.ensure_active()?;
        send_event(sender, UpdateEvent::Staging)?;
        let (staged_app, current_app, sidecar_path) = match release.artifact.kind.as_str() {
            "macos_app_zip" => {
                macos::extract_app_archive(&archive_path, &extract_path)?;
                let staged_app = macos::find_app_bundle(&extract_path)?;
                macos::verify_code_signature(&staged_app, config.expected_team_id())?;
                let current_app = config
                    .app_bundle_path
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(macos::current_app_bundle)?;
                let sidecar_path = config.sidecar_path()?;
                (staged_app, current_app, sidecar_path)
            }
            "windows_update_zip" | "windows_zip" => {
                let main_exe = crate::windows::current_main_exe_name()?;
                let updater_exe = crate::windows::updater_exe_name_for(&main_exe)?;
                crate::windows::extract_windows_update_zip(
                    &archive_path,
                    &extract_path,
                    &main_exe,
                    &updater_exe,
                )?;
                crate::windows::verify_staged_update_signatures(
                    &extract_path,
                    &main_exe,
                    &updater_exe,
                    config.windows_signature(),
                )?;
                let current_app = config
                    .app_bundle_path
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(crate::windows::current_install_dir)?;
                let sidecar_path = extract_path.join(updater_exe);
                (extract_path.clone(), current_app, sidecar_path)
            }
            _ => {
                return Err(UpdateError::UnsupportedArtifactKind(
                    release.artifact.kind.clone(),
                ));
            }
        };

        Ok(StagedUpdate {
            release,
            app_id: config.app_id.clone(),
            staged_app,
            current_app,
            sidecar_path,
            health_timeout: config.health_timeout(),
            pending_record: None,
            cleanup: Arc::new(StagingCleanup {
                staging_root: staging_root.clone(),
                installer_started: AtomicBool::new(false),
                retained: AtomicBool::new(false),
                cleanup_sender,
            }),
        })
    })();

    if result.is_err() {
        _ = fs::remove_dir_all(&staging_root);
    }

    result
}

fn send_event(sender: &Sender<UpdateEvent>, event: UpdateEvent) -> Result<(), UpdateError> {
    sender
        .send_blocking(event)
        .map_err(|_| UpdateError::EventReceiverClosed)
}

fn supported_artifact_kind(kind: &str) -> bool {
    matches!(kind, "macos_app_zip" | "windows_update_zip" | "windows_zip")
}

fn artifact_archive_file_name(kind: &str) -> Result<&'static str, UpdateError> {
    match kind {
        "macos_app_zip" => Ok("update.app.zip"),
        "windows_update_zip" | "windows_zip" => Ok("update.windows.zip"),
        other => Err(UpdateError::UnsupportedArtifactKind(other.to_owned())),
    }
}

fn default_sidecar_path() -> Result<PathBuf, UpdateError> {
    if cfg!(target_os = "windows") {
        return crate::windows::default_sidecar_path();
    }
    macos::default_sidecar_path()
}

fn spawn_install_helper(request: InstallHelperRequest<'_>) -> Result<(), UpdateError> {
    if cfg!(target_os = "windows") {
        return crate::windows::spawn_install_helper(crate::windows::InstallHelperRequest {
            process_id: request.process_id,
            app_id: request.app_id,
            current_app: request.current_app,
            staged_app: request.staged_app,
            staging_root: request.staging_root,
            sidecar_path: request.sidecar_path,
            health_timeout: request.health_timeout,
            pending_records: request.pending_records,
        });
    }
    macos::spawn_install_helper(macos::InstallHelperRequest {
        process_id: request.process_id,
        app_id: request.app_id,
        current_app: request.current_app,
        staged_app: request.staged_app,
        staging_root: request.staging_root,
        sidecar_path: request.sidecar_path,
        health_timeout: request.health_timeout,
        pending_records: request.pending_records,
    })
}

fn create_staging_root(
    config: &UpdateConfig,
    release: &UpdateRelease,
) -> Result<PathBuf, UpdateError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| UpdateError::Random(error.to_string()))?;
    let nonce = URL_SAFE_NO_PAD.encode(random);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = staging_base(config)?.join(format!(
        "{}-{}-{}-{timestamp}-{nonce}",
        release.version,
        release.build_number,
        std::process::id()
    ));
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn staging_base(config: &UpdateConfig) -> Result<PathBuf, UpdateError> {
    let cache_dir = config.cache_dir()?;
    #[cfg(target_os = "windows")]
    crate::windows::prepare_cache_dir(&cache_dir)?;
    Ok(cache_dir.join("staging"))
}

fn start_staging_cleanup_worker() -> Option<mpsc::Sender<PathBuf>> {
    let (sender, receiver) = mpsc::channel();
    match thread::Builder::new()
        .name("nexora-update-cleanup".to_owned())
        .spawn(move || {
            while let Ok(staging_root) = receiver.recv() {
                discard_staging_root(staging_root);
            }
        }) {
        Ok(_) => Some(sender),
        Err(error) => {
            tracing::warn!(error = %error, "无法启动更新暂存目录清理线程");
            None
        }
    }
}

fn discard_staging_root(staging_root: PathBuf) {
    let file_name = staging_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("staging");
    let discarded_root = staging_root.with_file_name(format!(".discarded-{file_name}"));
    let cleanup_root = match fs::rename(&staging_root, &discarded_root) {
        Ok(()) => discarded_root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            tracing::warn!(
                path = %staging_root.display(),
                error = %error,
                "无法标记待清理的更新暂存目录"
            );
            staging_root
        }
    };
    if let Err(error) = fs::remove_dir_all(&cleanup_root)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(path = %cleanup_root.display(), error = %error, "无法清理更新暂存目录");
    }
}

fn cleanup_stale_staging_roots(config: &UpdateConfig) {
    let Ok(base) = staging_base(config) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&base) else {
        return;
    };

    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        let file_name = entry.file_name();
        let discarded = file_name.to_string_lossy().starts_with(".discarded-");
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age >= STALE_STAGING_AGE);
        if !discarded && !stale {
            continue;
        }

        let path = entry.path();
        if let Err(error) = fs::remove_dir_all(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %path.display(), error = %error, "无法清理遗留的更新暂存目录");
        }
    }
}

fn restore_pending_inner(
    config: &UpdateConfig,
    cache_dir: &Path,
    record_path: &Path,
) -> Result<StagedUpdate, UpdateError> {
    let contents = fs::read(record_path)?;
    let record: PendingUpdateRecord =
        serde_json::from_slice(&contents).map_err(UpdateError::InvalidPendingRecord)?;
    if record.schema_version != PENDING_RECORD_SCHEMA_VERSION {
        return Err(UpdateError::InvalidPendingRecordSchema(
            record.schema_version,
        ));
    }

    let target = UpdateTarget::current()?;
    if record.app_id != config.app_id()
        || record.channel != config.channel()
        || record.target != target.as_str()
    {
        return Err(UpdateError::PendingMetadataMismatch);
    }
    let manifest = record.manifest.verify(config.trusted_public_keys())?;
    let Some(release) = manifest.select_update(config, target)? else {
        return Err(UpdateError::PendingReleaseNotNewer);
    };
    if release.version != record.version
        || release.build_number != record.build_number
        || manifest.app_id != record.app_id
        || manifest.channel != record.channel
        || release.artifact.target != record.target
        || !release_is_newer(config, &release)
    {
        return Err(UpdateError::PendingMetadataMismatch);
    }
    if !supported_artifact_kind(&release.artifact.kind) {
        return Err(UpdateError::UnsupportedArtifactKind(
            release.artifact.kind.clone(),
        ));
    }

    let staging_root = resolve_cached_path(cache_dir, &record.staging_root)?;
    let pending_base = cache_dir.join("pending");
    ensure_path_within(&staging_root, &pending_base)?;
    let archive_path = resolve_cached_path(cache_dir, &record.archive_path)?;
    let staged_app = resolve_cached_path(cache_dir, &record.staged_app)?;
    ensure_path_within(&archive_path, &staging_root)?;
    ensure_path_within(&staged_app, &staging_root)?;
    let archive_size = fs::metadata(&archive_path)?.len();
    if archive_size != release.artifact.size {
        return Err(UpdateError::SizeMismatch {
            expected: release.artifact.size,
            actual: archive_size,
        });
    }
    let actual_sha256 = sha256_file(&archive_path)?;
    if !actual_sha256.eq_ignore_ascii_case(release.artifact.sha256.trim()) {
        return Err(UpdateError::ChecksumMismatch {
            expected: release.artifact.sha256.clone(),
            actual: actual_sha256,
        });
    }

    let (current_app, sidecar_path) = match release.artifact.kind.as_str() {
        "macos_app_zip" => {
            if staged_app
                .extension()
                .is_none_or(|extension| extension != "app")
            {
                return Err(UpdateError::InvalidPendingPath);
            }
            let discovered_app = macos::find_app_bundle(&staging_root.join("extracted"))?;
            if fs::canonicalize(discovered_app)? != fs::canonicalize(&staged_app)? {
                return Err(UpdateError::InvalidPendingPath);
            }
            macos::verify_code_signature(&staged_app, config.expected_team_id())?;
            let current_app = config
                .app_bundle_path
                .clone()
                .map(Ok)
                .unwrap_or_else(macos::current_app_bundle)?;
            let sidecar_path = config.sidecar_path()?;
            (current_app, sidecar_path)
        }
        "windows_update_zip" | "windows_zip" => {
            if !staged_app.is_dir() {
                return Err(UpdateError::InvalidPendingPath);
            }
            let main_exe = crate::windows::current_main_exe_name()?;
            let updater_exe = crate::windows::updater_exe_name_for(&main_exe)?;
            for required in [&main_exe, &updater_exe, "nexora-updater.json"] {
                if !staged_app.join(required).is_file() {
                    return Err(UpdateError::InvalidPendingPath);
                }
            }
            crate::windows::verify_staged_update_signatures(
                &staged_app,
                &main_exe,
                &updater_exe,
                config.windows_signature(),
            )?;
            let current_app = config
                .app_bundle_path
                .clone()
                .map(Ok)
                .unwrap_or_else(crate::windows::current_install_dir)?;
            let sidecar_path = staged_app.join(updater_exe);
            (current_app, sidecar_path)
        }
        _ => {
            return Err(UpdateError::UnsupportedArtifactKind(
                release.artifact.kind.clone(),
            ));
        }
    };

    Ok(StagedUpdate {
        release: release.with_verified_manifest(record.manifest),
        app_id: config.app_id.clone(),
        staged_app,
        current_app,
        sidecar_path,
        health_timeout: config.health_timeout(),
        pending_record: Some(record_path.to_path_buf()),
        cleanup: Arc::new(StagingCleanup {
            staging_root,
            installer_started: AtomicBool::new(false),
            retained: AtomicBool::new(true),
            cleanup_sender: None,
        }),
    })
}

fn release_is_newer(config: &UpdateConfig, release: &UpdateRelease) -> bool {
    release.version > *config.current_version()
        || (release.version == *config.current_version()
            && release.build_number > config.current_build_number())
}

fn pending_candidate_name(release: &UpdateRelease) -> Result<String, UpdateError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| UpdateError::Random(error.to_string()))?;
    Ok(format!(
        "{}-{}-{}",
        release.version,
        release.build_number,
        URL_SAFE_NO_PAD.encode(random)
    ))
}

fn remap_staging_path(
    path: &Path,
    source: &Path,
    destination: &Path,
) -> Result<PathBuf, UpdateError> {
    path.strip_prefix(source)
        .map(|relative| destination.join(relative))
        .map_err(|_| UpdateError::InvalidPendingPath)
}

fn relative_to_cache(path: &Path, cache_dir: &Path) -> Result<PathBuf, UpdateError> {
    path.strip_prefix(cache_dir)
        .map(Path::to_path_buf)
        .map_err(|_| UpdateError::InvalidPendingPath)
}

fn resolve_cached_path(cache_dir: &Path, relative: &Path) -> Result<PathBuf, UpdateError> {
    validate_relative_path(relative)?;
    let path = cache_dir.join(relative);
    ensure_path_within(&path, cache_dir)?;
    Ok(path)
}

fn validate_relative_path(path: &Path) -> Result<(), UpdateError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(UpdateError::InvalidPendingPath);
    }
    Ok(())
}

fn ensure_path_within(path: &Path, root: &Path) -> Result<(), UpdateError> {
    let canonical_root = fs::canonicalize(root)?;
    let canonical_path = fs::canonicalize(path)?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(UpdateError::InvalidPendingPath);
    }
    Ok(())
}

fn write_pending_record(
    cache_dir: &Path,
    record_path: &Path,
    record: &PendingUpdateRecord,
) -> Result<(), UpdateError> {
    fs::create_dir_all(cache_dir)?;
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).map_err(|error| UpdateError::Random(error.to_string()))?;
    let temporary = cache_dir.join(format!(".pending-{}.tmp", URL_SAFE_NO_PAD.encode(random)));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        serde_json::to_writer(&mut file, record)
            .map_err(UpdateError::InvalidPendingRecordSerialization)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        replace_record_file(&temporary, record_path)?;
        sync_directory_best_effort(cache_dir);
        Ok(())
    })();
    if result.is_err() {
        _ = fs::remove_file(temporary);
    }
    result
}

fn replace_record_file(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    if cfg!(target_os = "windows") {
        return crate::windows::replace_file(source, destination);
    }
    fs::rename(source, destination)?;
    Ok(())
}

fn sync_directory_best_effort(_path: &Path) {
    #[cfg(not(target_os = "windows"))]
    if let Err(error) = File::open(_path).and_then(|directory| directory.sync_all()) {
        tracing::warn!(path = %_path.display(), error = %error, "待安装记录已提交，但无法同步缓存目录");
    }
}

fn cleanup_other_pending_roots(pending_base: &Path, retained: &Path) {
    let Ok(entries) = fs::read_dir(pending_base) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == retained || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        if let Err(error) = fs::remove_dir_all(&path) {
            tracing::warn!(path = %path.display(), error = %error, "无法清理被替换的待安装更新");
        }
    }
}

fn discard_pending_cache(cache_dir: &Path) {
    for path in [
        cache_dir.join(PENDING_RECORD_FILE_NAME),
        cache_dir.join("pending"),
    ] {
        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        if let Err(error) = result
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %path.display(), error = %error, "无法清理无效待安装缓存");
        }
    }
}

fn sha256_file(path: &Path) -> Result<String, UpdateError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format_digest(&hasher.finalize()))
}

fn normalize_windows_thumbprint(value: &str) -> Result<String, UpdateError> {
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
        Err(UpdateError::InvalidBundleConfig(
            "expected_windows_signer_thumbprint 必须是 SHA-1 证书指纹".to_owned(),
        ))
    }
}

fn valid_app_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_APP_ID_BYTES
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'-' | b'_'))
}

fn validate_transport(url: &Url, allow_insecure_http: bool) -> Result<(), UpdateError> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if allow_insecure_http || is_loopback_url(url) => Ok(()),
        "http" => Err(UpdateError::InsecureHttpDenied),
        scheme => Err(UpdateError::UnsupportedUrlScheme(scheme.to_owned())),
    }
}

fn is_loopback_url(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn format_digest(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            write!(output, "{byte:02x}").expect("写入 String 不会失败");
            output
        },
    )
}

fn parse_bundled_duration(value: &str) -> Result<Duration, UpdateError> {
    let (number, multiplier) = [
        ("ms", 0_u64),
        ("s", 1),
        ("m", 60),
        ("h", 60 * 60),
        ("d", 24 * 60 * 60),
    ]
    .into_iter()
    .find_map(|(suffix, multiplier)| {
        value
            .strip_suffix(suffix)
            .map(|number| (number, multiplier))
    })
    .ok_or_else(|| {
        UpdateError::InvalidBundleConfig(format!(
            "health_timeout `{value}` 必须使用 ms、s、m、h 或 d 单位"
        ))
    })?;
    let amount = number.parse::<u64>().map_err(|_| {
        UpdateError::InvalidBundleConfig(format!("health_timeout `{value}` 不是有效时长"))
    })?;
    if multiplier == 0 {
        return Ok(Duration::from_millis(amount));
    }
    amount
        .checked_mul(multiplier)
        .map(Duration::from_secs)
        .ok_or_else(|| {
            UpdateError::InvalidBundleConfig(format!("health_timeout `{value}` 超出支持范围"))
        })
}

/// 更新配置、网络传输、清单解析、校验或安装阶段可能产生的错误。
#[derive(Debug, Error)]
pub enum UpdateError {
    /// 应用 bundle 中的 updater 安全配置缺失必要字段、格式无效或版本不受支持。
    #[error("应用 bundle updater 配置无效: {0}")]
    InvalidBundleConfig(
        /// 面向用户的具体配置失败原因。
        String,
    ),
    /// 更新清单或安装包 URL 无效。
    #[error("更新地址无效: {0}")]
    InvalidUrl(
        /// URL 解析器返回的具体失败原因。
        String,
    ),
    /// 应用标识为空、过长或包含不能安全用于暂存目录的字符。
    #[error(
        "应用标识无效；请使用字母或数字开头和结尾，并且只包含 ASCII 字母、数字、点、连字符或下划线"
    )]
    InvalidAppId,
    /// 当前应用版本不是合法的 SemVer。
    #[error("当前应用版本无效: {0}")]
    InvalidVersion(
        /// SemVer 解析器返回的具体失败原因。
        #[from]
        semver::Error,
    ),
    /// `latest.json` 不是有效的更新清单。
    #[error("更新清单格式无效: {0}")]
    InvalidManifest(
        /// JSON 反序列化更新清单时产生的具体错误。
        #[source]
        serde_json::Error,
    ),
    /// 更新清单负载无法序列化为签名字节。
    #[error("更新清单无法序列化用于签名: {0}")]
    InvalidManifestSerialization(
        /// JSON 序列化失败原因。
        String,
    ),
    /// 更新服务器使用了客户端尚不支持的协议版本。
    #[error("不支持更新协议版本 {0}")]
    UnsupportedSchema(
        /// 服务端清单声明、但当前客户端无法处理的协议版本。
        u32,
    ),
    /// 更新地址使用了不支持的协议。
    #[error("更新地址协议 `{0}` 不受支持")]
    UnsupportedUrlScheme(
        /// URL 中的 scheme。
        String,
    ),
    /// 非 loopback 明文 HTTP 未显式允许。
    #[error("更新地址使用明文 HTTP，必须显式设置 allow_insecure_http = true")]
    InsecureHttpDenied,
    /// 客户端未配置任何可信 Ed25519 公钥。
    #[error("更新清单验签失败：未配置可信公钥")]
    MissingTrustedPublicKeys,
    /// 下载流程缺少最初通过验签的完整清单信封，不能安全持久化。
    #[error("更新缺少已验证的签名清单")]
    MissingVerifiedManifest,
    /// 公钥配置格式无效。
    #[error("更新公钥格式无效；应为 key_id:ed25519:BASE64_PUBLIC_KEY")]
    InvalidPublicKey,
    /// 更新清单签名无法由任何可信公钥验证。
    #[error("更新清单签名无效或使用了未知公钥")]
    ManifestSignatureRejected,
    /// 清单属于其他应用。
    #[error("更新清单应用标识不匹配，期望 `{expected}`，实际 `{actual}`")]
    AppIdMismatch {
        /// 当前应用配置的标识。
        expected: String,
        /// 清单声明的标识。
        actual: String,
    },
    /// 清单通道与当前应用通道不一致。
    #[error("更新通道不匹配，期望 `{}`，实际 `{}`", expected.as_str(), actual.as_str())]
    ChannelMismatch {
        /// 当前应用接收的更新通道。
        expected: UpdateChannel,
        /// 清单实际所属的更新通道。
        actual: UpdateChannel,
    },
    /// 清单没有提供当前操作系统和架构的安装包。
    #[error("更新清单缺少目标 `{0}` 的安装包")]
    MissingArtifact(
        /// 当前客户端需要、但清单没有提供的 Rust target triple。
        String,
    ),
    /// 清单序号低于客户端已经接受的最高序号。
    #[error("更新清单序号回放，已接受最高 `{highest}`，实际 `{actual}`")]
    ManifestReplay {
        /// 客户端已经接受的最高序号。
        highest: u64,
        /// 本次清单声明的序号。
        actual: u64,
    },
    /// 指定的 Rust target triple 暂不受自动更新协议支持。
    #[error("更新目标 `{0}` 暂不受支持")]
    UnsupportedTarget(
        /// 不受支持的 Rust target triple。
        String,
    ),
    /// 当前平台尚未实现原位安装。
    #[error("当前平台暂不支持自动安装更新")]
    UnsupportedPlatform,
    /// Windows update ZIP 格式、路径或必需文件不合法。
    #[error("Windows update ZIP 无效: {0}")]
    InvalidWindowsZipArchive(
        /// 面向用户和日志的稳定诊断信息。
        String,
    ),
    /// HTTP 请求或响应读取失败。
    #[error("更新网络请求失败: {0}")]
    Http(
        /// HTTP 客户端返回的请求、状态码或响应读取错误。
        #[from]
        reqwest::Error,
    ),
    /// 文件创建、读取、写入或清理失败。
    #[error("更新文件操作失败: {0}")]
    Io(
        /// 文件系统、系统命令或工作线程创建阶段产生的 I/O 错误。
        #[from]
        std::io::Error,
    ),
    /// 下载内容与清单摘要不一致。
    #[error("安装包 SHA-256 校验失败，期望 `{expected}`，实际 `{actual}`")]
    ChecksumMismatch {
        /// 清单声明的摘要。
        expected: String,
        /// 下载内容计算得到的摘要。
        actual: String,
    },
    /// 下载内容大小与清单声明不一致。
    #[error("安装包大小校验失败，期望 `{expected}` 字节，实际 `{actual}` 字节")]
    SizeMismatch {
        /// 清单声明的字节数。
        expected: u64,
        /// 实际下载的字节数。
        actual: u64,
    },
    /// 清单声明了当前平台不支持的更新包类型。
    #[error("更新包类型 `{0}` 暂不支持")]
    UnsupportedArtifactKind(
        /// 清单中的 artifact kind。
        String,
    ),
    /// Windows ZIP 条目路径存在越界或 NTFS 特殊语义风险。
    #[error("Windows 更新包条目路径不安全: {0}")]
    InvalidWindowsZipEntry(
        /// ZIP 中声明的原始条目路径。
        String,
    ),
    /// macOS 系统命令执行失败。
    #[error("macOS 更新命令 `{command}` 执行失败: {message}")]
    CommandFailed {
        /// 执行失败的命令名称。
        command: &'static str,
        /// 命令输出或失败原因。
        message: String,
    },
    /// 解压目录中没有且仅有一个 `.app`。
    #[error("更新包中找不到唯一的 macOS .app")]
    InvalidAppArchive,
    /// 下载后的应用不属于配置要求的签名团队。
    #[error("应用签名团队不匹配，期望 `{expected}`，实际 `{actual}`")]
    TeamIdMismatch {
        /// 配置要求的 Apple Team ID。
        expected: String,
        /// 安装包实际读取到的 Team ID。
        actual: String,
    },
    /// 当前进程不是从 macOS `.app` 内启动，无法确定替换位置。
    #[error("当前程序不是从 macOS .app 中启动，无法执行原位更新")]
    AppBundleNotFound,
    /// 无法找到或复制独立 updater sidecar。
    #[error("无法准备独立 updater sidecar: {0}")]
    SidecarUnavailable(
        /// 面向用户的失败原因。
        String,
    ),
    /// sidecar 事务替换、重启或回滚失败。
    #[error("sidecar 安装失败: {0}")]
    SidecarFailed(
        /// 面向用户的失败原因。
        String,
    ),
    /// 新版本未在限定时间内完成健康确认。
    #[error("新版本健康确认超时")]
    HealthCheckTimedOut,
    /// 健康确认会话参数无效或与当前会话不匹配。
    #[error("健康确认会话无效")]
    InvalidHealthSession,
    /// 无法生成一次性更新会话随机值。
    #[error("无法生成更新会话随机值: {0}")]
    Random(
        /// 系统随机源返回的错误。
        String,
    ),
    /// 当前暂存更新已经启动过安装 helper。
    #[error("更新安装已经启动，请等待应用退出并完成替换")]
    InstallerAlreadyStarted,
    /// 当前平台无法确定应用级用户缓存目录。
    #[error("无法确定应用级更新缓存目录")]
    CacheDirectoryUnavailable,
    /// 待安装记录不是合法 JSON。
    #[error("待安装更新记录无效: {0}")]
    InvalidPendingRecord(
        /// JSON 解析器返回的具体失败原因。
        #[source]
        serde_json::Error,
    ),
    /// 待安装记录无法序列化。
    #[error("无法写入待安装更新记录: {0}")]
    InvalidPendingRecordSerialization(
        /// JSON 序列化器返回的具体失败原因。
        #[source]
        serde_json::Error,
    ),
    /// 待安装记录使用了不受支持的 schema 版本。
    #[error("不支持待安装更新记录版本 {0}")]
    InvalidPendingRecordSchema(
        /// 本地记录声明的 schema 版本。
        u32,
    ),
    /// 待安装记录中的路径不是缓存目录内的受控相对路径。
    #[error("待安装更新路径越出应用缓存边界")]
    InvalidPendingPath,
    /// 待安装记录元数据与签名清单或当前应用配置不一致。
    #[error("待安装更新元数据与当前应用不匹配")]
    PendingMetadataMismatch,
    /// 待安装版本已经不高于当前运行版本。
    #[error("待安装更新已经不高于当前版本")]
    PendingReleaseNotNewer,
    /// 用户主动取消了更新。
    #[error("更新已取消")]
    Cancelled,
    /// UI 已经销毁事件接收端，工作线程应停止。
    #[error("更新界面已经关闭")]
    EventReceiverClosed,
}
