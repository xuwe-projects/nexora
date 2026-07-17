//! 默认角色管理页面的私有组件。

mod create;
mod editor;
mod list;
mod page;

pub(super) use create::RoleCreateDialog;
pub(super) use editor::RoleEditor;
pub(super) use list::RolesList;
pub(super) use page::RolesPage;
