//! Nexora 桌面应用启动契约与通用 Feature Shell。
//!
//! 应用实现方只负责提供启动选项和初始化自己的全局状态；注册表发现、首路由校验、
//! 主窗口创建以及 Feature Entity 的生命周期由框架统一管理。

use std::{
    collections::HashMap,
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
};

use ::desktop::{
    Application as DesktopApplication, ApplicationOptions as DesktopApplicationOptions,
};
#[cfg(feature = "desktop")]
use actions::account::{self as account_actions, AccountActionKind, SignInAccount, SignOutAccount};
use actions::{settings::OpenSettings, window as window_actions};
use configuration::UserConfigStore;
#[cfg(feature = "desktop")]
use gpui::{Anchor, WindowHandle};
use gpui::{
    AnyElement, AnyView, App, Context, Global, Image, ImageFormat, IntoElement as _, MouseButton,
    Pixels, Render, ScrollHandle, Size, Subscription, WeakEntity, Window, WindowOptions, div, img,
    prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar,
    alert::Alert,
    breadcrumb::{Breadcrumb, BreadcrumbItem},
    button::{Button, ButtonVariants as _, Toggle},
    h_flex,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarGroup, SidebarHeader as SidebarHeaderContainer,
        SidebarMenu, SidebarMenuItem,
    },
    tab::{Tab, TabBar},
};
#[cfg(feature = "desktop")]
use gpui_component::{
    avatar::Avatar, menu::DropdownMenu as _, sidebar::SidebarFooter as SidebarFooterContainer,
};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ui::{PanelHeader, layout::WorkspaceLayout};

/// 应用默认品牌区域使用的 PNG Logo。
///
/// Logo 字节应通过 `include_bytes!` 编译进最终桌面程序，避免运行时依赖当前工作目录。
/// 完全自定义登录页时仍可使用 `LoginFeature` 覆盖默认实现。
#[derive(Clone, Copy, Debug)]
pub struct ApplicationLogo {
    bytes: &'static [u8],
}

impl ApplicationLogo {
    /// 从编译期 PNG 字节创建应用 Logo。
    pub const fn png(bytes: &'static [u8]) -> Self {
        Self { bytes }
    }

    pub(crate) fn image(self) -> std::sync::Arc<Image> {
        std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, self.bytes.to_vec()))
    }
}

#[derive(Clone)]
pub(crate) struct ApplicationBranding {
    pub(crate) application_name: String,
    pub(crate) application_version: Option<String>,
    pub(crate) logo: Option<ApplicationLogo>,
}

impl Global for ApplicationBranding {}

pub(crate) fn application_branding(cx: &App) -> ApplicationBranding {
    cx.try_global::<ApplicationBranding>()
        .cloned()
        .unwrap_or_else(|| ApplicationBranding {
            application_name: "Nexora".to_owned(),
            application_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            logo: None,
        })
}

use crate::{
    AppRegistry, FeatureInstance, FeatureMetadata, FeatureRuntimeError, NavigationContextExt as _,
    RegistryError, ResolveError, RouteMatch, RouteTargetKind, WindowRuntimeError,
    runtime::{clear_navigation_handler, install_navigation_handler},
};

/// Nexora 桌面应用的启动选项。
///
/// 默认值会创建一个 `900 × 640` 的主窗口、限制最小尺寸为 `640 × 480`、主动激活应用，
/// 并以中文和根路径 `/` 启动。应用只需要覆盖与自身产品有关的字段。
#[derive(Debug)]
pub struct ApplicationOptions {
    /// 应用在系统菜单和默认 Sidebar Header 中展示的名称。
    pub application_name: String,
    /// 默认登录页左下角展示的应用版本号。
    pub application_version: Option<String>,
    /// 默认登录页和 Sidebar Header 共享的可选 PNG Logo。
    pub application_logo: Option<ApplicationLogo>,
    /// 默认 Sidebar Header 中位于应用名称下方的说明文字。
    ///
    /// 应用注册自定义 `SidebarHeader` 时不会显示该文字。
    pub sidebar_subtitle: Option<String>,
    /// 是否在最后一个窗口关闭后继续保持应用进程运行。
    pub daemon_mode: bool,
    /// 创建主窗口后是否主动激活应用。
    pub activate: bool,
    /// 需要直接传递给 GPUI 的原生窗口选项。
    ///
    /// 为 `None` 时从 GPUI 默认值构造；没有配置 titlebar 时，框架会补上与
    /// [`WorkspaceLayout`] 匹配的 `gpui-component` TitleBar 选项。
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
            application_name: "Nexora".to_owned(),
            application_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            application_logo: None,
            sidebar_subtitle: Some("Desktop workspace".to_owned()),
            daemon_mode: false,
            activate: true,
            window_options: Some(WindowOptions {
                titlebar: Some(TitleBar::title_bar_options()),
                ..Default::default()
            }),
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

    /// 设置应用在系统菜单和默认 Sidebar Header 中展示的名称。
    pub fn application_name(mut self, application_name: impl Into<String>) -> Self {
        self.application_name = application_name.into();
        self
    }

    /// 设置默认登录页左下角展示的应用版本号。
    pub fn application_version(mut self, application_version: impl Into<String>) -> Self {
        self.application_version = Some(application_version.into());
        self
    }

    /// 设置默认登录页和 Sidebar Header 使用的 PNG Logo。
    pub const fn application_logo(mut self, application_logo: ApplicationLogo) -> Self {
        self.application_logo = Some(application_logo);
        self
    }

    /// 设置默认 Sidebar Header 中位于应用名称下方的说明文字。
    pub fn sidebar_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.sidebar_subtitle = Some(subtitle.into());
        self
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
        let mut window_options = self.window_options.unwrap_or_default();
        if window_options.titlebar.is_none() {
            window_options.titlebar = Some(TitleBar::title_bar_options());
        }
        DesktopApplicationOptions {
            daemon_mode: self.daemon_mode,
            activate: self.activate,
            window_options: Some(window_options),
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
    account_registry: AppRegistry,
    account_initial_route: RouteMatch,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct ShellPreferences {
    pinned_tabs: Vec<String>,
}

struct PreferencesWriter {
    sender: Sender<PreferencesWriteCommand>,
    worker: Option<JoinHandle<()>>,
}

enum PreferencesWriteCommand {
    Persist(ShellPreferences),
    Shutdown,
}

impl PreferencesWriter {
    fn start(store: UserConfigStore<ShellPreferences>) -> Result<Self, std::io::Error> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("nexora-shell-preferences".to_owned())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    let PreferencesWriteCommand::Persist(mut preferences) = command else {
                        break;
                    };
                    let mut shutdown = false;
                    while let Ok(command) = receiver.try_recv() {
                        match command {
                            PreferencesWriteCommand::Persist(latest) => preferences = latest,
                            PreferencesWriteCommand::Shutdown => {
                                shutdown = true;
                                break;
                            }
                        }
                    }

                    _ = store.save(&preferences);
                    if shutdown {
                        break;
                    }
                }
            })?;

        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }

    fn persist(&self, preferences: ShellPreferences) {
        _ = self
            .sender
            .send(PreferencesWriteCommand::Persist(preferences));
    }
}

