//! 桌面应用更新检查、下载校验与原位安装能力。
//!
//! 该 crate 不依赖 GPUI。每个桌面应用只需要提供更新清单地址、应用标识、当前版本、
//! 构建号和更新通道，即可在自己的 UI 中消费 [`UpdateEvent`] 并展示进度。

mod dialog;
mod macos;
mod manifest;
mod service;

pub use dialog::open_update_dialog;
pub use manifest::{UpdateArtifact, UpdateChannel, UpdateManifest, UpdateRelease, UpdateTarget};
pub use service::{
    CancellationToken, StagedUpdate, UpdateConfig, UpdateError, UpdateEvent, UpdateSession, Updater,
};
