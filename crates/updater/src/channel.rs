//! 桌面应用发布通道的轻量协议类型。

use serde::{Deserialize, Serialize};

/// 应用接收更新时使用的发布通道。
///
/// 不同通道使用独立的 `latest.json`，避免稳定版客户端意外接收测试版或每日构建。
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
