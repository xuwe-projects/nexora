//! Feature 与 Window 的确定性应用注册表。

use std::collections::{HashMap, HashSet};

use gpui::{App, AppContext as _, Global, Window};
#[cfg(feature = "desktop")]
use gpui::{WindowHandle, WindowOptions};
use matchit::Router;
use thiserror::Error;

#[cfg(feature = "account-client")]
use crate::{__private::LoginFeatureRegistration, LoginFeature as LoginFeatureDefinition};
use crate::{
    __private::{
        FeatureRegistration, SettingsWindowRegistration, SidebarFooterRegistration,
        SidebarHeaderRegistration, WindowRegistration,
    },
    Feature, FeatureInstance, FeatureMetadata, FeatureRuntimeError, ResolveError, RouteMatch,
    RouteTarget, SettingsWindow as SettingsWindowDefinition,
    SidebarFooter as SidebarFooterDefinition, SidebarHeader as SidebarHeaderDefinition,
    Window as WindowDefinition, WindowMetadata,
    route::{RouteParameters, canonical_segment, decode_parameter, parse_location},
};
#[cfg(feature = "desktop")]
use crate::{WindowInstance, WindowRuntimeError};

/// 生成代码组装应用注册表时使用的构建器。
///
/// CLI 会按目录和显式 `order` 生成连续的 `.feature::<T>()`、`.window::<T>()` 调用，
/// 应用代码也可以直接使用该 API 编写小型注册表或测试。
#[derive(Debug, Default)]
pub struct AppRegistryBuilder {
    features: Vec<FeatureMetadata>,
    feature_registrations: Vec<FeatureRegistration>,
    windows: Vec<WindowMetadata>,
    window_registrations: Vec<WindowRegistration>,
    settings_windows: Vec<SettingsWindowRegistration>,
    #[cfg(feature = "account-client")]
    login_features: Vec<LoginFeatureRegistration>,
    sidebar_headers: Vec<SidebarHeaderRegistration>,
    sidebar_footers: Vec<SidebarFooterRegistration>,
}

impl AppRegistryBuilder {
    /// 注册一个实现 [`Feature`] 的业务功能类型。
    pub fn feature<F>(mut self) -> Self
    where
        F: Feature,
    {
        self.features.push(F::METADATA);
        if let Some(registration) = F::REGISTRATION {
            self.feature_registrations.push(registration);
        }
        self
    }

    /// 注册一个实现 [`Window`] 的独立窗口类型。
    pub fn window<W>(mut self) -> Self
    where
        W: WindowDefinition,
    {
        if let Some(registration) = W::REGISTRATION {
            if let Some(type_name) = registration.settings_window_type_name() {
                self.settings_windows
                    .push(SettingsWindowRegistration::new(type_name, registration));
                return self;
            }
            self.window_registrations.push(registration);
        }
        self.windows.push(W::METADATA);
        self
    }

    /// 注册一个应用级 Settings Window 覆盖类型。
    ///
    /// 同一个注册表最多允许一个覆盖项；没有注册时框架会自动使用默认设置窗口。重复项
    /// 会在 [`Self::build`] 阶段返回结构化错误。
    pub fn settings_window<S>(mut self) -> Self
    where
        S: SettingsWindowDefinition,
    {
        self.settings_windows
            .push(<S as SettingsWindowDefinition>::REGISTRATION);
        self
    }

    /// 注册一个 Account 桌面客户端使用的 Login Feature 覆盖类型。
    ///
    /// 同一个注册表最多允许一个覆盖项；没有注册时框架会自动使用默认登录页面。该 API
    /// 仅在启用 `account-client` 时存在。
    #[cfg(feature = "account-client")]
    pub fn login_feature<L>(mut self) -> Self
    where
        L: LoginFeatureDefinition,
    {
        self.login_features.push(L::REGISTRATION);
        self
    }

    /// 注册一个直接实现 GPUI `Render` 的主 Sidebar Header 类型。
    ///
    /// 同一个注册表最多允许一个 Header；重复项会在 [`Self::build`] 阶段返回结构化错误。
    pub fn sidebar_header<H>(mut self) -> Self
    where
        H: SidebarHeaderDefinition,
    {
        self.sidebar_headers.push(H::REGISTRATION);
        self
    }

