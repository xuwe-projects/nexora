//! Nexora 桌面运行时自带的默认专用界面。
//!
//! 默认登录页和设置窗口只作为注册表没有发现应用覆盖类型时的回退工厂，不会向
//! `inventory` 主动提交注册记录。

#[cfg(feature = "desktop")]
mod account;
#[cfg(feature = "desktop")]
mod login;
#[cfg(feature = "desktop")]
mod settings;

#[cfg(feature = "desktop")]
pub(crate) use account::default_account_feature_registrations;
#[cfg(feature = "desktop")]
pub(crate) use login::default_login_registration;
#[cfg(feature = "desktop")]
pub(crate) use settings::default_settings_window_registration;
