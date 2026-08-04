//! 桌面安装包携带的通用发布身份与更新日志完整性契约。

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

use crate::UpdateChannel;

/// 安装包内通用发布元数据的文件名。
pub const RELEASE_METADATA_FILE_NAME: &str = "nexora-release.json";

/// 安装包内冻结更新日志的统一文件名。
pub const RELEASE_NOTES_FILE_NAME: &str = "notes.md";

/// 单份更新日志允许的最大字节数，当前限制为 1 MiB。
///
/// 构建、远程下载和安装包本地读取都使用同一上限，避免超大 Markdown 占用过多内存或
/// 阻塞桌面渲染线程。
pub const MAX_RELEASE_NOTES_BYTES: u64 = 1024 * 1024;

/// 正式安装包中冻结的一份应用发布身份。
///
/// 该结构由 `nexora build` 根据 release receipt 生成，并在签名与归档前写入安装包。
/// 运行时只读取和校验该文件，不会重新计算版本、构建号或发布通道。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationReleaseMetadata {
    /// 元数据协议版本；当前唯一支持值为 `1`。
    pub schema_version: u32,
    /// `nexora.toml` 中选择应用时使用的稳定 app key。
    pub app_key: String,
    /// 操作系统和更新协议使用的稳定应用标识。
    pub app_id: String,
    /// 面向用户展示的应用名称。
    pub display_name: String,
    /// Cargo 桌面二进制 package 名称。
    pub package: String,
    /// release receipt 冻结的语义化版本号。
    pub version: Version,
    /// release receipt 冻结且大于零的构建号。
    pub build_number: u64,
    /// 此安装包所属的固定发布通道。
    pub channel: UpdateChannel,
    /// 此安装包对应的 Rust target triple。
    pub target: String,
    /// 安装包内冻结日志的可选完整性信息。
    pub notes: Option<ReleaseNotesMetadata>,
}

impl ApplicationReleaseMetadata {
    /// 校验发布身份与日志描述是否满足运行时契约。
    ///
    /// # Errors
    ///
    /// schema 不受支持、必需字符串为空、构建号为零、应用标识不安全，或日志描述的文件名、
    /// 大小、SHA-256 不合法时返回 [`ReleaseMetadataError::InvalidMetadata`]。
    pub fn validate(&self) -> Result<(), ReleaseMetadataError> {
        if self.schema_version != 1 {
            return Err(ReleaseMetadataError::InvalidMetadata(format!(
                "不支持发布元数据版本 {}",
                self.schema_version
            )));
        }
        for (label, value) in [
            ("app_key", self.app_key.as_str()),
            ("display_name", self.display_name.as_str()),
            ("package", self.package.as_str()),
            ("target", self.target.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ReleaseMetadataError::InvalidMetadata(format!(
                    "{label} 不能为空"
                )));
            }
        }
        if !valid_app_id(&self.app_id) {
            return Err(ReleaseMetadataError::InvalidMetadata(
                "app_id 不是安全的应用标识".to_owned(),
            ));
        }
        if self.build_number == 0 {
            return Err(ReleaseMetadataError::InvalidMetadata(
                "build_number 必须大于 0".to_owned(),
            ));
        }
        if let Some(notes) = &self.notes {
            notes.validate()?;
        }
        Ok(())
    }
}

/// 安装包或签名 manifest 中一份更新日志的完整性描述。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseNotesMetadata {
    /// 安装包内日志文件名；当前必须为 `notes.md`。
    pub file_name: String,
    /// 冻结日志的准确字节数。
    pub size: u64,
    /// 冻结日志内容的小写十六进制 SHA-256。
    pub sha256: String,
}