    /// 注册一个直接实现 GPUI `Render` 的主 Sidebar Footer 类型。
    ///
    /// 同一个注册表最多允许一个 Footer；重复项会在 [`Self::build`] 阶段返回结构化错误。
    pub fn sidebar_footer<F>(mut self) -> Self
    where
        F: SidebarFooterDefinition,
    {
        self.sidebar_footers.push(F::REGISTRATION);
        self
    }

    /// 校验全部元数据并构建统一路径路由器。
    ///
    /// # Errors
    ///
    /// 路径格式无效、动态路径错误地进入导航、标识重复、父 Feature 不存在、专用界面
    /// 覆盖项重复，或者任意 Feature 与 Window 的路径模式发生冲突时返回
    /// [`RegistryError`]。
    pub fn build(mut self) -> Result<AppRegistry, RegistryError> {
        self.settings_windows
            .sort_by_key(SettingsWindowRegistration::type_name);
        let settings_window = unique_settings_window(self.settings_windows)?
            .unwrap_or_else(crate::defaults::default_settings_window_registration);
        let settings_window = settings_window.window();
        self.windows.push(settings_window.metadata());
        self.window_registrations.push(settings_window);

        #[cfg(feature = "account-client")]
        self.login_features
            .sort_by_key(LoginFeatureRegistration::type_name);
        #[cfg(feature = "account-client")]
        let login_feature = unique_login_feature(self.login_features)?
            .unwrap_or_else(crate::defaults::default_login_registration);

        self.features
            .sort_by_key(|metadata| (metadata.order(), metadata.path(), metadata.id()));
        self.windows
            .sort_by_key(|metadata| (metadata.order(), metadata.path(), metadata.id()));
        self.sidebar_headers
            .sort_by_key(SidebarHeaderRegistration::type_name);
        self.sidebar_footers
            .sort_by_key(SidebarFooterRegistration::type_name);

        validate_feature_ids(&self.features)?;
        validate_window_ids(&self.windows)?;
        validate_feature_parents(&self.features)?;
        let sidebar_header = unique_sidebar_header(self.sidebar_headers)?;
        let sidebar_footer = unique_sidebar_footer(self.sidebar_footers)?;

        let mut routes = Router::new();
        for metadata in &self.features {
            let route = validate_feature(*metadata)?;
            insert_route(
                &mut routes,
                route,
                metadata.path(),
                RouteTarget::Feature(*metadata),
            )?;
        }
        for metadata in &self.windows {
            let route = validate_window(*metadata)?;
            insert_route(
                &mut routes,
                route,
                metadata.path(),
                RouteTarget::Window(*metadata),
            )?;
        }

        Ok(AppRegistry {
            features: self.features,
            feature_registrations: self
                .feature_registrations
                .into_iter()
                .map(|registration| (registration.metadata().id(), registration))
                .collect(),
            windows: self.windows,
            window_registrations: self
                .window_registrations
                .into_iter()
                .map(|registration| (registration.metadata().id(), registration))
                .collect(),
            sidebar_header,
            sidebar_footer,
            #[cfg(feature = "account-client")]
            login_feature,
            routes,
        })
    }
}

/// 一个应用中全部 Feature、独立 Window 与路径模式的统一注册表。
///
/// Feature 与 Window 共用同一套路由树，因此相同或等价动态路径会在启动阶段直接报错，
/// 不会等到 deeplink 到达时才产生不确定行为。
pub struct AppRegistry {
    features: Vec<FeatureMetadata>,
    feature_registrations: HashMap<&'static str, FeatureRegistration>,
    windows: Vec<WindowMetadata>,
    window_registrations: HashMap<&'static str, WindowRegistration>,
    sidebar_header: Option<SidebarHeaderRegistration>,
    sidebar_footer: Option<SidebarFooterRegistration>,
    #[cfg(feature = "account-client")]
    login_feature: LoginFeatureRegistration,
    routes: Router<RouteTarget>,
}

#[derive(Default)]
struct SettingsWindowRuntime {
    handle: Option<WindowHandle<gpui_component::Root>>,
}

impl Global for SettingsWindowRuntime {}

