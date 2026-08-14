//! 默认用户管理页面的私有组件。

mod credential_secret;
mod credentials;
mod page;
mod provision;
mod role_editor;
mod service_account;
mod table;

pub(super) use credential_secret::CredentialSecretDialog;
pub(super) use credentials::ServiceAccountCredentials;
pub(super) use page::UsersPage;
pub(super) use provision::ProvisionUserDialog;
pub(super) use role_editor::UserRoleEditor;
pub(super) use service_account::CreateServiceAccountDialog;
pub(super) use table::UsersTable;
