//! 更新清单数据模型与版本选择规则。

use semver::Version;
use serde::Deserialize;

use crate::{UpdateConfig, UpdateError};

/// 应用接收更新时使用的发布通道。
///
/// 不同通道应使用独立的 `latest.json`，避免稳定版客户端意外接收测试版或每日构建。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    /// 仅面向正式用户发布的稳定版本。
    Stable,
    /// 面向测试用户发布、可能包含预发布功能的 Beta 版本。
    Beta,
    /// 高频构建、主要用于开发验证的每日版本。
    Nightly,
}

impl UpdateChannel {
    /// 返回更新清单中使用的小写通道标识。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Nightly => "nightly",
        }
    }
}

/// 当前更新包支持的操作系统和 CPU 架构组合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateTarget {
    /// Apple Silicon macOS，对应 Rust target `aarch64-apple-darwin`。
    MacOsAarch64,
    /// Intel macOS，对应 Rust target `x86_64-apple-darwin`。
    MacOsX86_64,
}

impl UpdateTarget {
    /// 检测当前进程所在平台，并返回更新清单使用的目标标识。
    ///
    /// # Errors
    ///
    /// 当前首版只支持 macOS 的 Apple Silicon 与 Intel 架构；其他平台会返回
    /// [`UpdateError::UnsupportedPlatform`]。
    pub fn current() -> Result<Self, UpdateError> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Ok(Self::MacOsAarch64);
        }

        if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            return Ok(Self::MacOsX86_64);
        }

        Err(UpdateError::UnsupportedPlatform)
    }

    /// 返回与 Rust target triple 一致的更新目标标识。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacOsAarch64 => "aarch64-apple-darwin",
            Self::MacOsX86_64 => "x86_64-apple-darwin",
        }
    }
}

/// `latest.json` 描述的一份目标平台安装包。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UpdateArtifact {
    /// 安装包对应的 Rust target triple。
    pub target: String,
    /// `.app.zip` 安装包下载地址；相对地址会基于清单地址解析。
    pub url: String,
    /// 安装包内容的十六进制 SHA-256 摘要。
    pub sha256: String,
    /// 服务端已知的安装包字节数，用于在响应缺少 `Content-Length` 时展示进度。
    pub size: Option<u64>,
}

/// 服务端发布的一条可安装更新。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRelease {
    /// 面向用户展示并参与 SemVer 排序的版本号。
    pub version: Version,
    /// 同一版本内持续递增的构建号。
    pub bundle_version: u64,
    /// 当前平台需要下载的安装包。
    pub artifact: UpdateArtifact,
    /// 可选的远程更新日志地址。
    pub notes_url: Option<String>,
}

/// 更新服务器返回的 `latest.json` 数据结构。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UpdateManifest {
    /// 更新协议版本；当前实现只接受 `1`。
    pub schema_version: u32,
    /// 应用稳定标识，用于避免错误安装其他桌面程序的更新。
    pub app_id: String,
    /// 该清单所属的更新通道。
    pub channel: UpdateChannel,
    /// 清单中最新发布的语义化版本。
    pub version: Version,
    /// 该版本最新发布的构建号。
    pub bundle_version: u64,
    /// 可选的远程更新日志地址。
    pub notes_url: Option<String>,
    /// 不同操作系统和架构对应的安装包列表。
    pub artifacts: Vec<UpdateArtifact>,
}

impl UpdateManifest {
    /// 从 UTF-8 JSON 文本解析更新清单。
    ///
    /// # Errors
    ///
    /// 当 JSON 语法、字段类型或 SemVer 版本号无效时返回 [`UpdateError::InvalidManifest`]。
    pub fn parse(json: &str) -> Result<Self, UpdateError> {
        serde_json::from_str(json).map_err(UpdateError::InvalidManifest)
    }

    /// 根据应用配置和目标平台选择可安装更新。
    ///
    /// 比较顺序是 `(version, bundle_version)`：优先选择更高的 SemVer；版本相同时，
    /// 只有更高的构建号才被视为更新。
    ///
    /// # Errors
    ///
    /// 协议版本、应用标识或更新通道不匹配，或者清单缺少当前平台安装包时返回错误。
    pub fn select_update(
        &self,
        config: &UpdateConfig,
        target: UpdateTarget,
    ) -> Result<Option<UpdateRelease>, UpdateError> {
        if self.schema_version != 1 {
            return Err(UpdateError::UnsupportedSchema(self.schema_version));
        }

        if self.app_id != config.app_id() {
            return Err(UpdateError::AppIdMismatch {
                expected: config.app_id().to_owned(),
                actual: self.app_id.clone(),
            });
        }

        if self.channel != config.channel() {
            return Err(UpdateError::ChannelMismatch {
                expected: config.channel(),
                actual: self.channel,
            });
        }

        let is_newer = self.version > *config.current_version()
            || (self.version == *config.current_version()
                && self.bundle_version > config.current_bundle_version());
        if !is_newer {
            return Ok(None);
        }

        let artifact = self
            .artifacts
            .iter()
            .find(|artifact| artifact.target == target.as_str())
            .cloned()
            .ok_or_else(|| UpdateError::MissingArtifact(target.as_str().to_owned()))?;

        Ok(Some(UpdateRelease {
            version: self.version.clone(),
            bundle_version: self.bundle_version,
            artifact,
            notes_url: self.notes_url.clone(),
        }))
    }
}