impl AppRegistry {
    /// 创建一个空注册表构建器。
    pub fn builder() -> AppRegistryBuilder {
        AppRegistryBuilder::default()
    }

    /// 自动发现当前程序中所有派生的 Feature 与 Window，并构建统一注册表。
    ///
    /// `#[derive(nexora::Feature)]` 与 `#[derive(nexora::Window)]` 会在链接时提交静态
    /// 元数据；Login Feature 与 Settings Window 派生宏只提交应用级覆盖项。没有覆盖时
    /// 注册表使用框架默认专用界面，有且只有一个覆盖时替换默认实现，多个覆盖则返回
    /// 确定性错误。应用无需维护额外的 `features()` 或 `windows()` 注册函数。
    ///
    /// # Errors
    ///
    /// 任意自动发现的元数据无效、专用界面覆盖重复或路由互相冲突时返回
    /// [`RegistryError`]。
    pub fn discover() -> Result<Self, RegistryError> {
        let mut builder = AppRegistryBuilder::default();
        inventory::iter::<crate::__private::FeatureRegistration>
            .into_iter()
            .for_each(|registration| {
                builder.features.push(registration.metadata());
                builder.feature_registrations.push(*registration);
            });
        inventory::iter::<crate::__private::WindowRegistration>
            .into_iter()
            .for_each(|registration| {
                builder.windows.push(registration.metadata());
                builder.window_registrations.push(*registration);
            });
        builder.settings_windows.extend(
            inventory::iter::<crate::__private::SettingsWindowRegistration>
                .into_iter()
                .copied(),
        );
        #[cfg(feature = "account-client")]
        builder.login_features.extend(
            inventory::iter::<crate::__private::LoginFeatureRegistration>
                .into_iter()
                .copied(),
        );
        builder.sidebar_headers.extend(
            inventory::iter::<crate::__private::SidebarHeaderRegistration>
                .into_iter()
                .copied(),
        );
        builder.sidebar_footers.extend(
            inventory::iter::<crate::__private::SidebarFooterRegistration>
                .into_iter()
                .copied(),
        );
        builder.build()
    }

    /// 返回自动发现或手动注册的全部 Feature。
    ///
    /// 该方法只用于导航生成、调试和其他只读查询，不负责触发注册。顺序由 `order`、路径
    /// 和稳定标识共同确定。
    pub fn features(&self) -> &[FeatureMetadata] {
        &self.features
    }

    /// 返回自动发现或手动注册的全部独立 Window。
    ///
    /// 该方法只用于窗口目录、调试和其他只读查询，不负责触发注册。顺序由 `order`、路径
    /// 和稳定标识共同确定。
    pub fn windows(&self) -> &[WindowMetadata] {
        &self.windows
    }

    /// 创建当前应用注册的 Sidebar Header Entity。
    ///
    /// 没有注册 Header 时返回 `None`。工厂只在调用本方法时执行一次，应用 Shell 会保存
    /// 返回的 `AnyView` 并在后续渲染中复用同一个 Entity。
    pub fn create_sidebar_header(
        &self,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<gpui::AnyView> {
        self.sidebar_header
            .map(|registration| (registration.factory())(window, cx))
    }

    /// 创建当前应用注册的 Sidebar Footer Entity。
    ///
    /// 没有注册 Footer 时返回 `None`。工厂只在调用本方法时执行一次，应用 Shell 会保存
    /// 返回的 `AnyView` 并在后续渲染中复用同一个 Entity。
    pub fn create_sidebar_footer(
        &self,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<gpui::AnyView> {
        self.sidebar_footer
            .map(|registration| (registration.factory())(window, cx))
    }

    /// 创建 Account 桌面客户端当前选中的 Login Feature Entity。
    ///
    /// 应用没有声明覆盖类型时使用框架默认实现；存在一个覆盖类型时使用该实现。工厂只
    /// 负责创建 Entity，主窗口 Shell 应保存并复用返回的 `AnyView`。
    #[cfg(feature = "account-client")]
    pub fn create_login_feature(&self, window: &mut Window, cx: &mut App) -> gpui::AnyView {
        (self.login_feature.factory())(window, cx)
    }

    /// 返回应该出现在主侧边栏中的 Feature。
    pub fn navigation_features(&self) -> impl Iterator<Item = FeatureMetadata> + '_ {
        self.features
            .iter()
            .copied()
            .filter(|metadata| metadata.navigation())
    }

    /// 返回指定父 Feature 下的直接子导航定义。
    pub fn children_of(&self, parent_id: &str) -> impl Iterator<Item = FeatureMetadata> + '_ {
        let parent_id = parent_id.to_owned();
        self.navigation_features()
            .filter(move |metadata| metadata.parent() == Some(parent_id.as_str()))
    }

