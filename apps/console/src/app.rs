//! 控制台桌面应用定义。
//!
//! 该模块实现 `desktop::Application`，把控制台应用接入统一的桌面启动流程。

use crate::features::root::RootView;
use actions::account as account_actions;
use desktop::{Application, ApplicationOptions};
use gpui::{App, AppContext, Entity, Window, WindowOptions, px, size};
use gpui_component::TitleBar;
use std::default::Default;

/// 控制台桌面应用。
///
/// 该类型保存应用启动选项，并负责创建主窗口中的业务根视图。
#[derive(Default)]
pub struct Console {
    /// 控制台应用的运行时配置。
    options: ApplicationOptions,
}

impl Console {
    /// 创建一个使用默认启动选项的控制台应用。
    ///
    /// 调用方可以继续通过 `with_*` 方法覆盖守护模式、激活行为、窗口尺寸、最小窗口尺寸或窗口选项。
    pub fn new() -> Self {
        Self {
            options: ApplicationOptions {
                window_size: Some(size(px(900.), px(640.))),
                window_min_size: Some(size(px(900.), px(640.))),
                activate: true,
                window_options: Some(WindowOptions {
                    titlebar: Some(TitleBar::title_bar_options()),
                    ..Default::default()
                }),
                daemon_mode: false,
                ..Default::default()
            },
        }
    }
}

impl Application for Console {
    /// 控制台应用主窗口中的业务根视图类型。
    ///
    /// 该类型负责渲染控制台窗口的最外层业务内容。
    type RootView = RootView;

    /// 返回控制台应用当前的运行时配置。
    ///
    /// 该配置会在调用 `run` 时被桌面运行器读取并消费。
    fn options(&self) -> &ApplicationOptions {
        &self.options
    }

    /// 返回控制台应用运行时配置的可变引用。
    ///
    /// 桌面运行器提供的链式配置方法会通过该引用写入启动参数。
    fn options_mut(&mut self) -> &mut ApplicationOptions {
        &mut self.options
    }

    /// 创建控制台应用的根视图实体。
    ///
    /// 当前实现创建 `features::root::RootView`，该实体会由桌面运行器包裹进
    /// `gpui_component::Root` 后作为窗口根节点。
    fn build_root_view(&mut self, _window: &mut Window, cx: &mut App) -> Entity<Self::RootView> {
        actions::init();
        account_actions::bind_keys(cx);
        cx.new(|_| RootView::new())
    }
}
