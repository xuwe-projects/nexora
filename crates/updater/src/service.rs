//! 更新工作线程、事件流与公共配置。

use std::{
    fmt::Write as _,
    fs::{self, File},
    io::{Read as _, Write as _},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_channel::{Receiver, Sender};
use reqwest::{Url, blocking::Client};
use semver::Version;
use sha2::{Digest as _, Sha256};
use thiserror::Error;

use crate::{UpdateChannel, UpdateManifest, UpdateRelease, UpdateTarget, macos};

const MAX_APP_ID_BYTES: usize = 255;
const STALE_STAGING_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// 启动一次更新检查所需的应用配置。
///
/// 每个桌面应用可以在初始化阶段创建自己的配置，并把 [`Updater`] 交给 UI 层使用。
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    manifest_url: Url,
    app_id: String,
    current_version: Version,
    current_bundle_version: u64,
    channel: UpdateChannel,
    expected_team_id: Option<String>,
    request_timeout: Duration,
    app_bundle_path: Option<PathBuf>,
}

impl UpdateConfig {
    /// 创建应用更新配置。
    ///
    /// `manifest_url` 指向当前通道的 `latest.json`；`app_id` 必须和清单一致，并且是由
    /// ASCII 字母、数字、点、连字符或下划线组成的安全路径分量；
    /// `current_version` 使用 SemVer；`current_bundle_version` 是当前安装包构建号。
    ///
    /// # Errors
    ///
    /// 当清单地址不是有效 URL、应用标识不安全，或当前版本不是有效 SemVer 时返回错误。
    pub fn new(
        manifest_url: impl AsRef<str>,
        app_id: impl Into<String>,
        current_version: impl AsRef<str>,
        current_bundle_version: u64,
        channel: UpdateChannel,
    ) -> Result<Self, UpdateError> {
        let app_id = app_id.into();
        if !valid_app_id(&app_id) {
            return Err(UpdateError::InvalidAppId);
        }

        Ok(Self {
            manifest_url: Url::parse(manifest_url.as_ref())
                .map_err(|error| UpdateError::InvalidUrl(error.to_string()))?,
            app_id,
            current_version: Version::parse(current_version.as_ref())?,
            current_bundle_version,
            channel,
            expected_team_id: None,
            request_timeout: Duration::from_secs(30),
            app_bundle_path: None,
        })
    }

    /// 要求下载后的 `.app` 必须由指定 Apple Team ID 签名。
    ///
    /// 未设置时仍会执行 `codesign --verify --deep --strict`，但不会限制具体签名团队。
    pub fn with_expected_team_id(mut self, team_id: impl Into<String>) -> Self {
        self.expected_team_id = Some(team_id.into());
        self
    }

    /// 设置检查清单和下载安装包时使用的单次请求超时。
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// 显式指定当前运行中的 `.app` 路径。
    ///
    /// 正常发布环境无需设置，updater 会从当前可执行文件路径向上查找 `.app`。
    /// 该选项主要用于集成测试和非标准应用启动器。
    pub fn with_app_bundle_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.app_bundle_path = Some(path.into());
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
    pub const fn current_bundle_version(&self) -> u64 {
        self.current_bundle_version
    }

