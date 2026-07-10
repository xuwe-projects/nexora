//! 应用设置相关 action。
//!
//! 该模块统一声明打开设置窗口的命令和跨平台默认快捷键，使菜单、快捷键和窗口处理器共享同一 action 身份。

use gpui::{App, KeyBinding};

gpui::actions!(
    application_settings,
    [
        /// 打开或激活应用设置窗口。
        OpenSettings
    ]
);

/// 返回当前平台展示给用户的设置快捷键文案。
///
/// macOS 使用系统约定的 `Cmd+,`，Windows 和 Linux 使用 `Ctrl+,`。
pub const fn shortcut_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+,"
    } else {
        "Ctrl+,"
    }
}

/// 注册打开应用设置窗口的默认快捷键。
///
/// 快捷键不限制 key context，因此主窗口内无论当前焦点位于导航、表格还是输入控件，
/// 都会派发同一个 `OpenSettings` action。
pub fn bind_keys(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.bind_keys([KeyBinding::new("cmd-,", OpenSettings, None)]);

    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([KeyBinding::new("ctrl-,", OpenSettings, None)]);
}
