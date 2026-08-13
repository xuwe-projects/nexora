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
    login_profile, login_session, login_snapshot, login_with_other_account, observe_authentication,
    oidc_config, retry_recovery, set_remember_login, sign_out, start_login,
};
pub use crate::application::{
    PersistentDataTableDelegate, ShellToolbarAction, install_shell_toolbar_actions,
    persistent_crud_table_state, persistent_data_table_state, reset_data_table_layout,
};
pub use crate::application_info::{ApplicationInfo, application_info};
pub use crate::global_search::{
    SearchAction, SearchActionError, SearchHistoryEntry, SearchItem, SearchMode, SearchProvider,
    SearchProviderError, SearchRequest, SearchSection, install_search_providers,
};
pub use ::updater::{
    CancellationToken, ReleaseStatus, SignedUpdateManifest, StagedUpdate, TrustedPublicKey,
    UpdateArtifact, UpdateChannel, UpdateConfig, UpdateError, UpdateEvent, UpdateManifest,
    UpdateManifestSignature, UpdateRelease, UpdateSession, UpdateTarget, Updater,
    report_health_from_env_args, run_sidecar_from_env_args,
};
pub use contracts::{crud_query::NoCrudSort, pagination::PageQuery};
pub use theme::{ColorScheme, ThemePresetMetadata, ThemeSelection, ThemeSelectionError};
pub use ui::{
    Cascader, CascaderEvent, CascaderOption, CascaderSelection, CascaderState, CascaderValueError,
    CrudColumnSort, CrudListState, CrudListStateError, CrudLoadError, CrudPage, CrudPanel,
    CrudTableDelegate, CrudTableRow, CrudTableSelection, DataTableColumnLayout, DataTableLayout,
    DataTableLayoutError, DataTableLayoutKey, FieldValue, FieldValueParseError, FormDialog,
    FormDialogState, FormFieldDraft, FormFieldEvent, FormFieldState, FormFieldStateBuilder,
    FormFieldTarget, LoadedRowsSelectionEvent, NumberFieldValue, RowSelectionEvent, SidebarRegion,
    TableCell, TableCellVerticalAlign, TableHeaderCell, TableSwitchCell,
};

/// 返回当前应用已注册的全部主题预设元数据。
///
/// 内置 Nexora 始终位于首项，下游主题按 `ApplicationOptions::theme_preset(...)` 的调用
/// 顺序排列。返回值借用应用级主题目录，不复制完整主题配置。
///
/// # Panics
///
/// 在 Nexora 完成主题初始化前调用时会 panic；普通应用只能在 `Application::initialize`
/// 及其后的生命周期中调用。
pub fn theme_presets(cx: &gpui::App) -> impl ExactSizeIterator<Item = &ThemePresetMetadata> {
    theme::presets(cx)
}

/// 返回当前应用声明的默认主题预设 ID。
///
/// 未显式配置 `ApplicationOptions::default_theme_preset(...)` 时返回 `nexora`。
///
/// # Panics
///
/// 在 Nexora 完成主题初始化前调用时会 panic。
pub fn default_theme_preset_id(cx: &gpui::App) -> &str {
    theme::default_preset_id(cx)
}

/// 返回当前应用正在使用的主题预设与颜色模式。
///
/// # Panics
///
/// 在 Nexora 完成主题初始化前调用时会 panic。
pub fn theme_selection(cx: &gpui::App) -> ThemeSelection {
    theme::selection(cx)
}

/// 切换到一个已注册主题预设，立即刷新所有窗口并自动持久化。
///
/// 颜色模式保持不变；处于跟随系统模式时，新预设会继续根据系统外观选择浅色或深色主题。
///
/// # Errors
///
/// `preset_id` 未注册时返回 [`ThemeSelectionError::UnknownPreset`]，当前主题和持久化偏好
/// 均保持不变。
pub fn set_theme_preset(preset_id: &str, cx: &mut gpui::App) -> Result<(), ThemeSelectionError> {
    theme::set_preset(preset_id, cx)?;
    crate::application::persist_current_appearance_preferences(cx);
    Ok(())
}

/// 切换颜色模式，立即刷新所有窗口并自动持久化。
///
/// 主题预设保持不变；`ColorScheme::System` 会继续响应各窗口的系统外观变化。
///
/// # Panics
///
/// 在 Nexora 完成主题初始化前调用时会 panic。
pub fn set_color_scheme(color_scheme: ColorScheme, cx: &mut gpui::App) {
    theme::set_color_scheme(color_scheme, cx);
    crate::application::persist_current_appearance_preferences(cx);
}