    /// 返回当前更新通道。
    pub const fn channel(&self) -> UpdateChannel {
        self.channel
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
    /// 服务端版本不高于当前 `(version, bundle_version)`。
    UpToDate,
    /// 已发现新版本，即将开始下载安装包。
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
    staged_app: PathBuf,
    current_app: PathBuf,
    cleanup: Arc<StagingCleanup>,
}

#[derive(Debug)]
struct StagingCleanup {
    staging_root: PathBuf,
    installer_started: AtomicBool,
    cleanup_sender: Option<mpsc::Sender<PathBuf>>,
}

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if self.installer_started.load(Ordering::Acquire) {
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
        let result = macos::spawn_install_helper(
            std::process::id(),
            &self.current_app,
            &self.staged_app,
            &self.cleanup.staging_root,
        );
        if result.is_err() {
            self.cleanup
                .installer_started
                .store(false, Ordering::Release);
        }
        result
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

    /// 在独立工作线程中检查、下载、验证并暂存更新。
    ///
    /// 返回的 [`UpdateSession`] 可直接由 GPUI Entity 持有；关闭弹窗或销毁 Entity 时调用
    /// [`CancellationToken::cancel`] 即可停止后续工作。
    ///
    /// # Errors
    ///
    /// 当操作系统无法创建 updater 工作线程时返回 [`UpdateError::Io`]。
    pub fn start(&self) -> Result<UpdateSession, UpdateError> {
        let (sender, receiver) = async_channel::unbounded();
        let cancellation = CancellationToken::default();
        let worker_cancellation = cancellation.clone();
        let config = self.config.clone();

        thread::Builder::new()
            .name("xuwe-updater".to_owned())
            .spawn(move || run_update(config, worker_cancellation, sender))?;

        Ok(UpdateSession {
            events: receiver,
            cancellation,
        })
    }
}

fn run_update(config: UpdateConfig, cancellation: CancellationToken, sender: Sender<UpdateEvent>) {
    let cleanup_sender = start_staging_cleanup_worker();
    cleanup_stale_staging_roots(&config);
    let result = run_update_inner(&config, &cancellation, &sender, cleanup_sender);
    if let Err(error) = result {
        let event = if matches!(error, UpdateError::Cancelled) {
            UpdateEvent::Cancelled
        } else {
            UpdateEvent::Failed(error.to_string())
        };
        _ = sender.send_blocking(event);
    }
}

fn run_update_inner(
    config: &UpdateConfig,
    cancellation: &CancellationToken,
    sender: &Sender<UpdateEvent>,
    cleanup_sender: Option<mpsc::Sender<PathBuf>>,
) -> Result<(), UpdateError> {
    send_event(sender, UpdateEvent::Checking)?;
    cancellation.ensure_active()?;

    let client = Client::builder()
        .timeout(config.request_timeout)
        .user_agent(format!(
            "{}/{} ({})",
            config.app_id, config.current_version, config.current_bundle_version
        ))
        .build()?;
    let manifest_text = client
        .get(config.manifest_url.clone())
        .send()?
        .error_for_status()?
        .text()?;
    cancellation.ensure_active()?;

    let manifest = UpdateManifest::parse(&manifest_text)?;
    let target = UpdateTarget::current()?;
    let Some(release) = manifest.select_update(config, target)? else {
        send_event(sender, UpdateEvent::UpToDate)?;
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

fn download_and_stage(
    client: &Client,
    config: &UpdateConfig,
    release: UpdateRelease,
    cancellation: &CancellationToken,
    sender: &Sender<UpdateEvent>,
    cleanup_sender: Option<mpsc::Sender<PathBuf>>,
) -> Result<StagedUpdate, UpdateError> {
    let staging_root = create_staging_root(config, &release)?;
    let archive_path = staging_root.join("update.app.zip");
    let extract_path = staging_root.join("extracted");
    fs::create_dir_all(&extract_path)?;

    let result = (|| {
        let artifact_url = config
            .manifest_url
            .join(&release.artifact.url)
            .map_err(|error| UpdateError::InvalidUrl(error.to_string()))?;
        let mut response = client.get(artifact_url).send()?.error_for_status()?;
        let total = response.content_length().or(release.artifact.size);
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

        send_event(sender, UpdateEvent::Verifying)?;
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
        macos::extract_app_archive(&archive_path, &extract_path)?;
        let staged_app = macos::find_app_bundle(&extract_path)?;
        macos::verify_code_signature(&staged_app, config.expected_team_id.as_deref())?;
        let current_app = config
            .app_bundle_path
            .clone()
            .map(Ok)
            .unwrap_or_else(macos::current_app_bundle)?;

        Ok(StagedUpdate {
            release,
            staged_app,
            current_app,
            cleanup: Arc::new(StagingCleanup {
                staging_root: staging_root.clone(),
                installer_started: AtomicBool::new(false),
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

fn create_staging_root(
    config: &UpdateConfig,
    release: &UpdateRelease,
) -> Result<PathBuf, UpdateError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = staging_base(config).join(format!(
        "{}-{}-{}-{timestamp}",
        release.version,
        release.bundle_version,
        std::process::id()
    ));
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn staging_base(config: &UpdateConfig) -> PathBuf {
    std::env::temp_dir()
        .join("xuwe-updater")
        .join(config.app_id())
}

fn start_staging_cleanup_worker() -> Option<mpsc::Sender<PathBuf>> {
    let (sender, receiver) = mpsc::channel();
    match thread::Builder::new()
        .name("xuwe-update-cleanup".to_owned())
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
    let base = staging_base(config);
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

fn format_digest(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            write!(output, "{byte:02x}").expect("写入 String 不会失败");
            output
        },
    )
}

/// 更新配置、网络传输、清单解析、校验或安装阶段可能产生的错误。
#[derive(Debug, Error)]
pub enum UpdateError {
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
    /// 更新服务器使用了客户端尚不支持的协议版本。
    #[error("不支持更新协议版本 {0}")]
    UnsupportedSchema(
        /// 服务端清单声明、但当前客户端无法处理的协议版本。
        u32,
    ),
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
    /// 当前平台尚未实现原位安装。
    #[error("当前平台暂不支持自动安装更新")]
    UnsupportedPlatform,
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
    /// 当前暂存更新已经启动过安装 helper。
    #[error("更新安装已经启动，请等待应用退出并完成替换")]
    InstallerAlreadyStarted,
    /// 用户主动取消了更新。
    #[error("更新已取消")]
    Cancelled,
    /// UI 已经销毁事件接收端，工作线程应停止。
    #[error("更新界面已经关闭")]
    EventReceiverClosed,
}
