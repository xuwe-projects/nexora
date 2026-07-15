//! 共享桌面 UI 组件库入口。
//!
//! 该 crate 用于沉淀跨桌面应用复用的 UI 组件、主题、布局工具和视觉资源。

/// 桌面工作区中用于承载表格、表单和摘要的内容卡片。
pub mod card;

/// 桌面应用中可复用的布局组件。
pub mod layout;

/// 桌面应用未登录时复用的认证门禁。
pub mod login_gate;

/// 桌面工作区右侧主面板的统一顶部栏。
pub mod panel_header;

/// 窗口级 Dialog、Sheet 与 Notification 遮罩层组合。
pub mod window_layers;

pub use card::Card;
pub use login_gate::LoginGate;
pub use panel_header::PanelHeader;
pub use window_layers::window_layers;
