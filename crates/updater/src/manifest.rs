//! 更新清单签名信封、目标平台和版本选择规则。

use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{UpdateConfig, UpdateError};

/// 应用接收更新时使用的发布通道。
///
/// 不同通道必须使用独立的 `latest.json`，避免稳定版客户端意外接收测试版或每日构建。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
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
    /// 64 位 Windows MSVC，对应 Rust target `x86_64-pc-windows-msvc`。
    WindowsX86_64,
    /// 64 位 Linux GNU，对应 Rust target `x86_64-unknown-linux-gnu`。
    LinuxX86_64,
}

impl UpdateTarget {
    /// 检测当前进程所在平台，并返回更新清单使用的目标标识。
    ///
    /// # Errors
    ///
    /// 当前平台不属于首版支持的 macOS、Windows 或 Linux x86_64/aarch64 组合时返回
    /// [`UpdateError::UnsupportedPlatform`]。
    pub fn current() -> Result<Self, UpdateError> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Ok(Self::MacOsAarch64);
        }
        if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            return Ok(Self::MacOsX86_64);
        }
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            return Ok(Self::WindowsX86_64);
        }
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Ok(Self::LinuxX86_64);
        }

        Err(UpdateError::UnsupportedPlatform)
    }

    /// 根据 Rust target triple 构造更新目标。
    ///
    /// # Errors
    ///
    /// 传入的 target triple 不属于当前协议支持列表时返回 [`UpdateError::UnsupportedTarget`]。
    pub fn from_triple(target: &str) -> Result<Self, UpdateError> {
        match target {
            "aarch64-apple-darwin" => Ok(Self::MacOsAarch64),
            "x86_64-apple-darwin" => Ok(Self::MacOsX86_64),
            "x86_64-pc-windows-msvc" => Ok(Self::WindowsX86_64),
            "x86_64-unknown-linux-gnu" => Ok(Self::LinuxX86_64),
            other => Err(UpdateError::UnsupportedTarget(other.to_owned())),
        }
    }

    /// 返回与 Rust target triple 一致的更新目标标识。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacOsAarch64 => "aarch64-apple-darwin",
            Self::MacOsX86_64 => "x86_64-apple-darwin",
            Self::WindowsX86_64 => "x86_64-pc-windows-msvc",
            Self::LinuxX86_64 => "x86_64-unknown-linux-gnu",
        }
    }
}

/// `latest.json` 中描述的一份目标平台更新负载。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct UpdateArtifact {
    /// 安装包对应的 Rust target triple。
    pub target: String,
    /// 更新负载下载地址；相对地址会基于清单地址解析。
    pub url: String,
    /// 更新负载内容的十六进制 SHA-256 摘要。
    pub sha256: String,
    /// 服务端已知的负载字节数，用于进度展示和下载后校验。
    pub size: u64,
    /// 负载格式，例如 `macos_app_zip`、`windows_zip`、`linux_appimage` 或 `portable_tar_zst`。
    pub kind: String,
}

/// 服务端发布的一条可安装更新。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRelease {
    /// 面向用户展示并参与 SemVer 排序的版本号。
    pub version: Version,
    /// 同一版本内持续递增的构建号。
    pub build_number: u64,
    /// 强制升级的最低受支持版本。
    pub minimum_supported_version: Version,
    /// 服务端发布的单调递增清单序号。
    pub manifest_sequence: u64,
    /// 当前平台需要下载的安装包。
    pub artifact: UpdateArtifact,
    /// 可选的远程更新日志地址。
    pub notes_url: Option<String>,
    /// 该更新是否强制安装。
    pub mandatory: bool,
    pub(crate) verified_manifest: Option<Arc<SignedUpdateManifest>>,
}

impl UpdateRelease {
    pub(crate) fn with_verified_manifest(mut self, manifest: SignedUpdateManifest) -> Self {
        self.verified_manifest = Some(Arc::new(manifest));
        self
    }

    pub(crate) fn verified_manifest(&self) -> Result<&SignedUpdateManifest, UpdateError> {
        self.verified_manifest
            .as_deref()
            .ok_or(UpdateError::MissingVerifiedManifest)
    }
}

/// 已验证签名后的更新清单负载。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct UpdateManifest {
    /// 服务端发布的单调递增清单序号，用于拒绝重放旧清单。
    pub manifest_sequence: u64,
    /// 应用稳定标识，用于避免错误安装其他桌面程序的更新。
    pub app_id: String,
    /// 该清单所属的更新通道。
    pub channel: UpdateChannel,
    /// 清单中最新发布的语义化版本。
    pub version: Version,
    /// 该版本最新发布的构建号。
    pub build_number: u64,
    /// 允许继续运行的最低版本；低于该版本时必须强制更新或退出。
    pub minimum_supported_version: Version,
    /// 发布时间，使用 Unix 秒。
    pub published_at: i64,
    /// 发布状态。
    pub status: ReleaseStatus,
    /// 可选的远程更新日志地址。
    pub notes_url: Option<String>,
    /// 不同操作系统和架构对应的安装包列表。
    pub artifacts: Vec<UpdateArtifact>,
}

/// 发布清单的控制状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseStatus {
    /// 正常发布，客户端可按版本规则安装。
    Available,
    /// 紧急撤回控制清单，不为客户端提供降级安装。
    Yanked,
}

/// `latest.json` 顶层签名信封。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SignedUpdateManifest {
    /// 签名信封协议版本；当前实现只接受 `1`。
    pub schema_version: u32,
    /// 被签名的清单负载。
    pub payload: UpdateManifest,
    /// 一个或多个 Ed25519 签名，支持公钥轮换。
    pub signatures: Vec<UpdateManifestSignature>,
}

