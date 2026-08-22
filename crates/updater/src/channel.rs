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

    /// 根据基础应用标识派生当前通道的本机安装与运行时标识。
    ///
    /// 稳定通道原样保留基础标识，以兼容既有安装、单例、凭据和 updater 状态；Beta 与
    /// Nightly 分别追加固定后缀，调用方不得再提供可配置后缀。
    pub fn installation_id(self, base_id: &str) -> String {
        match self {
            Self::Stable => base_id.to_owned(),
            Self::Beta => format!("{base_id}.beta"),
            Self::Nightly => format!("{base_id}.nightly"),
        }
    }

    /// 根据基础展示名称派生当前通道面向用户的应用名称。
    ///
    /// 稳定通道保持原名称；Beta 与 Nightly 使用固定英文通道后缀，让安装目录、快捷方式、
    /// Bundle 名称和运行时品牌能够彼此区分。
    pub fn display_name(self, base_name: &str) -> String {
        match self {
            Self::Stable => base_name.to_owned(),
            Self::Beta => format!("{base_name} Beta"),
            Self::Nightly => format!("{base_name} Nightly"),
        }
    }
}
