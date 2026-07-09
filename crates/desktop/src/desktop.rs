//! 桌面应用运行时封装。
//!
//! 该模块负责统一创建 GPUI 应用、初始化 `gpui-component`，并根据应用配置打开主窗口。

use gpui::{
    App, AppContext, Entity, Pixels, QuitMode, Render, Size, Window, WindowBounds, WindowOptions,
    px, size,
};
use gpui_component::{Theme, scroll::ScrollbarShow};
use gpui_platform::application;

/// 桌面应用运行时选项。
///
/// 该结构保存所有 `with_*` 链式配置产生的启动参数，最终由运行器消费。
#[derive(Debug, Default)]
pub struct ApplicationOptions {
    /// 是否以守护模式启动应用。
    ///
    /// 为 `true` 时，运行器仍会按窗口配置创建主窗口，但会使用显式退出策略，
    /// 适合需要在窗口关闭后继续常驻后台的桌面进程。
    /// 为 `false` 时，应用使用默认退出策略创建主窗口。
    pub daemon_mode: bool,
    /// 创建主窗口后是否激活应用。
    pub activate: bool,
    /// GPUI 原生窗口选项。
    ///
    /// 为 `None` 时，运行器会使用 `WindowOptions::default()`。
    pub window_options: Option<WindowOptions>,
    /// 主窗口初始尺寸。
    ///
    /// 设置该值后，运行器会在创建主窗口时根据当前屏幕把窗口居中显示。
    /// 该配置只表达窗口宽高；当它和 `window_options.window_bounds` 同时存在时，
    /// 运行器会优先使用该尺寸生成居中窗口，其他窗口行为仍由 `window_options` 控制。
    pub window_size: Option<Size<Pixels>>,
    /// 主窗口允许缩放到的最小尺寸。
    ///
    /// 设置该值后，运行器会把它写入 GPUI 的 `WindowOptions::window_min_size`。
    /// 当它和 `window_options.window_min_size` 同时存在时，该字段会优先接管最小尺寸。
    pub window_min_size: Option<Size<Pixels>>,
}

/// 桌面应用抽象。
///
/// 实现该 trait 的类型只需要提供应用配置存储和业务根视图创建逻辑，
/// 默认 `run` 方法会负责 GPUI 启动、组件初始化和主窗口创建。
pub trait Application: Sized + 'static {
    /// 主窗口中的业务根视图类型。
    ///
    /// 该视图会先由应用实现方创建，再由运行器包裹进 `gpui_component::Root`。
    type RootView: Render + 'static;

    /// 返回当前应用运行时选项。
    ///
    /// 该方法用于读取应用在链式配置中积累的启动参数。
    fn options(&self) -> &ApplicationOptions;

    /// 返回当前应用运行时选项的可变引用。
    ///
    /// 默认 `with_*` 方法通过该引用写入启动参数。
    fn options_mut(&mut self) -> &mut ApplicationOptions;

    /// 创建主窗口中的业务根视图。
    ///
    /// 实现方应在该方法中使用 `cx.new` 创建自己的根视图实体。
    /// 运行器会把返回的实体作为内容嵌入 `gpui_component::Root`。
    fn build_root_view(&mut self, window: &mut Window, cx: &mut App) -> Entity<Self::RootView>;

    /// 启动桌面应用。
    ///
    /// 该默认实现会消费应用实例，读取当前选项，并进入统一的 GPUI 运行流程。
    fn run(mut self) {
        let options = std::mem::take(self.options_mut());

        run_application(options, move |window, cx| self.build_root_view(window, cx));
    }

    /// 使用完整运行时选项替换当前配置。
    ///
    /// 适合调用方已经构造好 `ApplicationOptions`，并希望一次性覆盖全部启动参数的场景。
    fn with_options(mut self, options: ApplicationOptions) -> Self {
        *self.options_mut() = options;
        self
    }

    /// 设置应用是否以守护模式启动。
    ///
    /// 传入 `true` 时，应用仍会创建主窗口，但会使用显式退出策略，避免最后一个窗口关闭时进程自动退出；
    /// 传入 `false` 时，应用使用 GPUI 默认退出策略。该方法会返回更新后的应用实例，以支持继续链式配置。
    fn with_daemon_mode(mut self, daemon_mode: bool) -> Self {
        self.options_mut().daemon_mode = daemon_mode;
        self
    }

    /// 设置创建主窗口后是否激活应用。
    ///
    /// 当运行器创建主窗口后，该值决定是否调用 `App::activate(true)`。
    fn with_activate(mut self, activate: bool) -> Self {
        self.options_mut().activate = activate;
        self
    }

    /// 设置主窗口的 GPUI 原生窗口选项。
    ///
    /// 传入的 `WindowOptions` 会在创建主窗口时使用。
    fn with_window_options(mut self, window_options: WindowOptions) -> Self {
        self.options_mut().window_options = Some(window_options);
        self
    }

    /// 设置主窗口初始尺寸。
    ///
    /// `width` 和 `height` 使用 GPUI 逻辑像素作为单位。运行器会在 `run` 阶段，
    /// 等 `App` 上下文可用后，把该尺寸转换成居中的 `WindowBounds`。
    /// 如果同时设置了 `WindowOptions::window_bounds`，该尺寸配置会接管窗口 bounds。
    fn with_window_size(mut self, width: f32, height: f32) -> Self {
        self.options_mut().window_size = Some(size(px(width), px(height)));
        self
    }

    /// 设置主窗口允许缩放到的最小尺寸。
    ///
    /// `width` 和 `height` 使用 GPUI 逻辑像素作为单位。该配置会在创建主窗口前写入
    /// `WindowOptions::window_min_size`，用于限制用户手动缩放窗口时的最小宽高。
    /// 如果同时设置了 `WindowOptions::window_min_size`，该方法传入的尺寸会优先生效。
    fn with_window_min_size(mut self, width: f32, height: f32) -> Self {
        self.options_mut().window_min_size = Some(size(px(width), px(height)));
        self
    }
}