    /// 解析内部路径或 custom scheme URI，并返回具体 Feature/Window 与动态参数。
    ///
    /// `myapp://users/details/42` 会被规范化为 `/users/details/42`。框架本身不限制
    /// scheme 名称，具体应用可以在操作系统注册自己的唯一 custom scheme。
    ///
    /// # Errors
    ///
    /// 输入位置无法解析、动态参数不是有效 UTF-8 percent encoding，或者没有注册目标
    /// 与路径匹配时返回 [`ResolveError`]。
    pub fn resolve(&self, location: &str) -> Result<RouteMatch, ResolveError> {
        let location = parse_location(location)?;
        let matched = self
            .routes
            .at(&location.path)
            .map_err(|_| ResolveError::NotFound {
                path: location.path.clone(),
            })?;
        let parameters = matched
            .params
            .iter()
            .map(|(name, value)| {
                decode_parameter(name, value).map(|value| (name.to_owned(), value))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(RouteMatch::new(
            *matched.value,
            location.path,
            RouteParameters::from_pairs(parameters),
            location.query,
        ))
    }

    /// 根据已解析路由创建一个拥有独立 GPUI Entity 的 Feature 实例。
    ///
    /// 参数会按照派生宏声明的 `path_params` 与 `query_params` 类型完成反序列化，随后
    /// 框架调用页面工厂和一次性 `initialize` 生命周期。返回实例可以直接作为 `AnyView`
    /// 放入标签内容区。
    ///
    /// # Errors
    ///
    /// 路由指向 Window、Feature 没有运行时工厂，或者强类型参数提取失败时返回错误。
    pub fn create_feature(
        &self,
        route: RouteMatch,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<FeatureInstance, FeatureRuntimeError> {
        let RouteTarget::Feature(metadata) = route.target() else {
            return Err(FeatureRuntimeError::WindowTarget {
                path: route.concrete_path().to_owned(),
            });
        };
        let registration = self
            .feature_registrations
            .get(metadata.id())
            .ok_or(FeatureRuntimeError::MissingFactory { id: metadata.id() })?;

        (registration.factory())(route, window, cx)
    }

    /// 根据已解析路由创建一个绑定强类型参数的独立 Window Entity。
    ///
    /// 该方法用于需要自行控制原生窗口外壳的高级场景。普通应用应优先调用
    /// [`Self::open_window`]，让框架统一创建窗口并挂载 `gpui_component::Root`。
    ///
    /// # Errors
    ///
    /// 路由指向 Feature、Window 没有运行时工厂，或者强类型路径与查询参数提取失败时
    /// 返回 [`WindowRuntimeError`]。
    #[cfg(feature = "desktop")]
    pub fn create_window(
        &self,
        route: RouteMatch,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<WindowInstance, WindowRuntimeError> {
        let RouteTarget::Window(metadata) = route.target() else {
            return Err(WindowRuntimeError::FeatureTarget {
                path: route.concrete_path().to_owned(),
            });
        };
        let registration = self
            .window_registrations
            .get(metadata.id())
            .ok_or(WindowRuntimeError::MissingFactory { id: metadata.id() })?;

        (registration.factory())(route, window, cx)
    }

    /// 根据已解析路由创建独立原生窗口。
    ///
    /// 框架会先完成强类型路由校验和 Window 自定义选项计算，再在当前 GPUI 进程中调用
    /// `App::open_window`，把页面 Entity 包裹进 `gpui_component::Root` 并挂载应用主题。
    /// Window 不会进入主窗口导航或标签缓存。保留的 `settings` 窗口采用单实例语义；已有
    /// 窗口仍然存活时会直接激活，关闭后再次调用才创建新实例。
    ///
    /// # Errors
    ///
    /// 路由指向 Feature、Window 没有运行时工厂、参数提取失败，或当前平台无法创建原生
    /// 窗口时返回 [`WindowRuntimeError`]。
    ///
    /// # Panics
    ///
    /// 直接调用该方法前必须已经初始化 `gpui_component` 与应用主题；通过
    /// [`crate::Application::run`] 启动时框架会自动满足该前置条件。
    #[cfg(feature = "desktop")]
    pub fn open_window(
        &self,
        route: RouteMatch,
        cx: &mut App,
    ) -> Result<WindowHandle<gpui_component::Root>, WindowRuntimeError> {
        let RouteTarget::Window(metadata) = route.target() else {
            return Err(WindowRuntimeError::FeatureTarget {
                path: route.concrete_path().to_owned(),
            });
        };
        let registration = self
            .window_registrations
            .get(metadata.id())
            .ok_or(WindowRuntimeError::MissingFactory { id: metadata.id() })?;
        let options: WindowOptions = (registration.options_factory())(&route, cx)?;
        if metadata.id() == "settings"
            && let Some(handle) = cx
                .try_global::<SettingsWindowRuntime>()
                .and_then(|runtime| runtime.handle)
        {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                cx.activate(true);
                return Ok(handle);
            }
            cx.global_mut::<SettingsWindowRuntime>().handle = None;
        }
        let factory = registration.factory();
        let window_id = metadata.id();

        let handle = cx
            .open_window(options, move |window, cx| {
                let instance = factory(route, window, cx)
                    .expect("Window 路由已在创建原生窗口前完成相同的强类型校验");
                let root = cx.new(|cx| gpui_component::Root::new(instance.view(), window, cx));
                theme::attach_window(window, cx);
                root
            })
            .map_err(|source| WindowRuntimeError::OpenWindow {
                id: window_id,
                message: source.to_string(),
            })?;
        if metadata.id() == "settings" {
            cx.default_global::<SettingsWindowRuntime>().handle = Some(handle);
            _ = handle.update(cx, |_, window, _| window.activate_window());
        }
        cx.activate(true);
        Ok(handle)
    }
}

/// 构建应用注册表时发现的配置错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// 某个 Feature 的路径或导航配置无效。
    #[error("Feature `{id}` 配置无效：{message}")]
    InvalidFeature {
        /// 出错 Feature 的稳定标识。
        id: &'static str,
        /// 面向开发者的具体校验信息。
        message: String,
    },
    /// 某个独立 Window 的路径配置无效。
    #[error("Window `{id}` 配置无效：{message}")]
    InvalidWindow {
        /// 出错 Window 的稳定标识。
        id: &'static str,
        /// 面向开发者的具体校验信息。
        message: String,
    },
    /// 两个 Feature 使用了相同稳定标识。
    #[error("Feature 稳定标识 `{id}` 重复")]
    DuplicateFeatureId {
        /// 重复的 Feature 标识。
        id: &'static str,
    },
    /// 两个独立 Window 使用了相同稳定标识。
    #[error("Window 稳定标识 `{id}` 重复")]
    DuplicateWindowId {
        /// 重复的 Window 标识。
        id: &'static str,
    },
    /// 应用同时注册了多个 Login Feature 覆盖实现。
    #[error("Login Feature 重复注册：`{first}` 与 `{duplicate}`")]
    DuplicateLoginFeature {
        /// 按 Rust 类型名称排序后首先出现的 Login Feature。
        first: &'static str,
        /// 与首个覆盖实现冲突的 Login Feature。
        duplicate: &'static str,
    },
    /// 应用同时注册了多个 Settings Window 覆盖实现。
    #[error("Settings Window 重复注册：`{first}` 与 `{duplicate}`")]
    DuplicateSettingsWindow {
        /// 按 Rust 类型名称排序后首先出现的 Settings Window。
        first: &'static str,
        /// 与首个覆盖实现冲突的 Settings Window。
        duplicate: &'static str,
    },
    /// 应用同时注册了多个 Sidebar Header 实现。
    #[error("Sidebar Header 重复注册：`{first}` 与 `{duplicate}`")]
    DuplicateSidebarHeader {
        /// 排序后首先出现的 Header Rust 类型名称。
        first: &'static str,
        /// 与首个实现冲突的 Header Rust 类型名称。
        duplicate: &'static str,
    },
    /// 应用同时注册了多个 Sidebar Footer 实现。
    #[error("Sidebar Footer 重复注册：`{first}` 与 `{duplicate}`")]
    DuplicateSidebarFooter {
        /// 排序后首先出现的 Footer Rust 类型名称。
        first: &'static str,
        /// 与首个实现冲突的 Footer Rust 类型名称。
        duplicate: &'static str,
    },
    /// 子 Feature 引用了不存在的父 Feature。
    #[error("Feature `{id}` 引用了不存在的父 Feature `{parent}`")]
    UnknownFeatureParent {
        /// 子 Feature 的稳定标识。
        id: &'static str,
        /// 未找到的父 Feature 标识。
        parent: &'static str,
    },
    /// 可见子 Feature 指向了不会出现在导航中的父 Feature。
    #[error("Feature `{id}` 的父 Feature `{parent}` 设置了 navigation = false")]
    HiddenFeatureParent {
        /// 无法从根导航到达的子 Feature 标识。
        id: &'static str,
        /// 被隐藏的父 Feature 标识。
        parent: &'static str,
    },
    /// Feature 的父子关系形成了自引用或更长的循环。
    #[error("Feature `{id}` 的父级关系形成循环")]
    FeatureParentCycle {
        /// 检测到循环的 Feature 标识。
        id: &'static str,
    },
    /// 两个 Feature/Window 注册了相同或等价的路径模式。
    #[error("路径模式 `{path}` 与已注册模式 `{conflict}` 冲突")]
    RouteConflict {
        /// 后插入并触发冲突的路径模式。
        path: &'static str,
        /// 路由器报告的已存在模式。
        conflict: String,
    },
}

#[cfg(feature = "account-client")]
fn unique_login_feature(
    registrations: Vec<LoginFeatureRegistration>,
) -> Result<Option<LoginFeatureRegistration>, RegistryError> {
    let mut registrations = registrations.into_iter();
    let Some(first) = registrations.next() else {
        return Ok(None);
    };
    if let Some(duplicate) = registrations.next() {
        return Err(RegistryError::DuplicateLoginFeature {
            first: first.type_name(),
            duplicate: duplicate.type_name(),
        });
    }
    Ok(Some(first))
}

fn unique_settings_window(
    registrations: Vec<SettingsWindowRegistration>,
) -> Result<Option<SettingsWindowRegistration>, RegistryError> {
    let mut registrations = registrations.into_iter();
    let Some(first) = registrations.next() else {
        return Ok(None);
    };
    if let Some(duplicate) = registrations.next() {
        return Err(RegistryError::DuplicateSettingsWindow {
            first: first.type_name(),
            duplicate: duplicate.type_name(),
        });
    }
    Ok(Some(first))
}

fn unique_sidebar_header(
    registrations: Vec<SidebarHeaderRegistration>,
) -> Result<Option<SidebarHeaderRegistration>, RegistryError> {
    let mut registrations = registrations.into_iter();
    let Some(first) = registrations.next() else {
        return Ok(None);
    };
    if let Some(duplicate) = registrations.next() {
        return Err(RegistryError::DuplicateSidebarHeader {
            first: first.type_name(),
            duplicate: duplicate.type_name(),
        });
    }
    Ok(Some(first))
}

fn unique_sidebar_footer(
    registrations: Vec<SidebarFooterRegistration>,
) -> Result<Option<SidebarFooterRegistration>, RegistryError> {
    let mut registrations = registrations.into_iter();
    let Some(first) = registrations.next() else {
        return Ok(None);
    };
    if let Some(duplicate) = registrations.next() {
        return Err(RegistryError::DuplicateSidebarFooter {
            first: first.type_name(),
            duplicate: duplicate.type_name(),
        });
    }
    Ok(Some(first))
}

fn validate_feature_ids(features: &[FeatureMetadata]) -> Result<(), RegistryError> {
    let mut ids = HashSet::with_capacity(features.len());
    for metadata in features {
        if !ids.insert(metadata.id()) {
            return Err(RegistryError::DuplicateFeatureId { id: metadata.id() });
        }
    }
    Ok(())
}

fn validate_window_ids(windows: &[WindowMetadata]) -> Result<(), RegistryError> {
    let mut ids = HashSet::with_capacity(windows.len());
    for metadata in windows {
        if !ids.insert(metadata.id()) {
            return Err(RegistryError::DuplicateWindowId { id: metadata.id() });
        }
    }
    Ok(())
}

fn validate_feature_parents(features: &[FeatureMetadata]) -> Result<(), RegistryError> {
    let definitions = features
        .iter()
        .map(|metadata| (metadata.id(), *metadata))
        .collect::<HashMap<_, _>>();
    for metadata in features {
        if let Some(parent_id) = metadata.parent() {
            let Some(parent) = definitions.get(parent_id) else {
                return Err(RegistryError::UnknownFeatureParent {
                    id: metadata.id(),
                    parent: parent_id,
                });
            };
            if metadata.navigation() && !parent.navigation() {
                return Err(RegistryError::HiddenFeatureParent {
                    id: metadata.id(),
                    parent: parent_id,
                });
            }
        }

        let mut current = *metadata;
        let mut visited = HashSet::new();
        while let Some(parent_id) = current.parent() {
            if !visited.insert(current.id()) {
                return Err(RegistryError::FeatureParentCycle { id: metadata.id() });
            }
            let Some(parent) = definitions.get(parent_id) else {
                break;
            };
            current = *parent;
        }
    }
    Ok(())
}

fn validate_feature(metadata: FeatureMetadata) -> Result<String, RegistryError> {
    let route =
        validate_path(metadata.path()).map_err(|message| RegistryError::InvalidFeature {
            id: metadata.id(),
            message,
        })?;
    if metadata.navigation() && route.contains('{') {
        return Err(RegistryError::InvalidFeature {
            id: metadata.id(),
            message: "包含动态参数的 Feature 必须设置 navigation = false".to_owned(),
        });
    }
    Ok(route)
}

fn validate_window(metadata: WindowMetadata) -> Result<String, RegistryError> {
    validate_path(metadata.path()).map_err(|message| RegistryError::InvalidWindow {
        id: metadata.id(),
        message,
    })
}

fn validate_path(path: &'static str) -> Result<String, String> {
    if !path.starts_with('/') {
        return Err("path 必须以 `/` 开头".to_owned());
    }
    if path.contains(['?', '#']) || path.contains("://") {
        return Err("path 只能包含逻辑路径，不能包含 scheme、查询参数或片段".to_owned());
    }
    if path.len() > 1 && path.ends_with('/') {
        return Err("除根路径外，path 不能以 `/` 结尾".to_owned());
    }
    if path.contains("//") {
        return Err("path 不能包含空路径段".to_owned());
    }

    let mut parameters = HashSet::new();
    let mut route = String::with_capacity(path.len() + 4);
    for (index, segment) in path.split('/').enumerate() {
        if index > 0 {
            route.push('/');
        }
        if let Some(parameter) = segment.strip_prefix(':') {
            if !valid_parameter_name(parameter) {
                return Err(format!("动态参数名 `{parameter}` 无效"));
            }
            if !parameters.insert(parameter) {
                return Err(format!("动态参数名 `{parameter}` 重复"));
            }
            if parameters.len() > 25 {
                return Err("单条 path 最多支持 25 个动态参数".to_owned());
            }
            route.push('{');
            route.push_str(parameter);
            route.push('}');
        } else {
            if segment.contains([':', '{', '}', '*']) {
                return Err(format!("静态路径段 `{segment}` 包含保留字符"));
            }
            route.push_str(&canonical_segment(segment)?);
        }
    }
    Ok(route)
}

fn valid_parameter_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn insert_route(
    routes: &mut Router<RouteTarget>,
    route: String,
    public_path: &'static str,
    target: RouteTarget,
) -> Result<(), RegistryError> {
    routes
        .insert(route, target)
        .map_err(|error| RegistryError::RouteConflict {
            path: public_path,
            conflict: error.to_string(),
        })
}
