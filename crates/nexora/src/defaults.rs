//! Nexora 桌面运行时自带的默认专用界面。
//!
//! 默认登录页和设置窗口只作为注册表没有发现应用覆盖类型时的回退工厂，不会向
//! `inventory` 主动提交注册记录。

#[cfg(feature = "account-client")]
#[path = "defaults/login.rs"]
mod login;
#[cfg(feature = "desktop")]
#[path = "defaults/settings.rs"]
mod settings;

#[cfg(feature = "account-client")]
pub(crate) use login::default_login_registration;
#[cfg(feature = "desktop")]
pub(crate) use settings::default_settings_window_registration;
