//! 桌面应用单例门禁、子进程启动与版本化本地 IPC。

use std::{
    collections::HashMap,
    env,
    fs::{self, File, OpenOptions},
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
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

/// 当前 IPC 协议版本。
pub const PROCESS_PROTOCOL_VERSION: u32 = 1;
/// Nexora 内部进程角色参数前缀。
pub const PROCESS_ROLE_ARGUMENT: &str = "--nexora-process-role=";
/// Nexora 内部窗口会话参数前缀。
pub const SESSION_ID_ARGUMENT: &str = "--nexora-session-id=";
/// Nexora 内部一次性握手凭据参数前缀。
pub const HANDSHAKE_ARGUMENT: &str = "--nexora-handshake=";
/// Nexora 内部端点标识参数前缀。
pub const ENDPOINT_ARGUMENT: &str = "--nexora-endpoint=";

const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// 顶层桌面窗口对应的进程角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRole {
    /// 唯一主进程与主 Shell 窗口。
    Main,
    /// 拥有完整 Shell 的子进程窗口。
    Shell,
    /// 应用级唯一设置子进程窗口。
    Settings,
    /// 派生注册的业务 Window 子进程。
    Registered,
}

impl ProcessRole {
    fn argument_value(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Shell => "shell",
            Self::Settings => "settings",
            Self::Registered => "registered",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "main" => Some(Self::Main),
            "shell" => Some(Self::Shell),
            "settings" => Some(Self::Settings),
            "registered" => Some(Self::Registered),
            _ => None,
        }
    }
}

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

/// 子进程完成安全握手后由主进程下发的实际启动载荷。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct WindowStartupPayload {
    /// 子进程角色。
    pub role: ProcessRole,
    /// 稳定窗口会话 ID。
    pub session_id: String,
    /// 启动时优先使用的显示器 UUID。
    pub display_uuid: Option<String>,
    /// Shell 初始完整路由，或 Registered Window 完整路由。
    pub location: Option<String>,
    /// Registered Window 的稳定类型 ID。
    pub window_type_id: Option<String>,
    /// 由上层定义的版本化会话数据；只通过受保护 IPC 传输。
    pub session: serde_json::Value,
}

/// 子进程向主进程发送的窗口和偏好意图。
#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChildCommand {
    /// 子进程心跳。
    Heartbeat,
    /// 请求主进程创建或定位窗口。
    CreateWindow {
        /// 仅在安全握手后交给目标子进程的类型化启动载荷。
        payload: WindowStartupPayload,
    },
    /// 请求激活整个窗口组。
    ActivateGroup,
    /// 请求隐藏整个窗口组。
    HideGroup,
    /// 提交窗口会话 patch。
    WindowSessionPatch {
        /// 上层窗口会话协调器定义的版本化 patch。
        patch: serde_json::Value,
    },
    /// 提交普通偏好字段 patch。
    PreferencePatch {
        /// 上层偏好协调器定义的字段级 patch。
        patch: serde_json::Value,
    },
    /// 提交 DataTable 列布局 patch。
    DataTableLayoutPatch {
        /// 上层表格布局协调器定义的列布局 patch。
        patch: serde_json::Value,
    },
    /// 通过认证 IPC 同步账号内存状态；可能包含短期凭据，但不会持久化或进入日志。
    AccountState {
        /// 不落盘、不写入命令行且不得记录的账号内存状态。
        state: serde_json::Value,
    },
    /// 用户单独关闭子窗口。
    WindowClosed,
}

/// 主进程从 IPC 事件队列接收的类型化事件。
#[derive(Clone, PartialEq)]
pub enum CoordinatorEvent {
    /// 第二次启动或子进程请求激活窗口组。
    ActivateGroup,
    /// 子进程提交了经过认证的命令。
    ChildCommand {
        /// 发起命令的窗口会话。
        session_id: String,
        /// 命令内容。
        command: ChildCommand,
    },
}

