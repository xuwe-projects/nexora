//! 桌面应用更新相关 action。
//!
//! 更新入口通过同一个 GPUI action 连接原生菜单、登录页、账户菜单和设置窗口。

use gpui::{App, KeyBinding};

gpui::actions!(
    nexora_updater,
    [
        /// 在当前活动窗口中打开公共更新检查流程。
        CheckForUpdates
    ]
);

/// 返回当前平台展示给用户的检查更新快捷键文案。
///
/// macOS 使用 `Cmd+Shift+U`，Windows 和 Linux 使用 `Ctrl+Shift+U`。
pub const fn shortcut_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+Shift+U"
    } else {
        "Ctrl+Shift+U"
    }
}

/// 注册检查应用更新的默认快捷键。
///
/// 快捷键不限制 key context，因此安装 updater 后可从当前活动窗口的任意焦点位置触发
/// 同一个 [`CheckForUpdates`] action。
pub fn bind_keys(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.bind_keys([KeyBinding::new("cmd-shift-u", CheckForUpdates, None)]);

    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([KeyBinding::new("ctrl-shift-u", CheckForUpdates, None)]);
}
