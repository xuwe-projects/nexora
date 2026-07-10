//! 共享桌面 UI 组件库入口。
//!
//! 该 crate 用于沉淀跨桌面应用复用的 UI 组件、主题、布局工具和视觉资源。

/// 桌面工作区中用于承载表格、表单和摘要的内容卡片。
pub mod card;

/// 桌面应用中可复用的布局组件。
pub mod layout;

/// 桌面工作区右侧主面板的统一顶部栏。
pub mod panel_header;

pub use card::Card;
pub use panel_header::PanelHeader;
