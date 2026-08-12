//! 桌面应用身份级单例门禁与重复启动激活 IPC。

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use directories::ProjectDirs;
use fs2::FileExt as _;
use rand::{TryRngCore as _, rngs::OsRng};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

/// 当前单例激活 IPC 协议版本。
pub const PROCESS_PROTOCOL_VERSION: u32 = 2;

const MAX_FRAME_BYTES: usize = 16 * 1024;

/// 一个应用在本机上用于单例与 IPC 目录隔离的稳定身份。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApplicationIdentity(String);

impl ApplicationIdentity {
    /// 根据生产 app ID，或“应用名 + 规范化可执行文件”开发身份构造稳定值。
    ///
    /// `identity_override` 最优先，适合需要显式隔离的测试或白标应用；其次使用
    /// `production_app_id`。两者都缺失时对应用名与可执行文件规范路径做 SHA-256。
    ///
    /// # Errors
    ///
    /// 开发模式下无法取得或规范化可执行文件路径时返回 I/O 错误。
    pub fn resolve(
        application_name: &str,
        production_app_id: Option<&str>,
        identity_override: Option<&str>,
    ) -> Result<Self, ProcessError> {
        if let Some(value) = identity_override.or(production_app_id) {
            return Self::explicit(value);
        }
        let executable = fs::canonicalize(env::current_exe()?)?;
        Self::for_development(application_name, &executable)
    }

    /// 使用调用方提供的可执行文件路径构造可测试的开发身份。
    ///
    /// # Errors
    ///
    /// 可执行文件无法规范化，或应用名无法生成安全的本地身份时返回错误。
    pub fn for_development(
        application_name: &str,
        executable: &Path,
    ) -> Result<Self, ProcessError> {
        let executable = fs::canonicalize(executable)?;
        let mut digest = Sha256::new();
        digest.update(application_name.trim().as_bytes());
        digest.update([0]);
        digest.update(executable.to_string_lossy().as_bytes());
        let hash = hex(&digest.finalize());
        Ok(Self(format!(
            "{}-{}",
            normalize_identity(application_name)?,
            &hash[..24]
        )))
    }

    /// 构造显式稳定身份。
    ///
    /// # Errors
    ///
    /// `value` 为空或规范化后不包含可用字符时返回 [`ProcessError::InvalidIdentity`]。
    pub fn explicit(value: &str) -> Result<Self, ProcessError> {
        Ok(Self(normalize_identity(value)?))
    }

    /// 返回可用作本地目录名的规范值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 主进程从单例 IPC 接收的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorEvent {
    /// 第二次启动请求激活当前进程中的窗口组。
    ActivateGroup,
}

/// 进程协调启动选项。
#[derive(Debug, Clone)]
pub struct ProcessBootstrapOptions {
    /// 应用稳定身份。
    pub identity: ApplicationIdentity,
    /// 是否启用应用身份级单例门禁。
    pub enabled: bool,
    /// 测试可覆盖的运行时目录。
    pub runtime_root: Option<PathBuf>,
}

/// 启动单例门禁后的结果。
pub enum ProcessBootstrap {
    /// 协调功能被应用显式关闭。
    Disabled,
    /// 当前进程是权威主进程。
    Main(
        /// 权威主进程持有的单例协调器。
        MainProcess,
    ),
    /// 已通知现有主进程激活窗口组，当前进程应立即退出。
    SecondaryActivated,
}

/// 主进程对应用身份锁与重复启动激活端点的所有权。
pub struct MainProcess {
    identity: ApplicationIdentity,
    endpoint_path: PathBuf,
    descriptor: EndpointDescriptor,
    stopping: Arc<AtomicBool>,
    events: Receiver<CoordinatorEvent>,
    listener_thread: Option<JoinHandle<()>>,
    lock: File,
}

impl MainProcess {
    /// 非阻塞获取下一个重复启动激活事件。
    pub fn try_recv(&self) -> Option<CoordinatorEvent> {
        self.events.try_recv().ok()
    }

    /// 返回当前主进程的稳定应用身份。
    pub fn identity(&self) -> &ApplicationIdentity {
        &self.identity
    }
}

impl Drop for MainProcess {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        _ = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.descriptor.port));
        if let Some(listener_thread) = self.listener_thread.take() {
            _ = listener_thread.join();
        }
        _ = fs::remove_file(&self.endpoint_path);
        _ = fs2::FileExt::unlock(&self.lock);
    }
}

