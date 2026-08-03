//! Nexora 桌面端认证、会话与 Account HTTP 客户端 facade。
//!
//! 应用通过本模块安装认证协调器、读取登录状态并调用用户、角色和权限 API，不需要了解
//! Account 客户端的内部模块层级。

pub(crate) mod updater;

pub use self::updater::{
    UpdaterInstallError, check_for_updates, check_for_updates_button, install_updater,
    updater_available,
};
pub use crate::account::client::OidcSettings as AccountOidcSettings;
pub use crate::account::client::{
    AccountAuthenticationError, AccountAuthenticationScope, AccountAuthenticator, AccountClient,
    AccountClientConfig, AccountClientConfigError, AccountClientError, AccountLogin,
    AccountLoginFailure, AccountLoginRuntimeError, AccountLoginSnapshot, AccountSession,
    ApiSettings, OidcClient, OidcConfig, OidcError, OidcSession, OidcTokenCache,
    PendingAccountLogin, PendingOidcLogin, Settings as AccountSettings, api_session,
    authentication_scope, client_config, contract, install_authenticator, is_authenticated,
    login_profile, login_session, login_snapshot, observe_authentication, oidc_config, sign_out,
    start_login,
};
pub use crate::application::{
    PanelHeaderAction, PersistentDataTableDelegate, install_panel_header_actions,
    persistent_crud_table_state, persistent_crud_table_state_with,
    persistent_data_table_state_with,
};
pub use ::updater::{
    CancellationToken, ReleaseStatus, SignedUpdateManifest, StagedUpdate, TrustedPublicKey,
    UpdateArtifact, UpdateChannel, UpdateConfig, UpdateError, UpdateEvent, UpdateManifest,
    UpdateManifestSignature, UpdateRelease, UpdateSession, UpdateTarget, Updater,
    report_health_from_env_args, run_sidecar_from_env_args,
};
pub use ui::{
    Cascader, CascaderEvent, CascaderOption, CascaderSelection, CascaderState, CascaderValueError,
    CrudPanel, CrudPanelToolbar, CrudTableColumnState, CrudTableDelegate, CrudTableRow,
    CrudTableSelection, CrudTableSort, CrudTableSortDirection, CrudTableState, Event, FieldValue,
    FieldValueParseError, FormDialog, FormDialogState, FormFieldDraft, FormItem, FormItemControl,
    LabeledControl, LabeledControlBuilder, LabeledControlTarget, LoadedRowsSelectionEvent,
    NumberFieldValue, RowSelectionEvent, SidebarRegion, TableCell, TableCellVerticalAlign,
    TableHeaderCell,
};
