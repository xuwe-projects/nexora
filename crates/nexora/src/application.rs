//! Nexora 桌面应用启动契约与通用 Feature Shell。
//!
//! 应用实现方只负责提供启动选项和初始化自己的全局状态；注册表发现、首路由校验、
//! 主窗口创建以及 Feature Entity 的生命周期由框架统一管理。

use std::collections::HashMap;

use desktop::{Application as DesktopApplication, ApplicationOptions as DesktopApplicationOptions};
use gpui::{
    AnyElement, AnyView, App, Context, IntoElement as _, Pixels, Render, Size, Subscription,
    Window, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme as _, IconName,
    alert::Alert,
    h_flex,
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter as SidebarFooterContainer, SidebarGroup,
        SidebarHeader as SidebarHeaderContainer, SidebarMenu, SidebarMenuItem,
    },
    tab::{Tab, TabBar},
    v_flex,
};
use thiserror::Error;

use crate::{
    AppRegistry, FeatureInstance, FeatureMetadata, FeatureRuntimeError, RegistryError,
    ResolveError, RouteMatch, RouteTargetKind, WindowRuntimeError,
    runtime::{clear_navigation_handler, install_navigation_handler},
};

/// Nexora 桌面应用的启动选项。
///
/// 默认值会创建一个 `900 × 640` 的主窗口、限制最小尺寸为 `640 × 480`、主动激活应用，
/// 并以中文和根路径 `/` 启动。应用只需要覆盖与自身产品有关的字段。
#[derive(Debug)]
pub struct ApplicationOptions {
    /// 是否在最后一个窗口关闭后继续保持应用进程运行。
    pub daemon_mode: bool,
    /// 创建主窗口后是否主动激活应用。
    pub activate: bool,
    /// 需要直接传递给 GPUI 的原生窗口选项。
    ///
    /// 为 `None` 时使用 GPUI 默认窗口选项。
    pub window_options: Option<WindowOptions>,
    /// 主窗口的初始逻辑像素尺寸。
    ///
    /// 为 `None` 时由 GPUI 或调用方提供的 [`Self::window_options`] 决定。
    pub window_size: Option<Size<Pixels>>,
    /// 主窗口允许缩放到的最小逻辑像素尺寸。
    pub window_min_size: Option<Size<Pixels>>,
    /// 启动时优先使用的显示器稳定 UUID。
    ///
    /// 对应显示器不存在时，底层桌面运行时会安全回退到系统主显示器。
    pub startup_display_uuid: Option<String>,
    /// `gpui-component` 使用的界面语言，例如 `zh-CN` 或 `en`。
    pub locale: String,
    /// 主窗口创建后首先打开的 Feature 路径或 deeplink。
    ///
    /// 该位置会在进入 GPUI 事件循环前完成注册表匹配，并且必须指向 Feature。
    pub initial_path: String,
}

impl Default for ApplicationOptions {
    fn default() -> Self {
        Self {
            daemon_mode: false,
            activate: true,
            window_options: None,
            window_size: Some(size(px(900.0), px(640.0))),
            window_min_size: Some(size(px(640.0), px(480.0))),
            startup_display_uuid: None,
            locale: "zh-CN".to_owned(),
            initial_path: "/".to_owned(),
        }
    }
}

impl ApplicationOptions {
    /// 创建一份可以直接启动标准 Nexora 桌面程序的默认选项。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置主窗口首先打开的 Feature 路径或 deeplink。
    pub fn initial_path(mut self, initial_path: impl Into<String>) -> Self {
        self.initial_path = initial_path.into();
        self
    }

    /// 设置 `gpui-component` 使用的界面语言。
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = locale.into();
        self
    }

    /// 设置主窗口的初始逻辑像素尺寸。
    pub fn window_size(mut self, width: f32, height: f32) -> Self {
        self.window_size = Some(size(px(width), px(height)));
        self
    }

    /// 设置主窗口允许缩放到的最小逻辑像素尺寸。
    pub fn window_min_size(mut self, width: f32, height: f32) -> Self {
        self.window_min_size = Some(size(px(width), px(height)));
        self
    }

    /// 设置主窗口创建后是否主动激活应用。
    pub const fn activate(mut self, activate: bool) -> Self {
        self.activate = activate;
        self
    }

    /// 设置应用在最后一个窗口关闭后是否继续常驻。
    pub const fn daemon_mode(mut self, daemon_mode: bool) -> Self {
        self.daemon_mode = daemon_mode;
        self
    }

    /// 设置需要直接传递给 GPUI 的原生窗口选项。
    pub fn window_options(mut self, window_options: WindowOptions) -> Self {
        self.window_options = Some(window_options);
        self
    }

    /// 设置启动时优先使用的显示器稳定 UUID。
    pub fn startup_display_uuid(mut self, display_uuid: impl Into<String>) -> Self {
        self.startup_display_uuid = Some(display_uuid.into());
        self
    }

    fn into_desktop_options(self) -> DesktopApplicationOptions {
        DesktopApplicationOptions {
            daemon_mode: self.daemon_mode,
            activate: self.activate,
            window_options: self.window_options,
            window_size: self.window_size,
            window_min_size: self.window_min_size,
            startup_display_uuid: self.startup_display_uuid,
        }
    }
}