impl Drop for PreferencesWriter {
    fn drop(&mut self) {
        _ = self.sender.send(PreferencesWriteCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            _ = worker.join();
        }
    }
}

fn prepare_application(
    options: &ApplicationOptions,
) -> Result<PreparedApplication, ApplicationError> {
    let registry = AppRegistry::discover_for_application(false)?;
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

    let account_registry = AppRegistry::discover_for_application(true)?;
    let account_initial_route = account_registry
        .resolve(options.initial_path.as_str())
        .map_err(|source| ApplicationError::InitialRoute {
            path: options.initial_path.clone(),
            source,
        })?;

    Ok(PreparedApplication {
        registry,
        initial_route,
        account_registry,
        account_initial_route,
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
        account_registry,
        account_initial_route,
    } = prepare_application(&options)?;
    let locale = options.locale.clone();
    let application_name = options.application_name.clone();
    let application_version = options.application_version.clone();
    let application_logo = options.application_logo;
    let sidebar_subtitle = options.sidebar_subtitle.clone();
    let preferences_store = UserConfigStore::for_local_application(
        "com",
        "Nexora",
        application_name.as_str(),
        "workspace.toml",
    )
    .ok();
    let pinned_tab_paths = preferences_store
        .as_ref()
        .and_then(|store| store.load_or_default().ok())
        .map(|preferences: ShellPreferences| preferences.pinned_tabs)
        .unwrap_or_default();
    let adapter = ApplicationAdapter {
        application,
        options: options.into_desktop_options(),
        locale,
        application_name,
        application_version,
        application_logo,
        account_enabled: false,
        sidebar_subtitle,
        preferences_store,
        pinned_tab_paths,
        registry: Some(registry),
        initial_route: Some(initial_route),
        account_registry: Some(account_registry),
        account_initial_route: Some(account_initial_route),
    };

    DesktopApplication::run(adapter);
    Ok(())
}

struct ApplicationAdapter<A> {
    application: A,
    options: DesktopApplicationOptions,
    locale: String,
    application_name: String,
    application_version: Option<String>,
    application_logo: Option<ApplicationLogo>,
    account_enabled: bool,
    sidebar_subtitle: Option<String>,
    preferences_store: Option<UserConfigStore<ShellPreferences>>,
    pinned_tab_paths: Vec<String>,
    registry: Option<AppRegistry>,
    initial_route: Option<RouteMatch>,
    account_registry: Option<AppRegistry>,
    account_initial_route: Option<RouteMatch>,
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
        cx.set_global(ApplicationBranding {
            application_name: self.application_name.clone(),
            application_version: self.application_version.clone(),
            logo: self.application_logo,
        });
        actions::init();
        actions::settings::bind_keys(cx);
        cx.on_action(|_: &OpenSettings, cx| {
            _ = cx.navigate("/settings");
        });
        window_actions::init(self.application_name.clone(), cx);
        self.application.initialize(cx);
        self.account_enabled = crate::account::client::login_snapshot(cx).configured;
        if self.account_enabled {
            account_actions::bind_keys(cx);
            cx.on_action(|_: &SignInAccount, cx| {
                let snapshot = crate::account::client::login_snapshot(cx);
                if !snapshot.authenticated && !snapshot.busy {
                    _ = crate::account::client::start_login(cx);
                }
            });
            cx.on_action(|_: &SignOutAccount, cx| {
                crate::account::client::sign_out(cx);
            });
        }
    }

    fn build_root_view(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Entity<Self::RootView> {
        let (registry, initial_route) = if self.account_enabled {
            (
                self.account_registry
                    .take()
                    .expect("Nexora Account 主窗口注册表只能被消费一次"),
                self.account_initial_route
                    .take()
                    .expect("Nexora Account 主窗口首路由只能被消费一次"),
            )
        } else {
            (
                self.registry
                    .take()
                    .expect("Nexora 主窗口注册表只能被消费一次"),
                self.initial_route
                    .take()
                    .expect("Nexora 主窗口首路由只能被消费一次"),
            )
        };

        let application_name = self.application_name.clone();
        let application_logo = self.application_logo;
        let account_enabled = self.account_enabled;
        let sidebar_subtitle = self.sidebar_subtitle.clone();
        let preferences_store = self.preferences_store.clone();
        let pinned_tab_paths = std::mem::take(&mut self.pinned_tab_paths);
        cx.new(|cx| {
            ApplicationShell::new(
                registry,
                initial_route,
                ApplicationShellConfig {
                    application_name,
                    application_logo,
                    account_enabled,
                    sidebar_subtitle,
                    preferences_store,
                    pinned_tab_paths,
                },
                window,
                cx,
            )
        })
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
    #[cfg(feature = "desktop")]
    #[error("未登录时不能打开独立窗口 `{path}`")]
    AuthenticationRequired { path: String },
}

#[derive(Debug, Clone)]
struct ShellRoute {
    route: RouteMatch,
}

impl ShellRoute {
    fn new(route: RouteMatch) -> Self {
        debug_assert_eq!(route.target().kind(), RouteTargetKind::Feature);
        Self { route }
    }

    fn path(&self) -> &str {
        self.route.concrete_path()
    }

    fn title(&self) -> String {
        let title = self.route.target().title();
        let parameters = self
            .route
            .target()
            .path()
            .split('/')
            .zip(self.path().split('/'))
            .filter_map(|(pattern, value)| pattern.starts_with(':').then_some(value))
            .map(|value| percent_decode_str(value).decode_utf8_lossy().into_owned())
            .collect::<Vec<_>>();
        if !parameters.is_empty() {
            return format!("{title} · {}", parameters.join(" · "));
        }

        title.to_owned()
    }

    fn icon(&self) -> Option<&'static str> {
        self.route.target().icon()
    }

    const fn route(&self) -> &RouteMatch {
        &self.route
    }
}

impl PartialEq for ShellRoute {
    fn eq(&self, other: &Self) -> bool {
        self.path() == other.path()
    }
}

impl Eq for ShellRoute {}

struct ApplicationShellConfig {
    application_name: String,
    application_logo: Option<ApplicationLogo>,
    account_enabled: bool,
    sidebar_subtitle: Option<String>,
    preferences_store: Option<UserConfigStore<ShellPreferences>>,
    pinned_tab_paths: Vec<String>,
}

struct ApplicationShell {
    registry: AppRegistry,
    application_name: String,
    application_logo: Option<ApplicationLogo>,
    account_enabled: bool,
    sidebar_subtitle: Option<String>,
    initial_route: ShellRoute,
    active_route: ShellRoute,
    opened_tabs: Vec<ShellRoute>,
    pinned_tabs: Vec<ShellRoute>,
    tab_context_route: Option<ShellRoute>,
    pinned_tab_scroll_handle: ScrollHandle,
    regular_tab_scroll_handle: ScrollHandle,
    navigation_history: Vec<ShellRoute>,
    navigation_history_index: usize,
    preferences_writer: Option<PreferencesWriter>,
    feature_instances: HashMap<String, FeatureInstance>,
    #[cfg(feature = "desktop")]
    login_feature: AnyView,
    #[cfg(feature = "desktop")]
    authenticated: bool,
    #[cfg(feature = "desktop")]
    auth_identity: Option<String>,
    #[cfg(feature = "desktop")]
    business_windows: Vec<WindowHandle<gpui_component::Root>>,
    sidebar_header: Option<AnyView>,
    sidebar_footer: Option<AnyView>,
    navigation_error: Option<String>,
    #[cfg(feature = "desktop")]
    _authentication_subscription: Option<Subscription>,
    _release_subscription: Option<Subscription>,
}

impl ApplicationShell {
    fn new(
        registry: AppRegistry,
        initial_route: RouteMatch,
        config: ApplicationShellConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let ApplicationShellConfig {
            application_name,
            application_logo,
            account_enabled,
            sidebar_subtitle,
            preferences_store,
            pinned_tab_paths,
        } = config;
        let shell = cx.entity().downgrade();
        install_navigation_handler(
            move |location, cx| {
                _ = shell.update_in(cx, move |this, window, cx| {
                    this.handle_navigation_request(location, window, cx);
                });
            },
            cx,
        );
        let initial_route = ShellRoute::new(initial_route);
        let mut pinned_tabs = pinned_tab_paths
            .into_iter()
            .filter_map(|path| registry.resolve(path.as_str()).ok())
            .filter(|route| route.target().kind() == RouteTargetKind::Feature)
            .map(ShellRoute::new)
            .fold(Vec::new(), |mut routes, route| {
                if !routes.contains(&route) {
                    routes.push(route);
                }
                routes
            });
        if let Some(index) = pinned_tabs.iter().position(|route| route == &initial_route) {
            pinned_tabs[index] = initial_route.clone();
        }
        let mut opened_tabs = pinned_tabs.clone();
        if !opened_tabs.contains(&initial_route) {
            opened_tabs.push(initial_route.clone());
        }
        #[cfg(feature = "desktop")]
        let login_feature = registry.create_login_feature(window, cx);
        #[cfg(feature = "desktop")]
        let authenticated = !account_enabled || crate::account::client::is_authenticated(cx);
        #[cfg(feature = "desktop")]
        let auth_identity = account_enabled
            .then(|| crate::account::client::login_profile(cx))
            .flatten()
            .map(|profile| profile.user.identity_id.clone());
        #[cfg(feature = "desktop")]
        let (sidebar_header, sidebar_footer) = if authenticated {
            (
                registry.create_sidebar_header(window, cx),
                registry.create_sidebar_footer(window, cx),
            )
        } else {
            (None, None)
        };
        #[cfg(not(feature = "desktop"))]
        let sidebar_header = registry.create_sidebar_header(window, cx);
        #[cfg(not(feature = "desktop"))]
        let sidebar_footer = registry.create_sidebar_footer(window, cx);
        #[cfg(feature = "desktop")]
        let (feature_instances, navigation_error) = if authenticated {
            create_initial_feature(&registry, initial_route.route().clone(), window, cx)
        } else {
            (HashMap::new(), None)
        };
        #[cfg(not(feature = "desktop"))]
        let (feature_instances, navigation_error) =
            create_initial_feature(&registry, initial_route.route().clone(), window, cx);
        #[cfg(feature = "desktop")]
        let _authentication_subscription = account_enabled.then(|| {
            crate::account::client::observe_authentication_in(window, cx, |this, window, cx| {
                this.authentication_changed(window, cx);
            })
        });
        let _release_subscription = Some(cx.on_release_in(window, |this, window, cx| {
            clear_navigation_handler(cx);
            for (_, mut instance) in this.feature_instances.drain() {
                instance.close(window, cx);
            }
            #[cfg(feature = "desktop")]
            this.close_business_windows(cx);
            this.preferences_writer = None;
        }));
        let preferences_writer =
            preferences_store.and_then(|store| PreferencesWriter::start(store).ok());

        Self {
            registry,
            application_name,
            application_logo,
            account_enabled,
            sidebar_subtitle,
            initial_route: initial_route.clone(),
            active_route: initial_route.clone(),
            opened_tabs,
            pinned_tabs,
            tab_context_route: None,
            pinned_tab_scroll_handle: ScrollHandle::new(),
            regular_tab_scroll_handle: ScrollHandle::new(),
            navigation_history: vec![initial_route],
            navigation_history_index: 0,
            preferences_writer,
            feature_instances,
            #[cfg(feature = "desktop")]
            login_feature,
            #[cfg(feature = "desktop")]
            authenticated,
            #[cfg(feature = "desktop")]
            auth_identity,
            #[cfg(feature = "desktop")]
            business_windows: Vec::new(),
            sidebar_header,
            sidebar_footer,
            navigation_error,
            #[cfg(feature = "desktop")]
            _authentication_subscription,
            _release_subscription,
        }
    }

    #[cfg(feature = "desktop")]
    fn authentication_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.account_enabled {
            return;
        }
        let authenticated = crate::account::client::is_authenticated(cx);
        let auth_identity = crate::account::client::login_profile(cx)
            .map(|profile| profile.user.identity_id.clone());
        if authenticated == self.authenticated && auth_identity == self.auth_identity {
            cx.notify();
            return;
        }

        let identity_changed =
            authenticated && self.authenticated && auth_identity != self.auth_identity;
        self.authenticated = authenticated;
        self.auth_identity = auth_identity;
        if authenticated {
            if identity_changed {
                for (_, mut instance) in self.feature_instances.drain() {
                    instance.close(window, cx);
                }
                self.close_business_windows(cx);
            }
            self.sidebar_header = self.registry.create_sidebar_header(window, cx);
            self.sidebar_footer = self.registry.create_sidebar_footer(window, cx);
            self.activate_selected_feature(window, cx);
        } else {
            for (_, mut instance) in self.feature_instances.drain() {
                instance.close(window, cx);
            }
            self.close_business_windows(cx);
            self.sidebar_header = None;
            self.sidebar_footer = None;
            self.navigation_error = None;
        }
        cx.notify();
    }

    #[cfg(feature = "desktop")]
    fn close_business_windows(&mut self, cx: &mut App) {
        for handle in self.business_windows.drain(..) {
            _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    #[cfg(feature = "desktop")]
    fn activate_selected_feature(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active_route = self.active_route.clone();
        match self.ensure_feature_instance(&active_route, window, cx) {
            Ok(()) => {
                self.feature_instances
                    .get_mut(active_route.path())
                    .expect("当前 Feature 应当已进入运行时缓存")
                    .activate(window, cx);
                self.navigation_error = None;
            }
            Err(error) => self.navigation_error = Some(error.to_string()),
        }
    }

    fn active_path(&self) -> &str {
        self.active_route.path()
    }

    fn ensure_feature_instance(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if let Some(instance) = self.feature_instances.get_mut(route.path()) {
            if instance.route() != route.route() {
                instance.update_route(route.route().clone(), window, cx)?;
            }
            return Ok(());
        }

        let instance = self
            .registry
            .create_feature(route.route().clone(), window, cx)?;
        self.feature_instances
            .insert(route.path().to_owned(), instance);
        Ok(())
    }

    fn close_feature_instance(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut instance) = self.feature_instances.remove(path) else {
            return;
        };
        instance.close(window, cx);
    }

    fn close_orphaned_feature_instances(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let removed_paths = self
            .feature_instances
            .keys()
            .filter(|path| !self.opened_tabs.iter().any(|route| route.path() == *path))
            .cloned()
            .collect::<Vec<_>>();

        for path in removed_paths {
            self.close_feature_instance(path.as_str(), window, cx);
        }
    }

    fn synchronize_feature_runtime(
        &mut self,
        previous_active_path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        let active_route = self.active_route.clone();
        self.ensure_feature_instance(&active_route, window, cx)?;
        self.close_orphaned_feature_instances(window, cx);

        if previous_active_path != active_route.path()
            && let Some(previous) = self.feature_instances.get_mut(previous_active_path)
        {
            previous.deactivate(window, cx);
        }
        self.feature_instances
            .get_mut(active_route.path())
            .expect("当前 Feature 应当已进入运行时缓存")
            .activate(window, cx);
        Ok(())
    }

    fn navigate_to_route_in(
        &mut self,
        route: ShellRoute,
        record_history: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        self.ensure_feature_instance(&route, window, cx)?;
        let previous_active_path = self.active_path().to_owned();
        self.navigate_to_route(route, record_history);
        self.synchronize_feature_runtime(previous_active_path.as_str(), window, cx)
    }

    fn navigate_to_route(&mut self, route: ShellRoute, record_history: bool) {
        let same_instance = self.active_route == route;
        self.open_feature_tab(route.clone());

        if same_instance {
            self.active_route = route.clone();
            if let Some(current) = self
                .navigation_history
                .get_mut(self.navigation_history_index)
                && *current == route
            {
                *current = route.clone();
            }
            self.scroll_tab_into_view(&route);
            return;
        }

        self.active_route = route.clone();
        if record_history {
            self.push_navigation_history(route.clone());
        }
        self.scroll_tab_into_view(&route);
    }

    fn push_navigation_history(&mut self, route: ShellRoute) {
        if self.navigation_history.get(self.navigation_history_index) == Some(&route) {
            return;
        }

        self.navigation_history
            .truncate(self.navigation_history_index + 1);
        self.navigation_history.push(route);
        self.navigation_history_index = self.navigation_history.len().saturating_sub(1);
    }

    fn can_navigate_back(&self) -> bool {
        self.navigation_history_index > 0
    }

    fn can_navigate_forward(&self) -> bool {
        self.navigation_history_index + 1 < self.navigation_history.len()
    }

    fn navigate_back_in(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if !self.can_navigate_back() {
            return Ok(());
        }

        let target_index = self.navigation_history_index - 1;
        let route = self.navigation_history[target_index].clone();
        self.ensure_feature_instance(&route, window, cx)?;
        self.navigation_history_index = target_index;
        self.navigate_to_route_in(route, false, window, cx)
    }

    fn navigate_forward_in(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if !self.can_navigate_forward() {
            return Ok(());
        }

        let target_index = self.navigation_history_index + 1;
        let route = self.navigation_history[target_index].clone();
        self.ensure_feature_instance(&route, window, cx)?;
        self.navigation_history_index = target_index;
        self.navigate_to_route_in(route, false, window, cx)
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
            #[cfg(feature = "desktop")]
            if self.account_enabled && !self.authenticated && route.target().id() != "settings" {
                return Err(NavigationError::AuthenticationRequired {
                    path: route.concrete_path().to_owned(),
                });
            }
            #[cfg(feature = "desktop")]
            {
                let is_settings = route.target().id() == "settings";
                let handle = self.registry.open_window(route, cx)?;
                if !is_settings {
                    self.business_windows
                        .retain(|existing| existing.read(cx).is_ok());
                    self.business_windows.push(handle);
                }
            }
            #[cfg(not(feature = "desktop"))]
            self.registry.open_window(route, cx)?;
            self.navigation_error = None;
            cx.notify();
            return Ok(());
        }

        let route = ShellRoute::new(route);
        #[cfg(feature = "desktop")]
        if self.account_enabled && !self.authenticated {
            self.navigate_to_route(route, true);
            self.navigation_error = None;
            cx.notify();
            return Ok(());
        }

        self.navigate_to_route_in(route, true, window, cx)?;
        self.navigation_error = None;
        cx.notify();
        Ok(())
    }

    fn open_feature_tab(&mut self, route: ShellRoute) {
        if let Some(index) = self.tab_index(&route) {
            self.opened_tabs[index] = route.clone();
            if let Some(index) = self.pinned_tabs.iter().position(|pinned| pinned == &route) {
                self.pinned_tabs[index] = route;
            }
            return;
        }

        self.opened_tabs.push(route);
        self.reorder_tabs_by_pin();
    }

    fn regular_tab_routes(&self) -> Vec<ShellRoute> {
        self.opened_tabs
            .iter()
            .filter(|route| !self.is_route_pinned(route))
            .cloned()
            .collect()
    }

    fn tab_index(&self, route: &ShellRoute) -> Option<usize> {
        self.opened_tabs.iter().position(|opened| opened == route)
    }

    fn pinned_tab_index(&self, route: &ShellRoute) -> Option<usize> {
        self.pinned_tabs.iter().position(|pinned| pinned == route)
    }

    fn regular_tab_index(&self, route: &ShellRoute) -> Option<usize> {
        self.opened_tabs
            .iter()
            .filter(|opened| !self.is_route_pinned(opened))
            .position(|opened| opened == route)
    }

    fn active_pinned_tab_index(&self) -> Option<usize> {
        self.pinned_tab_index(&self.active_route)
    }

    fn active_regular_tab_index(&self) -> Option<usize> {
        self.regular_tab_index(&self.active_route)
    }

    fn scroll_tab_into_view(&self, route: &ShellRoute) {
        if let Some(index) = self.pinned_tab_index(route) {
            self.pinned_tab_scroll_handle.scroll_to_item(index);
        } else if let Some(index) = self.regular_tab_index(route) {
            self.regular_tab_scroll_handle.scroll_to_item(index);
        }
    }

    fn is_route_pinned(&self, route: &ShellRoute) -> bool {
        self.pinned_tabs.contains(route)
    }

    fn toggle_pin_route(&mut self, route: &ShellRoute) {
        if self.is_route_pinned(route) {
            self.pinned_tabs.retain(|pinned| pinned != route);
        } else {
            self.pinned_tabs.push(route.clone());
        }

        self.reorder_tabs_by_pin();
        self.scroll_tab_into_view(&self.active_route);
        self.persist_pinned_tabs();
    }

    fn persist_pinned_tabs(&self) {
        let Some(writer) = self.preferences_writer.as_ref() else {
            return;
        };
        let preferences = ShellPreferences {
            pinned_tabs: self
                .pinned_tabs
                .iter()
                .map(|route| route.path().to_owned())
                .collect(),
        };
        writer.persist(preferences);
    }

    fn reorder_tabs_by_pin(&mut self) {
        // 置顶列表只决定路径顺序；具体 route 始终取 opened_tabs 中的最新 query 快照。
        let mut pinned = self
            .pinned_tabs
            .iter()
            .filter_map(|pinned| {
                self.opened_tabs
                    .iter()
                    .find(|opened| *opened == pinned)
                    .cloned()
            })
            .fold(Vec::new(), |mut routes, route| {
                if !routes.contains(&route) {
                    routes.push(route);
                }
                routes
            });
        let mut regular = self
            .opened_tabs
            .iter()
            .filter(|route| !pinned.contains(route))
            .cloned()
            .collect::<Vec<_>>();

        self.pinned_tabs = pinned.clone();
        pinned.append(&mut regular);
        self.opened_tabs = pinned;
    }

    fn ensure_active_tab(&mut self) {
        if self.opened_tabs.is_empty() {
            self.opened_tabs.push(self.initial_route.clone());
        }
        self.active_route = self
            .opened_tabs
            .iter()
            .find(|opened| *opened == &self.active_route)
            .cloned()
            .unwrap_or_else(|| self.opened_tabs[0].clone());

        self.pinned_tabs
            .retain(|route| self.opened_tabs.contains(route));
        self.scroll_tab_into_view(&self.active_route);
    }

    fn ensure_active_or_select(&mut self, fallback: ShellRoute) {
        if !self.opened_tabs.contains(&self.active_route) {
            self.active_route = fallback;
        }
        self.ensure_active_tab();
    }

    fn close_tab_route(&mut self, route: &ShellRoute) {
        let Some(index) = self.tab_index(route) else {
            return;
        };
        let closing_active = &self.active_route == route;
        self.opened_tabs.remove(index);
        self.pinned_tabs.retain(|pinned| pinned != route);

        if self.opened_tabs.is_empty() {
            self.opened_tabs.push(self.initial_route.clone());
        }
        if closing_active {
            let fallback_index = index.min(self.opened_tabs.len().saturating_sub(1));
            if let Some(route) = self.opened_tabs.get(fallback_index).cloned() {
                self.active_route = route;
            }
        }
        self.ensure_active_tab();
    }

    fn close_tabs_to_left(&mut self, route: &ShellRoute) {
        let Some(index) = self.tab_index(route) else {
            return;
        };
        self.opened_tabs = self
            .opened_tabs
            .iter()
            .enumerate()
            .filter_map(|(tab_index, opened)| {
                (tab_index >= index || opened == route || self.is_route_pinned(opened))
                    .then_some(opened.clone())
            })
            .collect();
        self.ensure_active_or_select(route.clone());
    }

    fn close_tabs_to_right(&mut self, route: &ShellRoute) {
        let Some(index) = self.tab_index(route) else {
            return;
        };
        self.opened_tabs = self
            .opened_tabs
            .iter()
            .enumerate()
            .filter_map(|(tab_index, opened)| {
                (tab_index <= index || opened == route || self.is_route_pinned(opened))
                    .then_some(opened.clone())
            })
            .collect();
        self.ensure_active_or_select(route.clone());
    }

    fn close_other_tabs(&mut self, route: &ShellRoute) {
        self.opened_tabs = self
            .opened_tabs
            .iter()
            .filter(|opened| *opened == route || self.is_route_pinned(opened))
            .cloned()
            .collect();
        self.ensure_active_or_select(route.clone());
        self.reorder_tabs_by_pin();
    }

    fn update_runtime_after_tab_change(
        &mut self,
        previous_active_route: ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if previous_active_route.path() != self.active_path() {
            self.push_navigation_history(self.active_route.clone());
        } else if previous_active_route.route() != self.active_route.route() {
            if let Some(current) = self
                .navigation_history
                .get_mut(self.navigation_history_index)
                && current == &self.active_route
            {
                *current = self.active_route.clone();
            } else {
                self.push_navigation_history(self.active_route.clone());
            }
        }
        match self.synchronize_feature_runtime(previous_active_route.path(), window, cx) {
            Ok(()) => self.navigation_error = None,
            Err(error) => self.navigation_error = Some(error.to_string()),
        }
        cx.notify();
    }

    fn close_tab_route_in(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_active_route = self.active_route.clone();
        self.close_tab_route(route);
        self.persist_pinned_tabs();
        self.update_runtime_after_tab_change(previous_active_route, window, cx);
    }

    fn close_tabs_to_left_in(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_active_route = self.active_route.clone();
        self.close_tabs_to_left(route);
        self.update_runtime_after_tab_change(previous_active_route, window, cx);
    }

    fn close_tabs_to_right_in(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_active_route = self.active_route.clone();
        self.close_tabs_to_right(route);
        self.update_runtime_after_tab_change(previous_active_route, window, cx);
    }

    fn close_other_tabs_in(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_active_route = self.active_route.clone();
        self.close_other_tabs(route);
        self.update_runtime_after_tab_change(previous_active_route, window, cx);
    }

    fn select_pinned_tab_in(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if let Some(route) = self.pinned_tabs.get(index).cloned() {
            self.navigate_to_route_in(route, true, window, cx)?;
        }
        Ok(())
    }

    fn select_regular_tab_in(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if let Some(route) = self.regular_tab_routes().get(index).cloned() {
            self.navigate_to_route_in(route, true, window, cx)?;
        }
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
        let item = SidebarMenuItem::new(metadata.title())
            .icon(feature_icon(metadata.icon()))
            .active(self.active_target_id() == metadata.id())
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Err(error) = this.open_path(path, window, cx) {
                    this.navigation_error = Some(error.to_string());
                }
                cx.notify();
            }));

        if children.is_empty() {
            item
        } else {
            item.default_open(self.navigation_branch_is_active(metadata.id()))
                .click_to_toggle(true)
                .children(children)
        }
    }

    fn active_target_id(&self) -> &'static str {
        self.active_route.route().target().id()
    }

    fn navigation_branch_is_active(&self, branch_id: &str) -> bool {
        let mut active_id = Some(self.active_target_id());
        while let Some(id) = active_id {
            if id == branch_id {
                return true;
            }
            active_id = self
                .registry
                .features()
                .iter()
                .find(|metadata| metadata.id() == id)
                .and_then(|metadata| metadata.parent());
        }
        false
    }

    fn navigation_sections(&self) -> Vec<(&'static str, Vec<FeatureMetadata>)> {
        self.registry
            .navigation_features()
            .filter(|metadata| metadata.parent().is_none())
            .fold(Vec::new(), |mut sections, metadata| {
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
            })
    }

    fn render_default_sidebar_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .child(
                img(self
                    .application_logo
                    .map(ApplicationLogo::image)
                    .unwrap_or_else(ui::default_application_logo))
                .size_7()
                .flex_shrink_0(),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(theme.sidebar_accent_foreground)
                            .child(self.application_name.clone()),
                    )
                    .when_some(self.sidebar_subtitle.clone(), |this, subtitle| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.sidebar_foreground.opacity(0.66))
                                .child(subtitle),
                        )
                    }),
            )
            .into_any_element()
    }

    #[cfg(feature = "desktop")]
    fn render_default_account_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let profile = crate::account::client::login_profile(cx);
        let display_name = profile
            .map(|profile| profile.user.display_name.clone())
            .unwrap_or_else(|| "当前账户".to_owned());
        let avatar = match profile.and_then(|profile| profile.user.avatar_url.clone()) {
            Some(avatar_url) => Avatar::new()
                .name(display_name.clone())
                .src(avatar_url)
                .small(),
            None => Avatar::new().name(display_name.clone()).small(),
        };
        let menu_items = account_actions::menu_actions();
        let action_context = cx.focus_handle();

        SidebarFooterContainer::new()
            .w_full()
            .child(
                h_flex().flex_1().min_w_0().gap_2().child(avatar).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .child(display_name),
                ),
            )
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                menu_items.iter().cloned().fold(
                    menu.action_context(action_context.clone()).min_w(220.0),
                    |menu, item| {
                        let menu_item =
                            PopupMenuItem::new(item.label()).icon(account_icon(item.kind()));
                        let menu_item = match item.kind() {
                            AccountActionKind::SignIn => menu_item.on_click(|_, _, cx| {
                                _ = crate::account::client::start_login(cx);
                            }),
                            AccountActionKind::SignOut => menu_item.on_click(|_, _, cx| {
                                crate::account::client::sign_out(cx);
                            }),
                            AccountActionKind::Settings => menu_item.action(item.to_action()),
                        };
                        menu.item(menu_item)
                    },
                )
            })
            .into_any_element()
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let sidebar_border = cx.theme().sidebar_border;
        let navigation_sections = self.navigation_sections();
        let navigation_section_count = navigation_sections.len();
        let navigation_groups = navigation_sections
            .into_iter()
            .enumerate()
            .map(|(index, (section, items))| {
                let menu = SidebarMenu::new().children(
                    items
                        .into_iter()
                        .map(|metadata| self.render_navigation_item(metadata, cx)),
                );
                SidebarGroup::new(section).child(if index + 1 < navigation_section_count {
                    menu.pb_3().border_b_1().border_color(sidebar_border)
                } else {
                    menu
                })
            })
            .collect::<Vec<_>>();
        let header = self
            .sidebar_header
            .as_ref()
            .map(|header| header.clone().into_any_element())
            .unwrap_or_else(|| self.render_default_sidebar_header(cx));

        let sidebar = Sidebar::new("nexora-sidebar")
            .size_full()
            .collapsible(SidebarCollapsible::None)
            .header(
                div()
                    .w_full()
                    .pb_3()
                    .border_b_1()
                    .border_color(sidebar_border)
                    .child(SidebarHeaderContainer::new().child(header)),
            )
            .children(navigation_groups);
        let sidebar = if let Some(footer) = self.sidebar_footer.as_ref() {
            sidebar.footer(
                div()
                    .w_full()
                    .pt_3()
                    .border_t_1()
                    .border_color(sidebar_border)
                    .child(SidebarFooterContainer::new().w_full().child(footer.clone())),
            )
        } else {
            #[cfg(feature = "desktop")]
            if self.account_enabled {
                sidebar.footer(
                    div()
                        .w_full()
                        .pt_3()
                        .border_t_1()
                        .border_color(sidebar_border)
                        .child(self.render_default_account_footer(cx)),
                )
            } else {
                sidebar
            }
        };
        sidebar.into_any_element()
    }

    fn render_tab(route: ShellRoute, is_pinned: bool, shell: WeakEntity<Self>) -> Tab {
        let action_shell = shell.clone();
        let context_shell = shell;
        let action_route = route.clone();
        let action = if is_pinned {
            Toggle::new(format!("pin-tab-{}", route.path()))
                .xsmall()
                .checked(true)
                .icon(IconName::StarFill)
                .tooltip("取消置顶")
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    _ = action_shell.update(cx, |this, cx| {
                        this.toggle_pin_route(&action_route);
                        cx.notify();
                    });
                })
                .into_any_element()
        } else {
            Button::new(format!("close-tab-{}", route.path()))
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("关闭标签")
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    _ = action_shell.update(cx, |this, cx| {
                        this.close_tab_route_in(&action_route, window, cx);
                    });
                })
                .into_any_element()
        };

        Tab::new()
            .px_1()
            .prefix(feature_icon(route.icon()))
            .label(route.title())
            .suffix(h_flex().gap_1().child(action))
            .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                _ = context_shell.update(cx, |this, _| {
                    this.tab_context_route = Some(route.clone());
                });
            })
    }

    fn build_tab_context_menu(
        menu: PopupMenu,
        route: ShellRoute,
        shell: WeakEntity<Self>,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let Some(shell_entity) = shell.upgrade() else {
            return menu;
        };
        let Some((pinned, can_close_left, can_close_right, can_close_other)) = ({
            let shell = shell_entity.read(cx);
            let Some(index) = shell.tab_index(&route) else {
                return menu;
            };
            let can_close_left = shell
                .opened_tabs
                .iter()
                .take(index)
                .any(|opened| !shell.is_route_pinned(opened));
            let can_close_right = shell
                .opened_tabs
                .iter()
                .skip(index + 1)
                .any(|opened| !shell.is_route_pinned(opened));
            let can_close_other = shell
                .opened_tabs
                .iter()
                .any(|opened| opened != &route && !shell.is_route_pinned(opened));
            Some((
                shell.is_route_pinned(&route),
                can_close_left,
                can_close_right,
                can_close_other,
            ))
        }) else {
            return menu;
        };

        menu.min_w(220.0)
            .item(PopupMenuItem::new("关闭").icon(IconName::Close).on_click({
                let shell = shell.clone();
                let route = route.clone();
                move |_, window, cx| {
                    _ = shell.update(cx, |this, cx| {
                        this.close_tab_route_in(&route, window, cx);
                    });
                }
            }))
            .separator()
            .item(
                PopupMenuItem::new("关闭左侧标签页")
                    .icon(IconName::ArrowLeft)
                    .disabled(!can_close_left)
                    .on_click({
                        let shell = shell.clone();
                        let route = route.clone();
                        move |_, window, cx| {
                            _ = shell.update(cx, |this, cx| {
                                this.close_tabs_to_left_in(&route, window, cx);
                            });
                        }
                    }),
            )
            .item(
                PopupMenuItem::new("关闭右侧标签页")
                    .icon(IconName::ArrowRight)
                    .disabled(!can_close_right)
                    .on_click({
                        let shell = shell.clone();
                        let route = route.clone();
                        move |_, window, cx| {
                            _ = shell.update(cx, |this, cx| {
                                this.close_tabs_to_right_in(&route, window, cx);
                            });
                        }
                    }),
            )
            .item(
                PopupMenuItem::new("关闭其他标签页")
                    .disabled(!can_close_other)
                    .on_click({
                        let shell = shell.clone();
                        let route = route.clone();
                        move |_, window, cx| {
                            _ = shell.update(cx, |this, cx| {
                                this.close_other_tabs_in(&route, window, cx);
                            });
                        }
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new(if pinned {
                    "取消置顶标签页"
                } else {
                    "置顶标签页"
                })
                .checked(pinned)
                .on_click({
                    move |_, _, cx| {
                        _ = shell.update(cx, |this, cx| {
                            this.toggle_pin_route(&route);
                            cx.notify();
                        });
                    }
                }),
            )
    }

    fn render_title_bar_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let pinned_tabs = self.pinned_tabs.clone();
        let regular_tabs = self.regular_tab_routes();
        let active_pinned_tab_index = self.active_pinned_tab_index();
        let active_regular_tab_index = self.active_regular_tab_index();
        let shell = cx.entity().downgrade();
        let title_bar_background = cx.theme().tokens.title_bar;
        let can_navigate_back = self.can_navigate_back();
        let can_navigate_forward = self.can_navigate_forward();

        h_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .overflow_hidden()
            .gap_2()
            .items_center()
            .child(
                div()
                    .id("nexora-open-tabs-zone")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .id("nexora-open-tabs-strip")
                            .absolute()
                            .left_0()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .h_full()
                            .min_w_0()
                            .overflow_hidden()
                            .items_center()
                            .child(
                                h_flex()
                                    .mx_1()
                                    .flex_shrink_0()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .child(
                                        Button::new("tabs-back")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::ArrowLeft)
                                            .disabled(!can_navigate_back)
                                            .tooltip("后退")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                cx.stop_propagation();
                                                match this.navigate_back_in(window, cx) {
                                                    Ok(()) => this.navigation_error = None,
                                                    Err(error) => {
                                                        this.navigation_error =
                                                            Some(error.to_string())
                                                    }
                                                }
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("tabs-forward")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::ArrowRight)
                                            .disabled(!can_navigate_forward)
                                            .tooltip("前进")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                cx.stop_propagation();
                                                match this.navigate_forward_in(window, cx) {
                                                    Ok(()) => this.navigation_error = None,
                                                    Err(error) => {
                                                        this.navigation_error =
                                                            Some(error.to_string())
                                                    }
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .when(!pinned_tabs.is_empty(), |this| {
                                this.child(
                                    div()
                                        .id("nexora-pinned-tabs-zone")
                                        .flex_none()
                                        .max_w(px(220.0))
                                        .min_w_0()
                                        .h_full()
                                        .overflow_hidden()
                                        .child(
                                            TabBar::new("nexora-pinned-tabs")
                                                .w_full()
                                                .h_full()
                                                .track_scroll(&self.pinned_tab_scroll_handle)
                                                .menu(pinned_tabs.len() > 2)
                                                .when_some(
                                                    active_pinned_tab_index,
                                                    |this, index| this.selected_index(index),
                                                )
                                                .on_click(cx.listener(
                                                    |this, index: &usize, window, cx| {
                                                        match this.select_pinned_tab_in(
                                                            *index, window, cx,
                                                        ) {
                                                            Ok(()) => this.navigation_error = None,
                                                            Err(error) => {
                                                                this.navigation_error =
                                                                    Some(error.to_string())
                                                            }
                                                        }
                                                        cx.notify();
                                                    },
                                                ))
                                                .children(pinned_tabs.iter().cloned().map(
                                                    |route| {
                                                        Self::render_tab(route, true, shell.clone())
                                                    },
                                                )),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .id("nexora-regular-tabs-zone")
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_hidden()
                                    .child(
                                        TabBar::new("nexora-regular-tabs")
                                            .w_full()
                                            .h_full()
                                            .track_scroll(&self.regular_tab_scroll_handle)
                                            .menu(!regular_tabs.is_empty())
                                            .when_some(active_regular_tab_index, |this, index| {
                                                this.selected_index(index)
                                            })
                                            .on_click(cx.listener(
                                                |this, index: &usize, window, cx| {
                                                    match this
                                                        .select_regular_tab_in(*index, window, cx)
                                                    {
                                                        Ok(()) => this.navigation_error = None,
                                                        Err(error) => {
                                                            this.navigation_error =
                                                                Some(error.to_string())
                                                        }
                                                    }
                                                    cx.notify();
                                                },
                                            ))
                                            .children(regular_tabs.iter().cloned().map(|route| {
                                                Self::render_tab(route, false, shell.clone())
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("nexora-open-tabs-bottom-mask")
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(1.0))
                            .bg(title_bar_background),
                    )
                    .context_menu({
                        let shell = shell.clone();
                        move |menu, _, cx| {
                            let Some(shell_entity) = shell.upgrade() else {
                                return menu;
                            };
                            let Some(route) =
                                shell_entity.update(cx, |this, _| this.tab_context_route.take())
                            else {
                                return menu;
                            };
                            Self::build_tab_context_menu(menu, route, shell.clone(), cx)
                        }
                    }),
            )
            .child(
                div()
                    .id("titlebar-drag-space")
                    .flex_none()
                    .w(px(54.0))
                    .min_w(px(54.0))
                    .h_full(),
            )
            .into_any_element()
    }

    fn breadcrumb_items(&self) -> Vec<(String, Option<String>)> {
        let active_id = self.active_target_id();
        let Some(active_metadata) = self
            .registry
            .features()
            .iter()
            .find(|metadata| metadata.id() == active_id)
            .copied()
        else {
            return vec![(self.active_route.title(), None)];
        };
        let mut parents = Vec::new();
        let mut parent_id = active_metadata.parent();
        while let Some(id) = parent_id {
            let Some(parent) = self
                .registry
                .features()
                .iter()
                .find(|metadata| metadata.id() == id)
                .copied()
            else {
                break;
            };
            parent_id = parent.parent();
            parents.push(parent);
        }
        let section = active_metadata
            .section()
            .or_else(|| parents.iter().find_map(|metadata| metadata.section()))
            .unwrap_or("应用");
        let section_path = self
            .registry
            .navigation_features()
            .find(|metadata| {
                metadata.parent().is_none()
                    && metadata.section().unwrap_or("应用") == section
                    && !metadata.path().contains(':')
            })
            .map(|metadata| metadata.path().to_owned());
        let mut items = vec![(section.to_owned(), section_path)];
        parents.reverse();
        items.extend(parents.into_iter().map(|metadata| {
            let path = (!metadata.path().contains(':')).then(|| metadata.path().to_owned());
            (metadata.title().to_owned(), path)
        }));
        items.push((self.active_route.title(), None));
        items
    }

    fn render_panel_header(&self, cx: &mut Context<Self>) -> PanelHeader {
        let breadcrumb = self.breadcrumb_items().into_iter().fold(
            Breadcrumb::new(),
            |breadcrumb, (label, path)| {
                let item = match path {
                    Some(path) if path != self.active_path() => BreadcrumbItem::new(label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Err(error) = this.open_path(path.as_str(), window, cx) {
                                this.navigation_error = Some(error.to_string());
                            }
                            cx.notify();
                        })),
                    _ => BreadcrumbItem::new(label),
                };
                breadcrumb.child(item)
            },
        );
        let active_route = self.active_route.clone();
        let pinned = self.is_route_pinned(&active_route);

        PanelHeader::new(breadcrumb).action(
            Toggle::new("panel-pin-current-tab")
                .small()
                .checked(pinned)
                .icon(if pinned {
                    IconName::StarFill
                } else {
                    IconName::Star
                })
                .tooltip(if pinned {
                    "取消置顶当前标签"
                } else {
                    "置顶当前标签"
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_pin_route(&active_route);
                    cx.notify();
                })),
        )
    }

    fn render_active_feature(&self) -> AnyElement {
        self.feature_instances
            .get(self.active_path())
            .map(|instance| instance.view().into_any_element())
            .unwrap_or_else(|| div().into_any_element())
    }

    fn render_active_panel_overlay(&self, cx: &App) -> Option<AnyElement> {
        self.feature_instances
            .get(self.active_path())?
            .panel_overlay(cx)
            .map(IntoElement::into_any_element)
    }

    fn active_content_scrollable(&self) -> bool {
        self.registry
            .features()
            .iter()
            .find(|metadata| metadata.id() == self.active_target_id())
            .map(|metadata| metadata.content_scrollable())
            .unwrap_or(true)
    }

    fn render_workspace(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let navigation_error = self.navigation_error.clone();
        let active_feature = div()
            .relative()
            .size_full()
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
            });

        let layout = WorkspaceLayout::new(
            self.render_sidebar(cx),
            self.render_title_bar_content(cx),
            active_feature,
        )
        .with_sidebar_width(px(224.0))
        .with_sidebar_width_range(px(208.0)..px(300.0))
        .with_panel_header(self.render_panel_header(cx))
        .with_content_scrollable(self.active_content_scrollable());
        let layout = match self.render_active_panel_overlay(cx) {
            Some(overlay) => layout.with_panel_overlay(overlay),
            None => layout,
        };
        layout.render(window, cx)
    }
}

#[cfg(feature = "desktop")]
fn account_icon(kind: AccountActionKind) -> IconName {
    match kind {
        AccountActionKind::SignIn => IconName::CircleUser,
        AccountActionKind::Settings => IconName::Settings2,
        AccountActionKind::SignOut => IconName::CircleX,
    }
}

fn feature_icon(icon: Option<&str>) -> Icon {
    Icon::default().path(format!("icons/{}.svg", icon.unwrap_or("frame")))
}

impl Render for ApplicationShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        #[cfg(feature = "desktop")]
        let content = if self.account_enabled && !self.authenticated {
            let navigation_error = self.navigation_error.clone();
            div()
                .relative()
                .size_full()
                .child(self.login_feature.clone())
                .child(
                    TitleBar::new()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .border_b(px(0.0))
                        .bg(gpui::transparent_black()),
                )
                .when_some(navigation_error, |element, message| {
                    element.child(
                        div()
                            .absolute()
                            .left_3()
                            .right_3()
                            .bottom_3()
                            .child(Alert::error("nexora-login-navigation-error", message)),
                    )
                })
                .into_any_element()
        } else {
            self.render_workspace(window, cx)
        };
        #[cfg(not(feature = "desktop"))]
        let content = self.render_workspace(window, cx);

        let root = div()
            .relative()
            .size_full()
            .child(content)
            .children(ui::window_layers(window, cx));
        #[cfg(feature = "desktop")]
        let root = root.key_context(account_actions::CONTEXT);
        root.into_any_element()
    }
}

fn create_initial_feature(
    registry: &AppRegistry,
    initial_route: RouteMatch,
    window: &mut Window,
    cx: &mut App,
) -> (HashMap<String, FeatureInstance>, Option<String>) {
    let active_path = initial_route.concrete_path().to_owned();
    let mut feature_instances = HashMap::new();
    let navigation_error = match registry.create_feature(initial_route, window, cx) {
        Ok(mut instance) => {
            instance.activate(window, cx);
            feature_instances.insert(active_path, instance);
            None
        }
        Err(error) => Some(error.to_string()),
    };
    (feature_instances, navigation_error)
}