/// 整体退出命令使用的原因，用于区分用户单独关窗和应用退出。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    /// 主窗口确认退出。
    ApplicationExit,
    /// 托盘菜单确认退出。
    TrayExit,
    /// 更新器协调重启。
    RestartForUpdate,
}

/// 主进程随子进程心跳响应下发的一次性窗口指令。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParentWindowCommand {
    /// 恢复并激活该子进程的窗口。
    Activate,
    /// 隐藏该子进程的全部窗口，同时保持进程常驻。
    Hide,
    /// 最小化该子进程的全部窗口。
    Minimize,
}

/// 子进程发送命令后收到的主进程状态。
#[derive(Clone, PartialEq)]
pub struct ParentState {
    /// 整体退出原因；存在时子进程应保留会话快照并退出。
    pub exit_reason: Option<ExitReason>,
    /// 需要在当前子进程窗口上执行的指令。
    pub window_command: Option<ParentWindowCommand>,
    /// 主进程最新持久偏好快照；子进程只应在修订号更新时替换本地快照。
    pub preference_broadcast: Option<serde_json::Value>,
    /// 主进程最新账号内存快照；可能含短期凭据，因此该类型刻意不实现 `Debug`。
    pub account_broadcast: Option<serde_json::Value>,
}

/// 进程协调启动选项。
#[derive(Debug, Clone)]
pub struct ProcessBootstrapOptions {
    /// 应用稳定身份。
    pub identity: ApplicationIdentity,
    /// 是否启用应用单例与子进程 IPC。
    pub enabled: bool,
    /// 测试可覆盖的运行时目录。
    pub runtime_root: Option<PathBuf>,
}

/// 启动单例门禁后得到的当前进程角色。
pub enum ProcessBootstrap {
    /// 协调功能被应用显式关闭。
    Disabled,
    /// 当前进程是权威主进程。
    Main(
        /// 权威主进程持有的协调器。
        MainProcess,
    ),
    /// 当前进程是完成握手的窗口子进程。
    Child(
        /// 已认证窗口子进程持有的 IPC 客户端。
        ChildProcess,
    ),
    /// 已通知现有主进程激活窗口组，当前进程应立即退出。
    SecondaryActivated,
}

/// 主进程对本地 IPC、一次性启动载荷和子进程句柄的所有权。
pub struct MainProcess {
    identity: ApplicationIdentity,
    endpoint_path: PathBuf,
    descriptor: EndpointDescriptor,
    state: Arc<ServerState>,
    events: Receiver<CoordinatorEvent>,
    listener_thread: Option<JoinHandle<()>>,
    lock: File,
    children: Mutex<Vec<ManagedChild>>,
}

impl MainProcess {
    /// 非阻塞获取下一个子进程或二次启动事件。
    pub fn try_recv(&self) -> Option<CoordinatorEvent> {
        self.events.try_recv().ok()
    }

    /// 返回当前主进程的稳定应用身份。
    pub fn identity(&self) -> &ApplicationIdentity {
        &self.identity
    }

