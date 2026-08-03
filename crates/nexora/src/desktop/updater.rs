//! Nexora 桌面 Shell 的公共 updater 安装与入口协调。

use actions::{updater::CheckForUpdates, window as window_actions};
use gpui::{App, ElementId, Global, Window};
use gpui_component::{IconName, button::Button};
use thiserror::Error;

use ::updater::{
    UpdateConfig, open_update_dialog, report_health_from_env_args, start_update_check_on_launch,
};

#[derive(Clone)]
struct InstalledUpdater {
    config: UpdateConfig,
}

impl Global for InstalledUpdater {}

/// 安装应用级 updater 时可能返回的错误。
#[derive(Debug, Error)]
pub enum UpdaterInstallError {
    /// 当前进程已经安装过 updater；应用必须只维护一份 app 级更新配置。
    #[error("当前应用已经安装 updater，不能重复安装第二份配置")]
    AlreadyInstalled,
}

/// 在 Nexora 桌面运行时安装一份 app 级 updater 配置。
///
/// 安装成功后会注册全局 [`CheckForUpdates`] Action，并启用 macOS 原生菜单入口。该函数
/// 不发起网络请求；启动后台检查会在主窗口创建后由 Shell 根据配置单独触发。
///
/// # Errors
///
/// 同一进程已经安装 updater 时返回 [`UpdaterInstallError::AlreadyInstalled`]，且不会替换
/// 现有配置或创建第二个会话协调器。
pub fn install_updater(config: UpdateConfig, cx: &mut App) -> Result<(), UpdaterInstallError> {
    if cx.has_global::<InstalledUpdater>() {
        return Err(UpdaterInstallError::AlreadyInstalled);
    }
    cx.set_global(InstalledUpdater { config });
    cx.on_action(|_: &CheckForUpdates, cx| {
        if let Some(window) = cx.active_window() {
            _ = window.update(cx, |_, window, cx| {
                _ = check_for_updates(window, cx);
            });
        }
    });
    window_actions::enable_update_menu(cx);
    Ok(())
}

/// 返回当前应用是否已经显式安装 updater。
///
/// Shell、默认登录页、账户菜单和设置窗口都使用该查询决定是否展示更新入口。
pub fn updater_available(cx: &App) -> bool {
    cx.has_global::<InstalledUpdater>()
}

/// 在当前窗口打开公共更新检查流程。
///
/// 多个入口同时调用时会复用 updater crate 的应用级会话协调器，不会并发检查或下载。
///
/// 返回值以 `false` 表示 updater 尚未安装，以 `true` 表示请求已交给协调器。
pub fn check_for_updates(window: &mut Window, cx: &mut App) -> bool {
    let Some(installed) = cx.try_global::<InstalledUpdater>() else {
        return false;
    };
    let config = installed.config.clone();
    open_update_dialog(config, window, cx);
    true
}

/// 构造一个连接公共更新 Action 的可复用按钮。
///
/// updater 未安装时返回 `None`，调用方应直接隐藏入口；按钮使用组件库的标准图标、焦点和
/// 键盘语义，自定义登录页可以按自己的布局放置它。
pub fn check_for_updates_button(id: impl Into<ElementId>, cx: &App) -> Option<Button> {
    updater_available(cx).then(|| {
        Button::new(id)
            .icon(IconName::CircleCheck)
            .label("检查更新")
            .on_click(|_, window, cx| {
                _ = check_for_updates(window, cx);
            })
    })
}

pub(crate) fn start_installed_updater(window: &mut Window, cx: &mut App) {
    let Some(installed) = cx.try_global::<InstalledUpdater>() else {
        return;
    };
    let config = installed.config.clone();
    window.defer(cx, move |window, cx| {
        if config.health_report_on_launch()
            && let Err(error) = report_health_from_env_args()
        {
            tracing::warn!(error = %error, "无法报告 updater 启动健康状态");
        }
        if config.check_on_launch() {
            start_update_check_on_launch(config, window, cx);
        }
    });
}