impl ReleaseNotesMetadata {
    /// 校验日志文件名、大小和摘要格式。
    ///
    /// # Errors
    ///
    /// 文件名不是 [`RELEASE_NOTES_FILE_NAME`]、大小为零或超过
    /// [`MAX_RELEASE_NOTES_BYTES`]，或摘要不是 64 位十六进制时返回错误。
    pub fn validate(&self) -> Result<(), ReleaseMetadataError> {
        if self.file_name != RELEASE_NOTES_FILE_NAME {
            return Err(ReleaseMetadataError::InvalidMetadata(format!(
                "notes.file_name 必须为 `{RELEASE_NOTES_FILE_NAME}`"
            )));
        }
        if self.size == 0 || self.size > MAX_RELEASE_NOTES_BYTES {
            return Err(ReleaseMetadataError::InvalidMetadata(format!(
                "notes.size 必须在 1..={MAX_RELEASE_NOTES_BYTES} 字节范围内"
            )));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ReleaseMetadataError::InvalidMetadata(
                "notes.sha256 必须是 64 位十六进制 SHA-256".to_owned(),
            ));
        }
        Ok(())
    }
}

/// 从安装位置读取并校验后的发布元数据及其资源目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedApplicationReleaseMetadata {
    metadata: ApplicationReleaseMetadata,
    resource_directory: PathBuf,
}

impl LoadedApplicationReleaseMetadata {
    /// 返回已经通过结构校验的发布身份。
    pub fn metadata(&self) -> &ApplicationReleaseMetadata {
        &self.metadata
    }

    /// 返回包含发布元数据和本地 `notes.md` 的安装资源目录。
    pub fn resource_directory(&self) -> &Path {
        &self.resource_directory
    }
}

/// 读取正式安装包发布元数据时可能产生的错误。
#[derive(Debug, Error)]
pub enum ReleaseMetadataError {
    /// 文件存在但 JSON、schema 或字段值不符合发布契约。
    #[error("应用发布元数据无效: {0}")]
    InvalidMetadata(
        /// 不包含文件正文或秘密的结构校验原因。
        String,
    ),
    /// 无法读取发布元数据文件。
    #[error("无法读取应用发布元数据: {0}")]
    Io(
        /// 文件系统返回的底层读取错误。
        #[from]
        std::io::Error,
    ),
}

/// 更新日志读取或完整性验证失败。
#[derive(Debug, Error)]
pub enum ReleaseNotesError {
    /// 日志描述本身不符合发布元数据契约。
    #[error(transparent)]
    InvalidMetadata(
        /// 原始发布元数据校验错误。
        #[from]
        ReleaseMetadataError,
    ),
    /// 响应或本地文件超过协议允许的安全上限。
    #[error("更新日志超过 {MAX_RELEASE_NOTES_BYTES} 字节安全上限")]
    TooLarge,
    /// 实际字节数与签名或安装包元数据声明不一致。
    #[error("更新日志大小不一致，期望 {expected} 字节，实际 {actual} 字节")]
    SizeMismatch {
        /// 元数据声明的字节数。
        expected: u64,
        /// 实际读取的字节数。
        actual: u64,
    },
    /// 实际 SHA-256 与可信元数据不一致。
    #[error("更新日志 SHA-256 校验失败")]
    ChecksumMismatch,
    /// 日志不是有效 UTF-8 Markdown。
    #[error("更新日志不是有效 UTF-8")]
    InvalidUtf8,
    /// 日志包含不允许交给系统浏览器的危险链接协议。
    #[error("更新日志包含不安全的链接协议")]
    UnsafeLink,
    /// 本地日志文件无法读取。
    #[error("无法读取更新日志: {0}")]
    Io(
        /// 文件系统返回的底层读取错误。
        #[from]
        std::io::Error,
    ),
}

/// 从当前可执行程序的正式安装位置读取通用发布元数据。
///
/// macOS 从 `.app/Contents/Resources` 读取；Windows 与后续便携式平台从主程序同级目录
/// 读取。开发运行中找不到文件时返回 `Ok(None)`，文件存在但无效时返回错误，调用方不得
/// 把后者降级为伪造的开发身份。
///
/// # Errors
///
/// 当前可执行文件路径不可读取，或已存在的元数据文件无法读取、解析或通过校验时返回错误。
pub fn load_current_release_metadata()
-> Result<Option<LoadedApplicationReleaseMetadata>, ReleaseMetadataError> {
    let executable = std::env::current_exe()?;
    let executable_directory = executable.parent().ok_or_else(|| {
        ReleaseMetadataError::InvalidMetadata("当前可执行文件没有父目录".to_owned())
    })?;
    let resource_directory = if cfg!(target_os = "macos")
        && executable_directory
            .file_name()
            .and_then(|name| name.to_str())
            == Some("MacOS")
    {
        executable_directory
            .parent()
            .map(|contents| contents.join("Resources"))
            .ok_or_else(|| {
                ReleaseMetadataError::InvalidMetadata("macOS bundle 目录结构无效".to_owned())
            })?
    } else {
        executable_directory.to_path_buf()
    };
    load_release_metadata_from_directory(resource_directory)
}