/// 单例门禁或激活 IPC 错误。
#[derive(Debug, Error)]
pub enum ProcessError {
    /// 文件或本地套接字操作失败。
    #[error("桌面进程协调 I/O 失败: {0}")]
    Io(
        /// 底层文件或套接字 I/O 错误。
        #[from]
        std::io::Error,
    ),
    /// IPC JSON 帧无法序列化或解析。
    #[error("桌面进程协议编码失败: {0}")]
    Json(
        /// JSON 帧序列化或反序列化错误。
        #[from]
        serde_json::Error,
    ),
    /// 应用身份为空或包含不安全字符。
    #[error("桌面应用身份无效: {0}")]
    InvalidIdentity(
        /// 无法规范化为安全目录名的原始身份。
        String,
    ),
    /// 端点认证失败或协议版本不兼容。
    #[error("桌面进程协议失败: {0}")]
    Protocol(
        /// 认证、版本或帧语义不符合协议的脱敏原因。
        String,
    ),
}

/// 执行应用身份级单例门禁。
///
/// 首次启动返回 [`ProcessBootstrap::Main`]；二次启动向已有主进程发送激活命令并返回
/// [`ProcessBootstrap::SecondaryActivated`]。该协议只负责单例与激活，不创建或承载窗口
/// 子进程。
///
/// # Errors
///
/// 运行时目录、跨进程锁、本地端点、权限设置或激活请求失败时返回错误。
pub fn bootstrap(options: ProcessBootstrapOptions) -> Result<ProcessBootstrap, ProcessError> {
    if !options.enabled {
        return Ok(ProcessBootstrap::Disabled);
    }

    let runtime_root = match options.runtime_root {
        Some(path) => path,
        None => default_runtime_root(&options.identity)?,
    };
    fs::create_dir_all(&runtime_root)?;
    restrict_directory(&runtime_root)?;
    let lock_path = runtime_root.join("primary.lock");
    let lock = secure_file(&lock_path)?;
    match lock.try_lock_exclusive() {
        Ok(()) => bootstrap_main(options.identity, runtime_root, lock).map(ProcessBootstrap::Main),
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            activate_existing(&runtime_root)?;
            Ok(ProcessBootstrap::SecondaryActivated)
        }
        Err(error) => Err(error.into()),
    }
}

fn activate_existing(runtime_root: &Path) -> Result<(), ProcessError> {
    let descriptor = read_endpoint_with_retry(&runtime_root.join("endpoint.json"))?;
    match send_request(
        &descriptor,
        &WireRequest::Activate {
            protocol_version: PROCESS_PROTOCOL_VERSION,
            endpoint_secret: descriptor.secret.clone(),
        },
    )? {
        WireResponse::Acknowledged { .. } => Ok(()),
        WireResponse::Error { message, .. } => Err(ProcessError::Protocol(message)),
    }
}

fn bootstrap_main(
    identity: ApplicationIdentity,
    runtime_root: PathBuf,
    lock: File,
) -> Result<MainProcess, ProcessError> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let descriptor = EndpointDescriptor {
        protocol_version: PROCESS_PROTOCOL_VERSION,
        port: listener.local_addr()?.port(),
        secret: random_secret()?,
        pid: std::process::id(),
    };
    let endpoint_path = runtime_root.join("endpoint.json");
    write_secure_json(&endpoint_path, &descriptor)?;
    let (event_sender, events) = mpsc::channel();
    let stopping = Arc::new(AtomicBool::new(false));
    let listener_stopping = stopping.clone();
    let listener_descriptor = descriptor.clone();
    let listener_thread = thread::Builder::new()
        .name("nexora-instance-activation".to_owned())
        .spawn(move || {
            serve(
                listener,
                listener_descriptor,
                listener_stopping,
                event_sender,
            );
        })?;

    Ok(MainProcess {
        identity,
        endpoint_path,
        descriptor,
        stopping,
        events,
        listener_thread: Some(listener_thread),
        lock,
    })
}

fn serve(
    listener: TcpListener,
    descriptor: EndpointDescriptor,
    stopping: Arc<AtomicBool>,
    events: Sender<CoordinatorEvent>,
) {
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let response = read_frame::<WireRequest>(&mut stream)
                    .map(|request| handle_request(request, &descriptor, &events))
                    .unwrap_or_else(|error| WireResponse::error(error.to_string()));
                if let Err(error) = write_frame(&mut stream, &response) {
                    tracing::warn!(error = %error, "无法写回桌面单例 IPC 响应");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                tracing::warn!(error = %error, "桌面单例 IPC 监听器停止");
                break;
            }
        }
    }
}