/// 单个清单签名。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct UpdateManifestSignature {
    /// 公钥标识，与配置中的 `key_id` 对应。
    pub key_id: String,
    /// 签名算法；当前唯一合法值是 `ed25519`。
    pub algorithm: String,
    /// 对 canonical payload JSON 字节计算的 Base64 Ed25519 签名。
    pub signature: String,
}

/// 客户端信任的 Ed25519 公钥。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPublicKey {
    key_id: String,
    key: VerifyingKey,
}

impl TrustedPublicKey {
    /// 解析 `key_id:ed25519:BASE64_PUBLIC_KEY` 形式的公钥配置。
    ///
    /// # Errors
    ///
    /// 格式、算法、Base64 或 Ed25519 公钥字节无效时返回 [`UpdateError::InvalidPublicKey`]。
    pub fn parse(value: &str) -> Result<Self, UpdateError> {
        let parts = value.split(':').collect::<Vec<_>>();
        let [key_id, algorithm, encoded] = parts.as_slice() else {
            return Err(UpdateError::InvalidPublicKey);
        };
        if key_id.is_empty() || *algorithm != "ed25519" {
            return Err(UpdateError::InvalidPublicKey);
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| UpdateError::InvalidPublicKey)?;
        let key_bytes: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| UpdateError::InvalidPublicKey)?;
        let key =
            VerifyingKey::from_bytes(&key_bytes).map_err(|_| UpdateError::InvalidPublicKey)?;

        Ok(Self {
            key_id: (*key_id).to_owned(),
            key,
        })
    }

    /// 返回公钥标识。
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

impl SignedUpdateManifest {
    /// 从 UTF-8 JSON 文本解析并验签 `latest.json`。
    ///
    /// # Errors
    ///
    /// JSON 格式、协议版本、签名算法、签名值或可信公钥匹配失败时返回具体错误。
    pub fn parse_and_verify(
        json: &str,
        trusted_keys: &[TrustedPublicKey],
    ) -> Result<UpdateManifest, UpdateError> {
        let envelope: Self = serde_json::from_str(json).map_err(UpdateError::InvalidManifest)?;
        envelope.verify(trusted_keys)
    }

    /// 使用客户端可信公钥重新验证当前签名信封并返回清单负载。
    ///
    /// 该方法用于验证已从本地待安装缓存恢复的签名信封；本地记录本身不被视为可信来源。
    ///
    /// # Errors
    ///
    /// 协议版本不受支持、未配置可信公钥，或没有任何签名通过验证时返回错误。
    pub fn verify(&self, trusted_keys: &[TrustedPublicKey]) -> Result<UpdateManifest, UpdateError> {
        if trusted_keys.is_empty() {
            return Err(UpdateError::MissingTrustedPublicKeys);
        }
        if self.schema_version != 1 {
            return Err(UpdateError::UnsupportedSchema(self.schema_version));
        }

        let payload_bytes = canonical_payload_bytes(&self.payload)?;
        let verified = self.signatures.iter().any(|signature| {
            signature.algorithm == "ed25519"
                && trusted_keys
                    .iter()
                    .find(|key| key.key_id == signature.key_id)
                    .is_some_and(|key| verify_signature(key, signature, &payload_bytes))
        });
        if !verified {
            return Err(UpdateError::ManifestSignatureRejected);
        }

        Ok(self.payload.clone())
    }
}

impl UpdateManifest {
    /// 根据应用配置和目标平台选择可安装更新。
    ///
    /// 比较顺序是 `(version, build_number)`：优先选择更高的 SemVer；版本相同时，只有更高
    /// 构建号才被视为更新。低于 `minimum_supported_version` 时即使版本未更新也返回强制状态。
    ///
    /// # Errors
    ///
    /// 应用标识、通道、清单序号或目标平台不匹配时返回错误。
    pub fn select_update(
        &self,
        config: &UpdateConfig,
        target: UpdateTarget,
    ) -> Result<Option<UpdateRelease>, UpdateError> {
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

        if self.manifest_sequence < config.highest_manifest_sequence() {
            return Err(UpdateError::ManifestReplay {
                highest: config.highest_manifest_sequence(),
                actual: self.manifest_sequence,
            });
        }

        if self.status == ReleaseStatus::Yanked {
            return Ok(None);
        }

        let mandatory = config.current_version() < &self.minimum_supported_version;
        let is_newer = self.version > *config.current_version()
            || (self.version == *config.current_version()
                && self.build_number > config.current_build_number());
        if !mandatory && !is_newer {
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
            build_number: self.build_number,
            minimum_supported_version: self.minimum_supported_version.clone(),
            manifest_sequence: self.manifest_sequence,
            artifact,
            notes_url: self.notes_url.clone(),
            mandatory,
            verified_manifest: None,
        }))
    }
}

pub(crate) fn canonical_payload_bytes(payload: &UpdateManifest) -> Result<Vec<u8>, UpdateError> {
    serde_json::to_vec(payload)
        .map_err(|error| UpdateError::InvalidManifestSerialization(error.to_string()))
}

fn verify_signature(
    key: &TrustedPublicKey,
    manifest_signature: &UpdateManifestSignature,
    payload_bytes: &[u8],
) -> bool {
    let Ok(signature_bytes) = STANDARD.decode(&manifest_signature.signature) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(&signature_bytes) else {
        return false;
    };

    key.key.verify(payload_bytes, &signature).is_ok()
}