/// 启动 Nexora 桌面应用时可能发生的结构化错误。
///
/// 注册表和首路由错误会在进入原生事件循环前返回，因此 CLI 生成的程序可以直接使用
/// `?` 把错误报告给调用环境，而不会先创建一个不完整的窗口。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApplicationError {
    /// 自动发现的 Feature 或 Window 元数据无效或互相冲突。
    #[error(transparent)]
    Registry(
        /// 注册表在标识、路径或父子导航校验阶段返回的具体错误。
        #[from]
        RegistryError,
    ),

    /// 配置的首路由无法由当前应用注册表解析。
    #[error("无法解析应用首路由 `{path}`：{source}")]
    InitialRoute {
        /// 应用选项中配置的原始路径或 deeplink。
        path: String,
        /// 注册表返回的具体解析错误。
        #[source]
        source: ResolveError,
    },

    /// 配置的首路由指向独立窗口，不能作为主窗口 Feature 内容。
    #[error("应用首路由 `{path}` 指向 Window `{id}`，必须配置为 Feature 路径")]
    InitialRouteIsWindow {
        /// 应用选项中配置的原始路径或 deeplink。
        path: String,
        /// 被首路由匹配到的 Window 稳定标识。
        id: &'static str,
    },
}

/// Nexora 桌面应用的最小实现契约。
///
/// 框架负责自动发现 Feature、校验首路由、创建主窗口和通用导航 Shell。应用通常只需
/// 实现 [`Self::options`]；需要注册业务 Global、Action 或服务时再覆盖
/// [`Self::initialize`]。
///
/// # Examples
///
/// ```no_run
/// use nexora::{Application as _, ApplicationOptions};
///
/// struct DesktopApplication;
///
/// impl nexora::Application for DesktopApplication {
///     fn options(&self) -> ApplicationOptions {
///         ApplicationOptions::new().initial_path("/")
///     }
/// }
///
/// DesktopApplication.run()?;
/// # Ok::<(), nexora::ApplicationError>(())
/// ```
pub trait Application: Sized + 'static {
    /// 返回本次启动使用的应用选项。
    ///
    /// 默认实现会打开一个可直接使用的标准窗口；应用可以按值构造并返回自己的配置，
    /// 不需要在类型中保存一份仅供框架修改的可变选项。
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::default()
    }

    /// 在组件库初始化完成后、主窗口创建前初始化应用自己的全局状态。
    ///
    /// 这里适合注册 Global、Action、服务和恢复本地偏好。Feature Entity 和首路由由
    /// 框架随后创建，应用不需要自行组装 RootView。
    fn initialize(&mut self, _cx: &mut App) {}

    /// 校验自动发现的注册表和配置的首路由。
    ///
    /// 该方法不会启动 GPUI 或创建窗口，可以用于测试、诊断和启动前检查。
    ///
    /// # Errors
    ///
    /// 注册表无效、首路由无法解析，或首路由指向独立 Window 时返回错误。
    fn validate(&self) -> Result<(), ApplicationError> {
        prepare_application(&self.options()).map(|_| ())
    }

    /// 启动 Nexora 桌面应用并进入 GPUI 事件循环。
    ///
    /// 框架会先完成与 [`Self::validate`] 相同的同步校验。只有校验成功后才初始化原生
    /// 应用、创建主窗口并打开首个 Feature。
    ///
    /// # Errors
    ///
    /// 注册表无效、首路由无法解析，或首路由指向独立 Window 时返回错误。
    fn run(self) -> Result<(), ApplicationError> {
        run_application(self)
    }
}

struct PreparedApplication {
    registry: AppRegistry,
    initial_route: RouteMatch,
}

fn prepare_application(
    options: &ApplicationOptions,
) -> Result<PreparedApplication, ApplicationError> {
    let registry = AppRegistry::discover()?;
    let initial_route = registry
        .resolve(options.initial_path.as_str())
        .map_err(|source| ApplicationError::InitialRoute {
            path: options.initial_path.clone(),
            source,
        })?;
    if initial_route.target().kind() == RouteTargetKind::Window {
        return Err(ApplicationError::InitialRouteIsWindow {
            path: options.initial_path.clone(),
            id: initial_route.target().id(),
        });
    }

    Ok(PreparedApplication {
        registry,
        initial_route,
    })
}