    /// 注册一次性启动载荷并启动同一可执行文件的受管子进程。
    ///
    /// 命令行仅包含角色、会话 ID、端点标识和一次性凭据；完整路由与会话数据
    /// 只保留在主进程内存，子进程完成握手后才能读取。
    ///
    /// # Errors
    ///
    /// 随机凭据生成、当前可执行文件查找或子进程启动失败时返回错误。
    ///
    /// # Panics
    ///
    /// 仅当进程协调器内部互斥锁已经被其他线程 panic 污染时 panic。
    pub fn spawn_window(&self, payload: WindowStartupPayload) -> Result<u32, ProcessError> {
        if payload.role == ProcessRole::Main {
            return Err(ProcessError::InvalidPayload(
                "子进程启动载荷不能使用 main 角色".to_owned(),
            ));
        }
        if payload.session_id.trim().is_empty() {
            return Err(ProcessError::InvalidPayload(
                "窗口会话 ID 不能为空".to_owned(),
            ));
        }
        {
            let mut children = self.children.lock().expect("子进程句柄锁不应中毒");
            children.retain_mut(|managed| !matches!(managed.child.try_wait(), Ok(Some(_))));
            if let Some(existing) = children
                .iter()
                .find(|managed| managed.session_id == payload.session_id)
            {
                self.state
                    .window_commands
                    .lock()
                    .expect("子进程窗口指令锁不应中毒")
                    .insert(payload.session_id, ParentWindowCommand::Activate);
                return Ok(existing.child.id());
            }
        }
        let credential = random_secret()?;
        self.state
            .pending_payloads
            .lock()
            .expect("子进程启动载荷锁不应中毒")
            .insert(credential.clone(), payload.clone());

        let executable = env::current_exe()?;
        let mut command = Command::new(executable);
        command
            .arg(format!(
                "{PROCESS_ROLE_ARGUMENT}{}",
                payload.role.argument_value()
            ))
            .arg(format!("{SESSION_ID_ARGUMENT}{}", payload.session_id))
            .arg(format!("{HANDSHAKE_ARGUMENT}{credential}"))
            .arg(format!(
                "{ENDPOINT_ARGUMENT}{}",
                self.endpoint_path.to_string_lossy()
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match command.spawn() {
            Ok(child) => {
                let pid = child.id();
                self.children
                    .lock()
                    .expect("子进程句柄锁不应中毒")
                    .push(ManagedChild {
                        session_id: payload.session_id,
                        child,
                    });
                Ok(pid)
            }
            Err(error) => {
                self.state
                    .pending_payloads
                    .lock()
                    .expect("子进程启动载荷锁不应中毒")
                    .remove(&credential);
                Err(error.into())
            }
        }
    }

    /// 开始整体退出，使后续子进程心跳收到带原因的退出命令。
    ///
    /// # Panics
    ///
    /// 仅当进程协调器内部退出状态锁已经被其他线程 panic 污染时 panic。
    pub fn begin_shutdown(&self, reason: ExitReason) {
        *self.state.exit_reason.lock().expect("退出状态锁不应中毒") = Some(reason);
    }

    /// 请求全部已认证子进程在下一次心跳时最小化窗口。
    pub fn minimize_window_group(&self) {
        self.queue_window_command_for_all(ParentWindowCommand::Minimize);
    }

    /// 请求全部已认证子进程在下一次心跳时隐藏窗口。
    pub fn hide_window_group(&self) {
        self.queue_window_command_for_all(ParentWindowCommand::Hide);
    }

    /// 请求全部已认证子进程在下一次心跳时恢复并激活窗口。
    pub fn activate_window_group(&self) {
        self.queue_window_command_for_all(ParentWindowCommand::Activate);
    }

    /// 保存待随心跳广播给全部子进程的最新偏好快照。
    ///
    /// # Panics
    ///
    /// 仅当进程协调器内部广播状态锁已经被其他线程 panic 污染时 panic。
    pub fn broadcast_preferences(&self, preferences: serde_json::Value) {
        *self
            .state
            .preference_broadcast
            .lock()
            .expect("偏好广播锁不应中毒") = Some(preferences);
    }

    /// 保存待随心跳广播给全部子进程的账号内存快照。
    ///
    /// 该值只保留在主进程内存和认证 IPC 帧中；调用方不得记录其内容或写入偏好文件。
    ///
    /// # Panics
    ///
    /// 仅当进程协调器内部账号广播锁已经被其他线程 panic 污染时 panic。
    pub fn broadcast_account_state(&self, state: serde_json::Value) {
        *self
            .state
            .account_broadcast
            .lock()
            .expect("账号广播锁不应中毒") = Some(state);
    }

    fn queue_window_command_for_all(&self, command: ParentWindowCommand) {
        let session_ids = self
            .state
            .sessions
            .lock()
            .expect("子进程会话锁不应中毒")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut commands = self
            .state
            .window_commands
            .lock()
            .expect("子进程窗口指令锁不应中毒");
        for session_id in session_ids {
            commands.insert(session_id, command);
        }
    }

    /// 等待受管子进程优雅退出，超时后终止仍存活的进程。
    ///
    /// # Panics
    ///
    /// 仅当受管子进程句柄锁已经被其他线程 panic 污染时 panic。
    pub fn wait_for_children(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            let mut children = self.children.lock().expect("子进程句柄锁不应中毒");
            children.retain_mut(|managed| match managed.child.try_wait() {
                Ok(Some(_)) => false,
                Ok(None) => true,
                Err(error) => {
                    tracing::warn!(session_id = %managed.session_id, error = %error, "无法查询子进程状态");
                    true
                }
            });
            if children.is_empty() {
                return;
            }
            if Instant::now() >= deadline {
                for managed in children.iter_mut() {
                    if let Err(error) = managed.child.kill() {
                        tracing::warn!(session_id = %managed.session_id, error = %error, "无法终止超时子进程");
                    }
                    _ = managed.child.wait();
                }
                children.clear();
                return;
            }
            drop(children);
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for MainProcess {
    fn drop(&mut self) {
        self.state.stopping.store(true, Ordering::Release);
        _ = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.descriptor.port));
        if let Some(listener_thread) = self.listener_thread.take() {
            _ = listener_thread.join();
        }
        _ = fs::remove_file(&self.endpoint_path);
        _ = fs2::FileExt::unlock(&self.lock);
    }
}

/// 完成一次性握手的子进程客户端。
pub struct ChildProcess {
    endpoint: EndpointDescriptor,
    session_token: String,
    payload: WindowStartupPayload,
}

impl ChildProcess {
    /// 返回主进程通过安全 IPC 下发的完整启动载荷。
    pub fn payload(&self) -> &WindowStartupPayload {
        &self.payload
    }

    /// 向主进程发送类型化命令并读取整体退出状态。
    ///
    /// # Errors
    ///
    /// 主进程端点消失、网络帧无法收发、会话认证失败或协议版本不兼容时
    /// 返回错误。子进程应在连续心跳失败后保存最后快照并退出。
    pub fn send(&self, command: ChildCommand) -> Result<ParentState, ProcessError> {
        let request = WireRequest::Authenticated {
            protocol_version: PROCESS_PROTOCOL_VERSION,
            endpoint_secret: self.endpoint.secret.clone(),
            session_id: self.payload.session_id.clone(),
            session_token: self.session_token.clone(),
            command,
        };
        match send_request(&self.endpoint, &request)? {
            WireResponse::Acknowledged {
                exit_reason,
                window_command,
                preference_broadcast,
                account_broadcast,
                ..
            } => Ok(ParentState {
                exit_reason,
                window_command,
                preference_broadcast,
                account_broadcast,
            }),
            WireResponse::Error { message, .. } => Err(ProcessError::Protocol(message)),
            _ => Err(ProcessError::Protocol("主进程返回了非预期响应".to_owned())),
        }
    }
}

/// 单例门禁、启动载荷或 IPC 协议错误。
#[derive(Debug, Error)]
pub enum ProcessError {
    /// 文件、套接字或子进程操作失败。
    #[error("桌面进程协调 I/O 失败: {0}")]
    Io(
        /// 底层文件、套接字或进程 I/O 错误。
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
    /// 子进程启动载荷不满足结构约束。
    #[error("子进程启动载荷无效: {0}")]
    InvalidPayload(
        /// 启动载荷不满足约束的脱敏原因。
        String,
    ),
    /// 端点过期、认证失败或协议版本不兼容。
    #[error("桌面进程协议失败: {0}")]
    Protocol(
        /// 认证、版本或帧语义不符合协议的脱敏原因。
        String,
    ),
    /// 进程参数声明了不完整的子进程启动信息。
    #[error("Nexora 内部子进程参数不完整")]
    IncompleteChildArguments,
}

/// 执行单例门禁和子进程安全握手。
///
/// 首次启动返回 [`ProcessBootstrap::Main`]；二次启动向已有主进程发送激活
/// 命令并返回 [`ProcessBootstrap::SecondaryActivated`]。只有由主进程生成的完整内部
/// 参数才会进入子进程握手。
///
/// # Errors
///
/// 运行时目录、跨进程锁、本地端点、权限设置或安全握手失败时返回错误。
pub fn bootstrap(options: ProcessBootstrapOptions) -> Result<ProcessBootstrap, ProcessError> {
    if !options.enabled {
        return Ok(ProcessBootstrap::Disabled);
    }
    if let Some(arguments) = InternalChildArguments::from_env()? {
        return bootstrap_child(arguments).map(ProcessBootstrap::Child);
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
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            let endpoint_path = runtime_root.join("endpoint.json");
            let descriptor = read_endpoint_with_retry(&endpoint_path)?;
            let response = send_request(
                &descriptor,
                &WireRequest::Activate {
                    protocol_version: PROCESS_PROTOCOL_VERSION,
                    endpoint_secret: descriptor.secret.clone(),
                },
            )?;
            match response {
                WireResponse::Acknowledged { .. } => Ok(ProcessBootstrap::SecondaryActivated),
                WireResponse::Error { message, .. } => Err(ProcessError::Protocol(message)),
                _ => Err(ProcessError::Protocol("主进程返回了非预期响应".to_owned())),
            }
        }
        Err(error) => Err(error.into()),
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

/// 返回某个命令行参数是否属于 Nexora 内部子进程启动协议。
///
/// 配置文件参数解析必须先排除这些值，以免把角色或握手凭据误当成路径。
pub fn is_internal_process_argument(argument: &str) -> bool {
    [
        PROCESS_ROLE_ARGUMENT,
        SESSION_ID_ARGUMENT,
        HANDSHAKE_ARGUMENT,
        ENDPOINT_ARGUMENT,
    ]
    .iter()
    .any(|prefix| argument.starts_with(prefix))
}

fn bootstrap_main(
    identity: ApplicationIdentity,
    runtime_root: PathBuf,
    lock: File,
) -> Result<MainProcess, ProcessError> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let descriptor = EndpointDescriptor {
        protocol_version: PROCESS_PROTOCOL_VERSION,
        port,
        secret: random_secret()?,
        pid: std::process::id(),
    };
    let endpoint_path = runtime_root.join("endpoint.json");
    write_secure_json(&endpoint_path, &descriptor)?;
    let (event_sender, events) = mpsc::channel();
    let state = Arc::new(ServerState::default());
    let listener_state = state.clone();
    let listener_descriptor = descriptor.clone();
    let listener_thread = thread::Builder::new()
        .name("nexora-process-coordinator".to_owned())
        .spawn(move || {
            serve(listener, listener_descriptor, listener_state, event_sender);
        })?;

    Ok(MainProcess {
        identity,
        endpoint_path,
        descriptor,
        state,
        events,
        listener_thread: Some(listener_thread),
        lock,
        children: Mutex::new(Vec::new()),
    })
}

fn bootstrap_child(arguments: InternalChildArguments) -> Result<ChildProcess, ProcessError> {
    let endpoint: EndpointDescriptor = read_json(&arguments.endpoint_path)?;
    let request = WireRequest::Handshake {
        protocol_version: PROCESS_PROTOCOL_VERSION,
        endpoint_secret: endpoint.secret.clone(),
        credential: arguments.credential,
        role: arguments.role,
        session_id: arguments.session_id,
    };
    match send_request(&endpoint, &request)? {
        WireResponse::Startup {
            protocol_version,
            session_token,
            payload,
        } if protocol_version == PROCESS_PROTOCOL_VERSION => Ok(ChildProcess {
            endpoint,
            session_token,
            payload,
        }),
        WireResponse::Error { message, .. } => Err(ProcessError::Protocol(message)),
        _ => Err(ProcessError::Protocol("主进程未返回启动载荷".to_owned())),
    }
}

fn serve(
    listener: TcpListener,
    descriptor: EndpointDescriptor,
    state: Arc<ServerState>,
    events: Sender<CoordinatorEvent>,
) {
    while !state.stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let response = read_frame::<WireRequest>(&mut stream)
                    .map(|request| handle_request(request, &descriptor, &state, &events))
                    .unwrap_or_else(|error| WireResponse::error(error.to_string()));
                if let Err(error) = write_frame(&mut stream, &response) {
                    tracing::warn!(error = %error, "无法写回桌面进程 IPC 响应");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                tracing::warn!(error = %error, "桌面进程 IPC 监听器停止");
                break;
            }
        }
    }
}

fn handle_request(
    request: WireRequest,
    descriptor: &EndpointDescriptor,
    state: &ServerState,
    events: &Sender<CoordinatorEvent>,
) -> WireResponse {
    if request.protocol_version() != PROCESS_PROTOCOL_VERSION {
        return WireResponse::error(format!(
            "IPC 协议版本不兼容：收到 {}，期望 {}",
            request.protocol_version(),
            PROCESS_PROTOCOL_VERSION
        ));
    }
    if request.endpoint_secret() != descriptor.secret {
        return WireResponse::error("IPC 端点认证失败".to_owned());
    }
    match request {
        WireRequest::Activate { .. } => {
            let session_ids = state
                .sessions
                .lock()
                .expect("子进程会话锁不应中毒")
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            let mut commands = state
                .window_commands
                .lock()
                .expect("子进程窗口指令锁不应中毒");
            for session_id in session_ids {
                commands.insert(session_id, ParentWindowCommand::Activate);
            }
            _ = events.send(CoordinatorEvent::ActivateGroup);
            WireResponse::acknowledged(None, None, None, None)
        }
        WireRequest::Handshake {
            credential,
            role,
            session_id,
            ..
        } => {
            let payload = state
                .pending_payloads
                .lock()
                .expect("子进程启动载荷锁不应中毒")
                .remove(&credential);
            let Some(payload) = payload else {
                return WireResponse::error("一次性子进程握手凭据无效或已使用".to_owned());
            };
            if payload.role != role || payload.session_id != session_id {
                return WireResponse::error("子进程角色或会话与启动载荷不匹配".to_owned());
            }
            let session_token = match random_secret() {
                Ok(token) => token,
                Err(error) => return WireResponse::error(error.to_string()),
            };
            state
                .sessions
                .lock()
                .expect("子进程会话锁不应中毒")
                .insert(session_id, session_token.clone());
            WireResponse::Startup {
                protocol_version: PROCESS_PROTOCOL_VERSION,
                session_token,
                payload,
            }
        }
        WireRequest::Authenticated {
            session_id,
            session_token,
            command,
            ..
        } => {
            let authenticated = state
                .sessions
                .lock()
                .expect("子进程会话锁不应中毒")
                .get(&session_id)
                .is_some_and(|expected| expected == &session_token);
            if !authenticated {
                return WireResponse::error("子进程会话认证失败".to_owned());
            }
            if matches!(command, ChildCommand::WindowClosed) {
                state
                    .sessions
                    .lock()
                    .expect("子进程会话锁不应中毒")
                    .remove(&session_id);
            }
            if matches!(command, ChildCommand::ActivateGroup) {
                _ = events.send(CoordinatorEvent::ActivateGroup);
            } else {
                _ = events.send(CoordinatorEvent::ChildCommand {
                    session_id: session_id.clone(),
                    command,
                });
            }
            let exit_reason = *state.exit_reason.lock().expect("退出状态锁不应中毒");
            let window_command = state
                .window_commands
                .lock()
                .expect("子进程窗口指令锁不应中毒")
                .remove(&session_id);
            let preference_broadcast = state
                .preference_broadcast
                .lock()
                .expect("偏好广播锁不应中毒")
                .clone();
            let account_broadcast = state
                .account_broadcast
                .lock()
                .expect("账号广播锁不应中毒")
                .clone();
            WireResponse::acknowledged(
                exit_reason,
                window_command,
                preference_broadcast,
                account_broadcast,
            )
        }
    }
}

#[derive(Default)]
struct ServerState {
    pending_payloads: Mutex<HashMap<String, WindowStartupPayload>>,
    sessions: Mutex<HashMap<String, String>>,
    exit_reason: Mutex<Option<ExitReason>>,
    window_commands: Mutex<HashMap<String, ParentWindowCommand>>,
    preference_broadcast: Mutex<Option<serde_json::Value>>,
    account_broadcast: Mutex<Option<serde_json::Value>>,
    stopping: AtomicBool,
}

struct ManagedChild {
    session_id: String,
    child: Child,
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
    Handshake {
        protocol_version: u32,
        endpoint_secret: String,
        credential: String,
        role: ProcessRole,
        session_id: String,
    },
    Authenticated {
        protocol_version: u32,
        endpoint_secret: String,
        session_id: String,
        session_token: String,
        command: ChildCommand,
    },
}

impl WireRequest {
    fn protocol_version(&self) -> u32 {
        match self {
            Self::Activate {
                protocol_version, ..
            }
            | Self::Handshake {
                protocol_version, ..
            }
            | Self::Authenticated {
                protocol_version, ..
            } => *protocol_version,
        }
    }

