//! 桌面应用更新检查、下载校验与原位安装能力。
//!
//! 服务层不依赖具体窗口状态；每个桌面应用只需要提供更新清单地址、应用标识、当前版本、
//! 构建号和更新通道，即可消费 [`UpdateEvent`]，也可以直接使用内置 GPUI 确认与进度弹窗。

mod dialog;
mod macos;
mod manifest;
mod release;
mod service;
mod sidecar;
mod windows;

pub use dialog::{open_update_dialog, show_update_completed_dialog, start_update_check_on_launch};
pub use manifest::{
    ReleaseStatus, SignedUpdateManifest, TrustedPublicKey, UpdateArtifact, UpdateChannel,
    UpdateManifest, UpdateManifestSignature, UpdateRelease, UpdateTarget,
};
pub use release::{
    ApplicationReleaseMetadata, LoadedApplicationReleaseMetadata, MAX_RELEASE_NOTES_BYTES,
    RELEASE_METADATA_FILE_NAME, RELEASE_NOTES_FILE_NAME, ReleaseMetadataError, ReleaseNotesError,
    ReleaseNotesMetadata, load_current_release_metadata, load_release_metadata_from_directory,
    read_verified_local_release_notes, verify_release_notes_bytes,
};
pub use service::{
    CancellationToken, StagedUpdate, UpdateConfig, UpdateError, UpdateEvent, UpdateSession,
    Updater, WindowsSignatureConfig,
};
pub use sidecar::{report_health_from_env_args, run_sidecar_from_env_args};
pub use windows::{extract_windows_update_zip, validate_windows_zip_entry_path};