fn run_application<A>(application: A) -> Result<(), ApplicationError>
where
    A: Application,
{
    let options = application.options();
    let PreparedApplication {
        registry,
        initial_route,
    } = prepare_application(&options)?;
    let locale = options.locale.clone();
    let adapter = ApplicationAdapter {
        application,
        options: options.into_desktop_options(),
        locale,
        registry: Some(registry),
        initial_route: Some(initial_route),
    };

    DesktopApplication::run(adapter);
    Ok(())
}

struct ApplicationAdapter<A> {
    application: A,
    options: DesktopApplicationOptions,
    locale: String,
    registry: Option<AppRegistry>,
    initial_route: Option<RouteMatch>,
}

impl<A> DesktopApplication for ApplicationAdapter<A>
where
    A: Application,
{
    type RootView = ApplicationShell;

    fn options(&self) -> &DesktopApplicationOptions {
        &self.options
    }

    fn options_mut(&mut self) -> &mut DesktopApplicationOptions {
        &mut self.options
    }

    fn initialize(&mut self, cx: &mut App) {
        gpui_component::set_locale(self.locale.as_str());
        self.application.initialize(cx);
    }

    fn build_root_view(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Entity<Self::RootView> {
        let registry = self
            .registry
            .take()
            .expect("Nexora 主窗口注册表只能被消费一次");
        let initial_route = self
            .initial_route
            .take()
            .expect("Nexora 主窗口首路由只能被消费一次");

        cx.new(|cx| ApplicationShell::new(registry, initial_route, window, cx))
    }
}

#[derive(Debug, Error)]
enum NavigationError {
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Feature(#[from] FeatureRuntimeError),
    #[error(transparent)]
    Window(#[from] WindowRuntimeError),
}

struct ApplicationShell {
    registry: AppRegistry,
    active_path: String,
    opened_tabs: Vec<RouteMatch>,
    feature_instances: HashMap<String, FeatureInstance>,
    sidebar_header: Option<AnyView>,
    sidebar_footer: Option<AnyView>,
    navigation_error: Option<String>,
    _release_subscription: Option<Subscription>,
}

impl ApplicationShell {
    fn new(
        registry: AppRegistry,
        initial_route: RouteMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let shell = cx.entity().downgrade();
        install_navigation_handler(
            move |location, cx| {
                _ = shell.update_in(cx, move |this, window, cx| {
                    this.handle_navigation_request(location, window, cx);
                });
            },
            cx,
        );
        let sidebar_header = registry.create_sidebar_header(window, cx);
        let sidebar_footer = registry.create_sidebar_footer(window, cx);
        let active_path = initial_route.concrete_path().to_owned();
        let mut feature_instances = HashMap::new();
        let navigation_error = match registry.create_feature(initial_route.clone(), window, cx) {
            Ok(mut instance) => {
                instance.activate(window, cx);
                feature_instances.insert(active_path.clone(), instance);
                None
            }
            Err(error) => Some(error.to_string()),
        };
        let _release_subscription = Some(cx.on_release_in(window, |this, window, cx| {
            clear_navigation_handler(cx);
            for (_, mut instance) in this.feature_instances.drain() {
                instance.close(window, cx);
            }
        }));

        Self {
            registry,
            active_path,
            opened_tabs: vec![initial_route],
            feature_instances,
            sidebar_header,
            sidebar_footer,
            navigation_error,
            _release_subscription,
        }
    }

    fn handle_navigation_request(
        &mut self,
        location: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.open_path(location.as_str(), window, cx) {
            self.navigation_error = Some(error.to_string());
            cx.notify();
        }
    }

    fn open_path(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NavigationError> {
        let route = self.registry.resolve(path)?;
        self.open_route(route, window, cx)
    }

    fn open_route(
        &mut self,
        route: RouteMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NavigationError> {
        if route.target().kind() == RouteTargetKind::Window {
            self.registry.open_window(route, cx)?;
            self.navigation_error = None;
            return Ok(());
        }

        let target_path = route.concrete_path().to_owned();
        if let Some(instance) = self.feature_instances.get_mut(target_path.as_str()) {
            instance.update_route(route.clone(), window, cx)?;
        } else {
            let instance = self.registry.create_feature(route.clone(), window, cx)?;
            self.feature_instances.insert(target_path.clone(), instance);
        }

        if self.active_path != target_path
            && let Some(previous) = self.feature_instances.get_mut(self.active_path.as_str())
        {
            previous.deactivate(window, cx);
        }
        self.feature_instances
            .get_mut(target_path.as_str())
            .expect("刚创建或复用的 Feature 实例必须存在")
            .activate(window, cx);

        if let Some(tab) = self
            .opened_tabs
            .iter_mut()
            .find(|tab| tab.concrete_path() == target_path)
        {
            *tab = route;
        } else {
            self.opened_tabs.push(route);
        }
        self.active_path = target_path;
        self.navigation_error = None;
        cx.notify();
        Ok(())
    }

    fn render_navigation_item(
        &self,
        metadata: FeatureMetadata,
        cx: &mut Context<Self>,
    ) -> SidebarMenuItem {
        let children = self
            .registry
            .children_of(metadata.id())
            .map(|child| self.render_navigation_item(child, cx))
            .collect::<Vec<_>>();
        let path = metadata.path();
        let item = SidebarMenuItem::new(metadata.title());
        let item = match metadata.icon().and_then(feature_icon) {
            Some(icon) => item.icon(icon),
            None => item,
        };
        let item = item
            .active(self.active_target_id() == metadata.id())
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Err(error) = this.open_path(path, window, cx) {
                    this.navigation_error = Some(error.to_string());
                    cx.notify();
                }
            }));

        if children.is_empty() {
            item
        } else {
            item.default_open(true)
                .click_to_toggle(true)
                .children(children)
        }
    }

    fn active_target_id(&self) -> &'static str {
        self.opened_tabs
            .iter()
            .find(|route| route.concrete_path() == self.active_path)
            .map(|route| route.target().id())
            .expect("活动 Feature 必须具有对应标签")
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let sections = self
            .registry
            .navigation_features()
            .filter(|metadata| metadata.parent().is_none())
            .fold(
                Vec::<(&'static str, Vec<FeatureMetadata>)>::new(),
                |mut sections, metadata| {
                    let section = metadata.section().unwrap_or("应用");
                    if let Some((_, items)) = sections
                        .iter_mut()
                        .find(|(existing, _)| *existing == section)
                    {
                        items.push(metadata);
                    } else {
                        sections.push((section, vec![metadata]));
                    }
                    sections
                },
            );
        let groups = sections.into_iter().map(|(section, items)| {
            SidebarGroup::new(section).child(
                SidebarMenu::new().children(
                    items
                        .into_iter()
                        .map(|metadata| self.render_navigation_item(metadata, cx)),
                ),
            )
        });

        let sidebar = Sidebar::new("nexora-sidebar")
            .size_full()
            .collapsible(SidebarCollapsible::None)
            .children(groups);
        let sidebar = match self.sidebar_header.as_ref() {
            Some(header) => sidebar.header(SidebarHeaderContainer::new().child(header.clone())),
            None => sidebar,
        };
        let sidebar = match self.sidebar_footer.as_ref() {
            Some(footer) => sidebar.footer(SidebarFooterContainer::new().child(footer.clone())),
            None => sidebar,
        };
        sidebar.into_any_element()
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let active_index = self
            .opened_tabs
            .iter()
            .position(|route| route.concrete_path() == self.active_path)
            .unwrap_or_default();

        TabBar::new("nexora-feature-tabs")
            .selected_index(active_index)
            .on_click(cx.listener(|this, index: &usize, window, cx| {
                let Some(route) = this.opened_tabs.get(*index).cloned() else {
                    return;
                };
                if let Err(error) = this.open_route(route, window, cx) {
                    this.navigation_error = Some(error.to_string());
                    cx.notify();
                }
            }))
            .children(
                self.opened_tabs
                    .iter()
                    .map(|route| Tab::new().label(route.target().title())),
            )
            .into_any_element()
    }

    fn render_active_feature(&self) -> AnyElement {
        self.feature_instances
            .get(self.active_path.as_str())
            .map(|instance| instance.view().into_any_element())
            .unwrap_or_else(|| div().into_any_element())
    }
}

fn feature_icon(icon: &str) -> Option<IconName> {
    match icon {
        "asterisk" => Some(IconName::Asterisk),
        "folder-open" => Some(IconName::FolderOpen),
        "frame" => Some(IconName::Frame),
        "layout-dashboard" => Some(IconName::LayoutDashboard),
        "settings" => Some(IconName::Settings),
        "square-terminal" => Some(IconName::SquareTerminal),
        "user" => Some(IconName::User),
        _ => None,
    }
}

impl Render for ApplicationShell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let border = cx.theme().border;
        let navigation_error = self.navigation_error.clone();

        h_flex()
            .size_full()
            .items_stretch()
            .overflow_hidden()
            .child(
                div()
                    .w(px(224.0))
                    .h_full()
                    .flex_shrink_0()
                    .border_r_1()
                    .border_color(border)
                    .child(self.render_sidebar(cx)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .h(px(40.0))
                            .flex_shrink_0()
                            .border_b_1()
                            .border_color(border)
                            .child(self.render_tabs(cx)),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.render_active_feature())
                            .when_some(navigation_error, |element, message| {
                                element.child(
                                    div()
                                        .absolute()
                                        .left_3()
                                        .right_3()
                                        .bottom_3()
                                        .child(Alert::error("nexora-navigation-error", message)),
                                )
                            }),
                    ),
            )
    }
}
