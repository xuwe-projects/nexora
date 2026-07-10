//! 桌面窗口相关 action。
//!
//! 该模块把原生应用菜单、键盘快捷键和 GPUI 活动窗口操作连接到同一组 action，
//! 确保 macOS 全屏后仍能通过系统菜单或快捷键可靠退出全屏。

use gpui::{App, Global, KeyBinding, Menu, MenuItem, SharedString};

gpui::actions!(
    desktop_window,
    [
        /// 退出当前桌面应用。
        QuitApplication,
        /// 最小化当前活动窗口。
        MinimizeWindow,
        /// 在当前活动窗口的标准尺寸与缩放尺寸之间切换。
        ZoomWindow,
        /// 切换当前活动窗口的原生全屏状态。
        ToggleFullScreen
    ]
);

#[derive(Default)]
struct WindowActionsState;

impl Global for WindowActionsState {}

/// 初始化桌面窗口 action、快捷键与 macOS 系统菜单。
///
/// `application_name` 会作为 macOS 菜单栏中的应用菜单名称。初始化过程使用 GPUI
/// [`Global`] 标记保持幂等，因此同一进程创建多个窗口时不会重复注册全局 action 处理器。
/// 窗口命令始终作用于操作发生时的活动窗口。
pub fn init(application_name: impl Into<SharedString>, cx: &mut App) {
    if cx.has_global::<WindowActionsState>() {
        return;
    }

    cx.set_global(WindowActionsState);
    bind_keys(cx);
    register_handlers(cx);
    configure_application_menus(application_name.into(), cx);
}

/// 构建桌面应用使用的标准系统菜单。
///
/// 返回菜单包含应用退出入口以及名为 `Window` 的窗口菜单。`Window` 名称需要保持英文，
/// 以便 GPUI 的 macOS 后端把它注册为 AppKit 的标准窗口菜单，并在原生全屏时正常显示。
pub fn application_menus(application_name: impl Into<SharedString>) -> Vec<Menu> {
    let application_name = application_name.into();

    vec![
        Menu::new(application_name.clone()).items([MenuItem::action(
            format!("Quit {application_name}"),
            QuitApplication,
        )]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", MinimizeWindow),
            MenuItem::action("Zoom", ZoomWindow),
            MenuItem::separator(),
            MenuItem::action("Toggle Full Screen", ToggleFullScreen),
        ]),
    ]
}

fn bind_keys(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-q", QuitApplication, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
        KeyBinding::new("fn-f", ToggleFullScreen, None),
    ]);

    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([KeyBinding::new("f11", ToggleFullScreen, None)]);
}

fn register_handlers(cx: &mut App) {
    cx.on_action(|_: &QuitApplication, cx| cx.quit());
    cx.on_action(|_: &MinimizeWindow, cx| {
        if let Some(window) = cx.active_window() {
            _ = window.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.on_action(|_: &ZoomWindow, cx| {
        if let Some(window) = cx.active_window() {
            _ = window.update(cx, |_, window, _| window.zoom_window());
        }
    });
    cx.on_action(|_: &ToggleFullScreen, cx| {
        if let Some(window) = cx.active_window() {
            _ = window.update(cx, |_, window, _| window.toggle_fullscreen());
        }
    });
}

#[cfg(target_os = "macos")]
fn configure_application_menus(application_name: SharedString, cx: &mut App) {
    cx.set_menus(application_menus(application_name));
}

#[cfg(not(target_os = "macos"))]
fn configure_application_menus(_application_name: SharedString, _cx: &mut App) {}