/// 从指定安装资源目录读取通用发布元数据。
///
/// 该入口供平台适配和集成测试使用；文件缺失代表开发模式并返回 `Ok(None)`。
///
/// # Errors
///
/// 文件存在但无法读取、不是合法 JSON 或字段不符合发布契约时返回错误。
pub fn load_release_metadata_from_directory(
    resource_directory: impl Into<PathBuf>,
) -> Result<Option<LoadedApplicationReleaseMetadata>, ReleaseMetadataError> {
    let resource_directory = resource_directory.into();
    let path = resource_directory.join(RELEASE_METADATA_FILE_NAME);
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata: ApplicationReleaseMetadata = serde_json::from_slice(&contents)
        .map_err(|error| ReleaseMetadataError::InvalidMetadata(error.to_string()))?;
    metadata.validate()?;
    Ok(Some(LoadedApplicationReleaseMetadata {
        metadata,
        resource_directory,
    }))
}

/// 读取并验证安装包中冻结的本地更新日志。
///
/// 返回的 Markdown 已将原始 HTML 转义，可直接交给 `TextView::markdown`；普通 HTTPS/HTTP
/// 链接仍由组件库通过系统浏览器打开。
///
/// # Errors
///
/// 文件不可读、过大、大小或 SHA-256 不匹配、不是 UTF-8，或包含危险链接协议时返回错误。
pub fn read_verified_local_release_notes(
    directory: &Path,
    metadata: &ReleaseNotesMetadata,
) -> Result<String, ReleaseNotesError> {
    metadata.validate()?;
    let bytes = fs::read(directory.join(&metadata.file_name))?;
    verify_release_notes_bytes(metadata, &bytes)
}

/// 根据可信的大小和摘要验证一份更新日志字节。
///
/// 成功时返回已转义原始 HTML 的 Markdown 文本；该函数不会写入缓存或执行任何内容。
///
/// # Errors
///
/// 元数据、大小、摘要、UTF-8 或链接协议不合法时返回对应错误。
pub fn verify_release_notes_bytes(
    metadata: &ReleaseNotesMetadata,
    bytes: &[u8],
) -> Result<String, ReleaseNotesError> {
    metadata.validate()?;
    let actual = u64::try_from(bytes.len()).map_err(|_| ReleaseNotesError::TooLarge)?;
    if actual > MAX_RELEASE_NOTES_BYTES {
        return Err(ReleaseNotesError::TooLarge);
    }
    if actual != metadata.size {
        return Err(ReleaseNotesError::SizeMismatch {
            expected: metadata.size,
            actual,
        });
    }
    let digest = sha256_hex(bytes);
    if !digest.eq_ignore_ascii_case(&metadata.sha256) {
        return Err(ReleaseNotesError::ChecksumMismatch);
    }
    let markdown = std::str::from_utf8(bytes).map_err(|_| ReleaseNotesError::InvalidUtf8)?;
    let lowercase = markdown.to_ascii_lowercase();
    if ["javascript:", "data:", "file:"]
        .iter()
        .any(|scheme| lowercase.contains(scheme))
    {
        return Err(ReleaseNotesError::UnsafeLink);
    }
    Ok(markdown.replace('<', "&lt;").replace('>', "&gt;"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut output, byte| {
            write!(output, "{byte:02x}").expect("写入 String 不会失败");
            output
        })
}

fn valid_app_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 255
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}
