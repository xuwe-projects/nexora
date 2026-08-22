//! updater 主进程与 sidecar 共用的有界操作日志。

use std::{
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use fs2::FileExt as _;

pub(crate) const LOG_DIRECTORY_NAME: &str = "logs";
pub(crate) const MAX_LOG_SESSIONS: usize = 10;
pub(crate) const MAX_LOG_SESSION_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) enum OperationLogRole {
    Main,
    Sidecar,
}

impl OperationLogRole {
    const fn file_name(self) -> &'static str {
        match self {
            Self::Main => "main.log",
            Self::Sidecar => "sidecar.log",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Sidecar => "sidecar",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OperationLog {
    session_id: String,
    session_directory: PathBuf,
    role: OperationLogRole,
}

impl OperationLog {
    pub(crate) fn start_best_effort(cache_directory: &Path) -> Option<Self> {
        match Self::create(cache_directory) {
            Ok(log) => Some(log),
            Err(error) => {
                tracing::warn!(error = %error, "无法创建 updater 操作日志，本次更新继续执行");
                None
            }
        }
    }

    fn create(cache_directory: &Path) -> io::Result<Self> {
        let root = cache_directory.join(LOG_DIRECTORY_NAME);
        fs::create_dir_all(&root)?;
        let rotation_lock = open_lock(&root.join(".rotation.lock"))?;
        rotation_lock.lock_exclusive()?;

        let timestamp = now_millis();
        let mut random = [0_u8; 12];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let session_id = format!("{timestamp:013}-{}", URL_SAFE_NO_PAD.encode(random));
        let session_directory = root.join(&session_id);
        fs::create_dir(&session_directory)?;
        rotate_sessions(&root, &session_id);
        _ = fs2::FileExt::unlock(&rotation_lock);

        Ok(Self {
            session_id,
            session_directory,
            role: OperationLogRole::Main,
        })
    }

    pub(crate) fn open_sidecar_best_effort(
        cache_directory: &Path,
        session_id: &str,
    ) -> Option<Self> {
        Self::open_best_effort(cache_directory, session_id, OperationLogRole::Sidecar)
    }

    pub(crate) fn open_main_best_effort(cache_directory: &Path, session_id: &str) -> Option<Self> {
        Self::open_best_effort(cache_directory, session_id, OperationLogRole::Main)
    }

    fn open_best_effort(
        cache_directory: &Path,
        session_id: &str,
        role: OperationLogRole,
    ) -> Option<Self> {
        if !valid_session_id(session_id) {
            tracing::warn!("忽略格式无效的 updater 日志会话标识");
            return None;
        }
        let session_directory = cache_directory.join(LOG_DIRECTORY_NAME).join(session_id);
        if !session_directory.is_dir() {
            tracing::warn!("updater 日志会话目录不存在，sidecar 继续执行");
            return None;
        }
        Some(Self {
            session_id: session_id.to_owned(),
            session_directory,
            role,
        })
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn write(&self, message: &str) {
        if let Err(error) = self.write_inner(message) {
            tracing::warn!(error = %error, "无法写入 updater 操作日志，本次更新继续执行");
        }
    }

    fn write_inner(&self, message: &str) -> io::Result<()> {
        let lock = open_lock(&self.session_directory.join(".lock"))?;
        lock.lock_exclusive()?;

        let used = session_size(&self.session_directory)?;
        if used >= MAX_LOG_SESSION_BYTES {
            _ = fs2::FileExt::unlock(&lock);
            return Ok(());
        }

        let timestamp = now_millis();
        let sanitized = sanitize_message(message);
        let line = format!(
            "{}.{:03} {} {}\n",
            timestamp / 1000,
            timestamp % 1000,
            self.role.label(),
            sanitized
        );
        let remaining = usize::try_from(MAX_LOG_SESSION_BYTES - used).unwrap_or(usize::MAX);
        let bytes = truncate_utf8(line.as_bytes(), remaining);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.session_directory.join(self.role.file_name()))?;
        file.write_all(bytes)?;
        _ = fs2::FileExt::unlock(&lock);
        Ok(())
    }
}

fn open_lock(path: &Path) -> io::Result<std::fs::File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}

fn rotate_sessions(root: &Path, current_session: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut sessions = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let name = entry.file_name().into_string().ok()?;
            (file_type.is_dir() && valid_session_id(&name)).then_some((name, entry.path()))
        })
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| left.0.cmp(&right.0));
    let remove_count = sessions.len().saturating_sub(MAX_LOG_SESSIONS);
    for (_, path) in sessions
        .into_iter()
        .filter(|(name, _)| name != current_session)
        .take(remove_count)
    {
        _ = fs::remove_dir_all(path);
    }
}

fn session_size(directory: &Path) -> io::Result<u64> {
    let mut size = 0_u64;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && !entry.file_name().to_string_lossy().starts_with('.') {
            size = size.saturating_add(entry.metadata()?.len());
        }
    }
    Ok(size)
}

fn valid_session_id(value: &str) -> bool {
    value.len() == 30
        && value.as_bytes().get(13) == Some(&b'-')
        && value[..13].bytes().all(|byte| byte.is_ascii_digit())
        && value[14..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn sanitize_message(message: &str) -> String {
    let mut redact_next = false;
    let mut sanitized = Vec::new();
    for word in message.split_whitespace() {
        let lowered = word.to_ascii_lowercase();
        let sensitive_label = lowered.trim_end_matches(':');
        if redact_next {
            sanitized.push("[REDACTED]".to_owned());
            redact_next = false;
        } else if ["token=", "secret=", "password=", "cookie="]
            .iter()
            .any(|pattern| lowered.contains(pattern))
        {
            sanitized.push("[REDACTED]".to_owned());
        } else if [
            "token",
            "access_token",
            "refresh_token",
            "secret",
            "password",
            "authorization",
            "cookie",
            "bearer",
        ]
        .contains(&sensitive_label)
        {
            sanitized.push("[REDACTED]".to_owned());
            redact_next = true;
        } else if word.contains("://") {
            sanitized.push(word.split(['?', '#']).next().unwrap_or(word).to_owned());
        } else {
            sanitized.push(
                word.chars()
                    .filter(|character| !character.is_control())
                    .collect(),
            );
        }
    }
    sanitized.join(" ")
}

fn truncate_utf8(bytes: &[u8], maximum: usize) -> &[u8] {
    if bytes.len() <= maximum {
        return bytes;
    }
    let mut end = maximum;
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    &bytes[..end]
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
