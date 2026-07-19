//! 共享桌面 UI 组件库入口。
//!
//! 该 crate 用于沉淀跨桌面应用复用的 UI 组件、主题、布局工具和视觉资源。

/// 桌面工作区中用于承载表格、表单和摘要的内容卡片。
pub mod card;

/// 基于 gpui-component 组合实现的级联选择器。
pub mod cascader;

/// 标准 CRUD 资源管理 Panel 骨架。
pub mod crud_panel;

/// 桌面应用中可复用的布局组件。
pub mod layout;

/// 桌面应用未登录时复用的认证门禁。
pub mod login_gate;

/// 带草稿追踪与未保存确认的内容区表单对话框。
pub mod form_dialog;

/// 受右侧主面板边界约束的模态对话框。
pub mod panel_dialog;

/// 桌面工作区右侧主面板的统一顶部栏。
pub mod panel_header;

/// Sidebar Header/Footer 中由应用自行控制交互视觉的稳定区域。
pub mod sidebar_region;

/// 桌面数据表表头辅助组件。
pub mod table_header;

/// 窗口级 Dialog、Sheet 与 Notification 遮罩层组合。
pub mod window_layers;

pub use card::Card;
pub use cascader::{
    Cascader, CascaderEvent, CascaderOption, CascaderSelection, CascaderState, CascaderValueError,
};
pub use crud_panel::{CrudPanel, CrudPanelToolbar};
pub use form_dialog::{FormDialog, FormDialogState, FormFieldDraft};
pub use login_gate::{LoginGate, default_application_logo};
pub use panel_dialog::PanelDialog;
pub use panel_header::PanelHeader;
pub use sidebar_region::SidebarRegion;
pub use table_header::TableHeaderCell;
pub use window_layers::window_layers;