fn handle_request(
    request: WireRequest,
    descriptor: &EndpointDescriptor,
    events: &Sender<CoordinatorEvent>,
) -> WireResponse {
    match request {
        WireRequest::Activate {
            protocol_version,
            endpoint_secret,
        } => {
            if protocol_version != PROCESS_PROTOCOL_VERSION {
                return WireResponse::error(format!(
                    "IPC 协议版本不兼容：收到 {protocol_version}，期望 {PROCESS_PROTOCOL_VERSION}"
                ));
            }
            if endpoint_secret != descriptor.secret {
                return WireResponse::error("IPC 端点认证失败".to_owned());
            }
            _ = events.send(CoordinatorEvent::ActivateGroup);
            WireResponse::Acknowledged {
                protocol_version: PROCESS_PROTOCOL_VERSION,
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct EndpointDescriptor {
    protocol_version: u32,
    port: u16,
    secret: String,
    pid: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WireRequest {
    Activate {
        protocol_version: u32,
        endpoint_secret: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WireResponse {
    Acknowledged {
        protocol_version: u32,
    },
    Error {
        protocol_version: u32,
        message: String,
    },
}

impl WireResponse {
    fn error(message: String) -> Self {
        Self::Error {
            protocol_version: PROCESS_PROTOCOL_VERSION,
            message,
        }
    }
}

fn read_endpoint_with_retry(path: &Path) -> Result<EndpointDescriptor, ProcessError> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match read_json(path) {
            Ok(descriptor) => return Ok(descriptor),
            Err(ProcessError::Io(_) | ProcessError::Json(_)) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

fn send_request(
    endpoint: &EndpointDescriptor,
    request: &WireRequest,
) -> Result<WireResponse, ProcessError> {
    if endpoint.protocol_version != PROCESS_PROTOCOL_VERSION {
        return Err(ProcessError::Protocol(format!(
            "端点协议版本 {} 与当前版本 {} 不兼容",
            endpoint.protocol_version, PROCESS_PROTOCOL_VERSION
        )));
    }
    let mut stream = TcpStream::connect_timeout(
        &SocketAddrV4::new(Ipv4Addr::LOCALHOST, endpoint.port).into(),
        Duration::from_secs(2),
    )?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    write_frame(&mut stream, request)?;
    read_frame(&mut stream)
}

fn write_frame<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<(), ProcessError> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(ProcessError::Protocol(format!(
            "IPC 帧超过 {MAX_FRAME_BYTES} 字节限制"
        )));
    }
    stream.write_all(&bytes)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn read_frame<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<T, ProcessError> {
    let mut reader = BufReader::new(stream);
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(ProcessError::Protocol(format!(
            "IPC 帧超过 {MAX_FRAME_BYTES} 字节限制"
        )));
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err(ProcessError::Protocol("收到空 IPC 帧".to_owned()));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn normalize_identity(value: &str) -> Result<String, ProcessError> {
    let normalized = value
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let normalized = normalized.trim_matches('-').to_owned();
    if normalized.is_empty() {
        return Err(ProcessError::InvalidIdentity(value.to_owned()));
    }
    Ok(normalized)
}

fn default_runtime_root(identity: &ApplicationIdentity) -> Result<PathBuf, ProcessError> {
    let directories = ProjectDirs::from("com", "Nexora", identity.as_str()).ok_or_else(|| {
        ProcessError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "无法确定应用本地运行时目录",
        ))
    })?;
    Ok(directories.data_local_dir().join("runtime"))
}

fn random_secret() -> Result<String, ProcessError> {
    let mut bytes = [0_u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(hex(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        _ = write!(output, "{byte:02x}");
    }
    output
}

fn secure_file(path: &Path) -> Result<File, ProcessError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    restrict_file(path)?;
    Ok(file)
}

fn write_secure_json(path: &Path, value: &impl Serialize) -> Result<(), ProcessError> {
    let mut file = secure_file(path)?;
    file.set_len(0)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, ProcessError> {
    let file = File::open(path)?;
    Ok(serde_json::from_reader(file)?)
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<(), ProcessError> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<(), ProcessError> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<(), ProcessError> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> Result<(), ProcessError> {
    Ok(())
}
