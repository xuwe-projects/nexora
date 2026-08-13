//! 共享桌面 UI 组件库入口。
//!
//! 该 crate 用于沉淀跨桌面应用复用的 UI 组件、主题、布局工具和视觉资源。

/// 基于 gpui-component 组合实现的级联选择器。
pub mod cascader;

/// 标准 CRUD 资源管理 Panel 骨架。
pub mod crud_panel;

/// 标准 CRUD 分页列表的查询、缓存、选择与异步加载状态。
pub mod crud_list_state;

/// 标准 CRUD 数据表增强能力。
pub mod crud_table;

/// DataTable 列顺序与列宽的持久化模型和纯逻辑适配。
pub mod data_table_layout;

/// 桌面应用中可复用的布局组件。
pub mod layout;

/// 无视觉表单字段状态。
pub mod form_field_state;

/// 桌面应用未登录时复用的认证门禁。
pub mod login_gate;

/// 带草稿追踪与未保存确认的内容区表单对话框。
pub mod form_dialog;

/// 受右侧主面板边界约束的模态对话框。
pub mod panel_dialog;

/// Sidebar Header/Footer 中由应用自行控制交互视觉的稳定区域。
pub mod sidebar_region;

/// 桌面数据表表头辅助组件。
pub mod table_header;

/// 桌面数据表正文单元格辅助组件。
pub mod table_cell;

/// 窗口级 Dialog、Sheet 与 Notification 遮罩层组合。
pub mod window_layers;

pub use cascader::{
    Cascader, CascaderEvent, CascaderOption, CascaderSelection, CascaderState, CascaderValueError,
};
pub use crud_list_state::{CrudListState, CrudListStateError, CrudLoadError, CrudPage};
pub use crud_panel::CrudPanel;
pub use crud_table::{
    CrudTableDelegate, CrudTableRow, CrudTableSelection, LoadedRowsSelectionEvent,
    RowSelectionEvent,
};
pub use data_table_layout::{
    DataTableColumnLayout, DataTableLayout, DataTableLayoutError, DataTableLayoutKey,
    apply_data_table_layout, capture_data_table_layout, data_table_layout_from_event,
};
pub use form_dialog::{FormDialog, FormDialogState, FormFieldDraft};
pub use form_field_state::{
    AnyFormFieldState, FieldValue, FieldValueParseError, FormFieldEvent, FormFieldState,
    FormFieldStateBuilder, FormFieldTarget, NumberFieldValue,
};
pub use login_gate::{LoginGate, default_application_logo};
pub use panel_dialog::PanelDialog;
pub use sidebar_region::SidebarRegion;
pub use table_cell::{TableCell, TableCellVerticalAlign};
pub use table_header::TableHeaderCell;
pub use window_layers::window_layers;