/// 使用指定选项和根视图工厂启动 GPUI 应用。
///
/// 该函数负责创建平台应用、挂载 `gpui-component` 资源、初始化组件库，
/// 并按运行时选项创建包裹了 `gpui_component::Root` 的主窗口。
fn run_application<V, F>(options: ApplicationOptions, build_root_view: F)
where
    V: Render + 'static,
    F: FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
{
    let plan = runtime_plan(&options);
    let ApplicationOptions {
        daemon_mode: _,
        activate,
        window_options,
        window_size,
        window_min_size,
    } = options;

    application()
        .with_assets(gpui_component_assets::Assets)
        .with_quit_mode(plan.quit_mode)
        .run(move |cx| {
            // 初始化 gpui component
            gpui_component::init(cx);
            Theme::global_mut(cx).scrollbar_show = ScrollbarShow::Hover;

            if !plan.open_startup_window {
                return;
            }

            let mut window_options = window_options.unwrap_or_default();

            if let Some(window_size) = window_size {
                window_options.window_bounds = Some(WindowBounds::centered(window_size, cx));
            }

            if let Some(window_min_size) = window_min_size {
                window_options.window_min_size = Some(window_min_size);
            }

            cx.open_window(window_options, |window, cx| {
                let view = build_root_view(window, cx);
                cx.new(|cx| gpui_component::Root::new(view, window, cx))
            })
            .unwrap();

            if activate {
                cx.activate(true);
            }
        });
}

/// 应用启动时由运行器执行的内部计划。
///
/// 该计划把用户配置拆分成运行器真正关心的行为，避免把守护模式和是否创建窗口混在一起。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ApplicationRuntimePlan {
    /// GPUI 应用退出策略。
    quit_mode: QuitMode,
    /// 启动时是否创建主窗口。
    open_startup_window: bool,
}

/// 根据应用选项生成运行计划。
///
/// 守护模式只影响退出策略，不影响启动时是否创建主窗口。
fn runtime_plan(options: &ApplicationOptions) -> ApplicationRuntimePlan {
    let quit_mode = match options.daemon_mode {
        true => QuitMode::Explicit,
        false => QuitMode::Default,
    };

    ApplicationRuntimePlan {
        quit_mode,
        open_startup_window: true,
    }
}
