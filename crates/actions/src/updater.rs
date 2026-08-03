//! 桌面应用更新相关 action。
//!
//! 更新入口通过同一个 GPUI action 连接原生菜单、登录页、账户菜单和设置窗口。

gpui::actions!(
    nexora_updater,
    [
        /// 在当前活动窗口中打开公共更新检查流程。
        CheckForUpdates
    ]
);