    fn endpoint_secret(&self) -> &str {
        match self {
            Self::Activate {
                endpoint_secret, ..
            }
            | Self::Handshake {
                endpoint_secret, ..
            }
            | Self::Authenticated {
                endpoint_secret, ..
            } => endpoint_secret,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WireResponse {
    Acknowledged {
        protocol_version: u32,
        exit_reason: Option<ExitReason>,
        window_command: Option<ParentWindowCommand>,
        preference_broadcast: Option<serde_json::Value>,
        account_broadcast: Option<serde_json::Value>,
    },
    Startup {
        protocol_version: u32,
        session_token: String,
        payload: WindowStartupPayload,
    },
    Error {
        protocol_version: u32,
        message: String,
    },
}

impl WireResponse {
    fn acknowledged(
        exit_reason: Option<ExitReason>,
        window_command: Option<ParentWindowCommand>,
        preference_broadcast: Option<serde_json::Value>,
        account_broadcast: Option<serde_json::Value>,
    ) -> Self {
        Self::Acknowledged {
            protocol_version: PROCESS_PROTOCOL_VERSION,
            exit_reason,
            window_command,
            preference_broadcast,
            account_broadcast,
        }
    }

    fn error(message: String) -> Self {
        Self::Error {
            protocol_version: PROCESS_PROTOCOL_VERSION,
            message,
        }
    }
}

struct InternalChildArguments {
    role: ProcessRole,
    session_id: String,
    credential: String,
    endpoint_path: PathBuf,
}

impl InternalChildArguments {
    fn from_env() -> Result<Option<Self>, ProcessError> {
        let mut role = None;
        let mut session_id = None;
        let mut credential = None;
        let mut endpoint_path = None;
        for argument in env::args().skip(1) {
            if let Some(value) = argument.strip_prefix(PROCESS_ROLE_ARGUMENT) {
                role = ProcessRole::parse(value);
            } else if let Some(value) = argument.strip_prefix(SESSION_ID_ARGUMENT) {
                session_id = Some(value.to_owned());
            } else if let Some(value) = argument.strip_prefix(HANDSHAKE_ARGUMENT) {
                credential = Some(value.to_owned());
            } else if let Some(value) = argument.strip_prefix(ENDPOINT_ARGUMENT) {
                endpoint_path = Some(PathBuf::from(value));
            }
        }
        let any = role.is_some()
            || session_id.is_some()
            || credential.is_some()
            || endpoint_path.is_some();
        if !any {
            return Ok(None);
        }
        Ok(Some(Self {
            role: role.ok_or(ProcessError::IncompleteChildArguments)?,
            session_id: session_id.ok_or(ProcessError::IncompleteChildArguments)?,
            credential: credential.ok_or(ProcessError::IncompleteChildArguments)?,
            endpoint_path: endpoint_path.ok_or(ProcessError::IncompleteChildArguments)?,
        }))
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
