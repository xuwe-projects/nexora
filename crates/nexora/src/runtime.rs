//! GPUI Feature 实例、强类型路由上下文和生命周期运行时。

use std::{any::Any, collections::HashMap};

#[cfg(feature = "desktop")]
use std::rc::Rc;

use gpui::{
    AnyView, App, AppContext as _, Context, EntityId, Global, IntoElement, Render, Window,
    WindowOptions,
};
use serde::Deserialize;
use thiserror::Error;

use crate::{
    Feature, Path, Query, RouteExtractError, RouteMatch, RouteTarget, Window as WindowDefinition,
};

/// 不声明动态路径参数时使用的零尺寸类型。
///
/// `#[derive(nexora::Feature)]` 会在没有 `path_params` 属性时自动选择该类型，应用代码
/// 通常不需要直接构造它。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoPath {}

/// 不声明查询参数时使用的零尺寸类型。
///
/// 当路由不接受查询字符串时，派生宏会把 [`Feature::Query`] 设为该类型。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoQuery {}

/// 一个 Feature 实例已经完成校验的强类型路由上下文。
///
/// 路径参数和查询参数在 Entity 创建前完成反序列化，因此 Feature 生命周期和渲染方法
/// 读取到的值始终与当前具体路径一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRoute<P, Q> {
    concrete_path: String,
    path: Path<P>,
    query: Query<Q>,
}

impl<P, Q> FeatureRoute<P, Q> {
    /// 返回不包含查询字符串的规范化具体路径。
    ///
    /// 该值同时是默认标签实例键；例如 `/users/1` 与 `/users/2` 会创建两个页面实例。
    pub fn concrete_path(&self) -> &str {
        &self.concrete_path
    }

    /// 返回已经完成字段和类型校验的动态路径参数。
    pub const fn path(&self) -> &Path<P> {
        &self.path
    }

    /// 返回已经完成字段和类型校验的查询参数。
    pub const fn query(&self) -> &Query<Q> {
        &self.query
    }
}

/// 一个独立 Window 实例已经完成校验的强类型路由上下文。
///
/// 该值在原生窗口 Entity 创建前完成路径和查询反序列化，窗口构造、初始化及渲染阶段
/// 因而不会接触未经校验的字符串参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRoute<P, Q> {
    concrete_path: String,
    path: Path<P>,
    query: Query<Q>,
}

impl<P, Q> WindowRoute<P, Q> {
    /// 返回不包含查询字符串的规范化具体路径。
    pub fn concrete_path(&self) -> &str {
        &self.concrete_path
    }

    /// 返回已经完成字段和类型校验的动态路径参数。
    pub const fn path(&self) -> &Path<P> {
        &self.path
    }

    /// 返回已经完成字段和类型校验的查询参数。
    pub const fn query(&self) -> &Query<Q> {
        &self.query
    }
}

/// Nexora 可以在独立原生窗口中实例化的 GPUI 页面契约。
///
/// 该 trait 只会由根 crate 在启用 `desktop` Cargo feature 时公开。派生宏负责生成
/// GPUI [`Render`] 转发，应用只需要在这里实现窗口内容和可选生命周期。
pub trait WindowElement: WindowDefinition + Sized + Render {
    /// 构造当前独立窗口的完整 Element 树。
    ///
    /// 该方法没有默认实现；渲染过程只应读取现有状态并构造元素，不应创建长期 Entity、
    /// 订阅或异步任务。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;

    /// 根据强类型路由和当前应用状态创建原生窗口选项。
    ///
    /// 默认返回 [`WindowOptions::default`]。需要固定尺寸、标题栏或显示器策略的窗口可以
    /// 覆盖该方法，而无需接管窗口创建与 `gpui_component::Root` 挂载流程。
    fn window_options(_route: &WindowRoute<Self::Path, Self::Query>, _cx: &App) -> WindowOptions {
        WindowOptions::default()
    }

    /// 在窗口 Entity 创建并绑定强类型路由后执行一次初始化。
    fn initialize(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// 在窗口 Entity 释放前执行通知式清理。
    fn closing(&mut self, _window: &mut Window, _cx: &mut App) {}
}

/// Nexora 可自动实例化的 GPUI Feature 行为契约。
///
/// 应用在本 trait 中同时实现完整 Panel 内容和可选生命周期。`#[derive(nexora::Feature)]`
/// 会为具体页面生成 [`Render`] 实现，并把 GPUI 的渲染调用直接转发到 [`Self::render`]；
/// 这样调用方只需要维护一份页面实现，同时仍能让 Entity 作为原生 GPUI View 使用。
///
/// # Examples
///
/// ```no_run
/// use nexora::{
///     FeatureElement,
///     gpui::{Context, Empty, IntoElement, Window},
/// };
///
/// #[derive(Default, nexora::Feature)]
/// #[nexora(title = "首页", path = "/")]
/// struct HomeFeature;
///
/// impl FeatureElement for HomeFeature {
///     fn render(
///         &mut self,
///         _window: &mut Window,
///         _cx: &mut Context<Self>,
///     ) -> impl IntoElement {
///         Empty
///     }
/// }
/// ```
pub trait FeatureElement: Feature + Sized + Render {
    /// 构造当前 Feature 的完整 Panel Element 树。
    ///
    /// 该方法没有默认实现，每个 Feature 都必须明确决定自己的页面内容。渲染过程只应
    /// 读取状态并构造 Element；Entity、订阅和异步任务等长期副作用应放在
    /// [`Self::initialize`] 或其他明确生命周期中。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;

    /// 在 Entity 已创建且强类型路由已绑定后执行一次初始化。
    ///
    /// 子 Entity、订阅、焦点句柄和需要随页面释放而取消的任务应在这里创建；不要在
    /// [`Self::render`] 中执行这些副作用。
    fn initialize(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// 在页面实例从不可见状态变为当前活动标签时调用。
    fn activated(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// 在页面实例被其他标签替换或活动标签即将关闭时调用。
    fn deactivated(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// 在同一具体路径复用现有 Entity、但查询等路由上下文发生变化后调用。
    ///
    /// `previous` 保存更新前的强类型值；当前值可以通过 [`FeatureContextExt::path`] 和
    /// [`FeatureContextExt::query`] 读取。
    fn route_changed(
        &mut self,
        _previous: &FeatureRoute<Self::Path, Self::Query>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 在标签实例从运行时缓存永久移除、Entity 句柄被释放前调用。
    ///
    /// 该方法只用于通知式清理，不应承担“是否允许关闭”的确认逻辑。
    fn closing(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
}

/// 在 Feature 自身的 GPUI 上下文中读取当前强类型路由。
///
/// 访问器返回拥有值的 [`Path`] 和 [`Query`] 克隆，避免借用框架内部路由存储后阻止页面
/// 继续可变使用 `Context<Self>`。
pub trait FeatureContextExt<F>
where
    F: Feature,
{
    /// 返回当前 Entity 对应的动态路径参数。
    ///
    /// # Panics
    ///
    /// 仅当 Entity 不是由 Nexora Feature 工厂创建，或框架内部路由状态被破坏时 panic。
    fn path(&self) -> Path<F::Path>;

    /// 返回当前 Entity 对应的查询参数。
    ///
    /// # Panics
    ///
    /// 仅当 Entity 不是由 Nexora Feature 工厂创建，或框架内部路由状态被破坏时 panic。
    fn query(&self) -> Query<F::Query>;

    /// 返回当前 Entity 的完整强类型路由上下文。
    ///
    /// # Panics
    ///
    /// 仅当 Entity 不是由 Nexora Feature 工厂创建，或框架内部路由状态被破坏时 panic。
    fn feature_route(&self) -> FeatureRoute<F::Path, F::Query>;
}

impl<F> FeatureContextExt<F> for Context<'_, F>
where
    F: Feature,
{
    fn path(&self) -> Path<F::Path> {
        self.feature_route().path
    }

    fn query(&self) -> Query<F::Query> {
        self.feature_route().query
    }

    fn feature_route(&self) -> FeatureRoute<F::Path, F::Query> {
        feature_route::<F>(self.entity_id(), self)
    }
}

/// 在 Window 自身的 GPUI 上下文中读取当前强类型路由。
///
/// 访问器与 [`FeatureContextExt`] 保持一致，返回拥有值的 [`Path`]、[`Query`] 和
/// [`WindowRoute`] 克隆，避免框架内部存储的借用扩散到窗口业务代码。
pub trait WindowContextExt<W>
where
    W: WindowDefinition,
{
    /// 返回当前窗口 Entity 对应的动态路径参数。
    ///
    /// # Panics
    ///
    /// 仅当 Entity 不是由 Nexora Window 工厂创建，或内部路由状态被破坏时 panic。
    fn path(&self) -> Path<W::Path>;

    /// 返回当前窗口 Entity 对应的查询参数。
    ///
    /// # Panics
    ///
    /// 仅当 Entity 不是由 Nexora Window 工厂创建，或内部路由状态被破坏时 panic。
    fn query(&self) -> Query<W::Query>;

    /// 返回当前窗口 Entity 的完整强类型路由上下文。
    ///
    /// # Panics
    ///
    /// 仅当 Entity 不是由 Nexora Window 工厂创建，或内部路由状态被破坏时 panic。
    fn window_route(&self) -> WindowRoute<W::Path, W::Query>;
}

impl<W> WindowContextExt<W> for Context<'_, W>
where
    W: WindowDefinition,
{
    fn path(&self) -> Path<W::Path> {
        self.window_route().path
    }

    fn query(&self) -> Query<W::Query> {
        self.window_route().query
    }

    fn window_route(&self) -> WindowRoute<W::Path, W::Query> {
        window_route::<W>(self.entity_id(), self)
    }
}

/// Feature 或 Window 提交框架导航请求时可能发生的错误。
#[cfg(feature = "desktop")]
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NavigationRequestError {
    /// 当前 GPUI 应用尚未创建 Nexora 主窗口 Shell，无法接收导航请求。
    #[error("Nexora 主窗口尚未初始化，当前上下文无法提交导航请求")]
    DispatcherUnavailable,
}

/// 为任意 Nexora 页面上下文提供延迟且不会重入 Entity 的导航入口。
///
/// `navigate` 接受内部路径或 deeplink 字符串。路径解析、Feature 标签复用和独立 Window
/// 打开统一由当前应用 Shell 处理，调用页面不需要持有父视图或注册表句柄。
#[cfg(feature = "desktop")]
pub trait NavigationContextExt {
    /// 在当前 Entity 更新完成后请求打开指定路径。
    ///
    /// Feature 目标会打开或激活主窗口标签；Window 目标会创建独立原生窗口，并且不会
    /// 进入主导航或标签。该方法只排队请求，不同步执行目标页面生命周期。
    ///
    /// # Errors
    ///
    /// 当前应用尚未创建 Nexora 主窗口 Shell 时返回
    /// [`NavigationRequestError::DispatcherUnavailable`]。
    fn navigate(&mut self, location: impl Into<String>) -> Result<(), NavigationRequestError>;
}

#[cfg(feature = "desktop")]
impl<T> NavigationContextExt for Context<'_, T>
where
    T: 'static,
{
    fn navigate(&mut self, location: impl Into<String>) -> Result<(), NavigationRequestError> {
        let handler = self
            .has_global::<NavigationDispatcher>()
            .then(|| self.global::<NavigationDispatcher>().handler.clone())
            .flatten()
            .ok_or(NavigationRequestError::DispatcherUnavailable)?;
        let location = location.into();
        self.defer(move |cx| handler(location, cx));
        Ok(())
    }
}

#[cfg(feature = "desktop")]
type NavigationHandler = Rc<dyn Fn(String, &mut App)>;

#[cfg(feature = "desktop")]
#[derive(Default)]
struct NavigationDispatcher {
    handler: Option<NavigationHandler>,
}

#[cfg(feature = "desktop")]
impl Global for NavigationDispatcher {}

#[cfg(feature = "desktop")]
pub(crate) fn install_navigation_handler(
    handler: impl Fn(String, &mut App) + 'static,
    cx: &mut App,
) {
    let dispatcher = NavigationDispatcher {
        handler: Some(Rc::new(handler)),
    };
    if cx.has_global::<NavigationDispatcher>() {
        *cx.global_mut::<NavigationDispatcher>() = dispatcher;
    } else {
        cx.set_global(dispatcher);
    }
}

#[cfg(feature = "desktop")]
pub(crate) fn clear_navigation_handler(cx: &mut App) {
    if cx.has_global::<NavigationDispatcher>() {
        cx.global_mut::<NavigationDispatcher>().handler = None;
    }
}

/// 创建或更新 Feature 运行时实例时可能发生的错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FeatureRuntimeError {
    /// 路径或查询参数无法反序列化为 Feature 声明的业务类型。
    #[error(transparent)]
    Extract(
        /// 保留具体参数来源、目标类型、字段路径和反序列化失败原因的底层错误。
        #[from]
        RouteExtractError,
    ),

    /// 调用方尝试使用 Window 路由创建 Feature Entity。
    #[error("路由 `{path}` 指向 Window，不能创建 Feature 实例")]
    WindowTarget {
        /// 无法作为 Feature 创建的具体路径。
        path: String,
    },

    /// 当前 Feature 只有元数据，没有由派生宏生成的运行时工厂。
    #[error("Feature `{id}` 没有可用的运行时工厂")]
    MissingFactory {
        /// 缺少工厂的 Feature 稳定标识。
        id: &'static str,
    },

    /// 路由目标元数据与当前页面工厂声明不一致。
    #[error(
        "路由 `{path}` 指向 Feature `{actual}`（模式 `{actual_path}`），不能由 Feature `{expected}`（模式 `{expected_path}`）创建"
    )]
    FeatureTargetMismatch {
        /// 当前工厂声明的 Feature 稳定标识。
        expected: &'static str,
        /// 当前工厂声明的 Feature 路径模式。
        expected_path: &'static str,
        /// 路由实际指向的 Feature 稳定标识。
        actual: &'static str,
        /// 路由目标注册时使用的路径模式。
        actual_path: &'static str,
        /// 无法交给当前工厂创建的规范化具体路径。
        path: String,
    },

    /// 新路由与现有页面实例不是同一个 Feature 或具体路径。
    #[error("不能使用路由 `{actual}` 更新实例 `{expected}`")]
    RouteInstanceMismatch {
        /// 当前实例的规范化具体路径。
        expected: String,
        /// 调用方提供的新规范化具体路径。
        actual: String,
    },

    /// 调用方尝试修改已经永久关闭的页面实例。
    #[error("已关闭的 Feature 实例 `{path}` 不能再更新路由")]
    ClosedInstance {
        /// 已经关闭的页面实例路径。
        path: String,
    },
}

/// 创建或打开独立 Window 时可能发生的错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WindowRuntimeError {
    /// 路径或查询参数无法反序列化为 Window 声明的业务类型。
    #[error(transparent)]
    Extract(
        /// 保留参数来源、目标类型、字段路径和失败原因的底层错误。
        #[from]
        RouteExtractError,
    ),

    /// 调用方尝试使用 Feature 路由创建独立 Window。
    #[error("路由 `{path}` 指向 Feature，不能创建 Window 实例")]
    FeatureTarget {
        /// 无法作为独立 Window 创建的具体路径。
        path: String,
    },

    /// 当前 Window 只有元数据，没有由派生宏生成的运行时工厂。
    #[error("Window `{id}` 没有可用的运行时工厂")]
    MissingFactory {
        /// 缺少工厂的 Window 稳定标识。
        id: &'static str,
    },

    /// 路由目标元数据与当前 Window 工厂声明不一致。
    #[error(
        "路由 `{path}` 指向 Window `{actual}`（模式 `{actual_path}`），不能由 Window `{expected}`（模式 `{expected_path}`）创建"
    )]
    WindowTargetMismatch {
        /// 当前工厂声明的 Window 稳定标识。
        expected: &'static str,
        /// 当前工厂声明的 Window 路径模式。
        expected_path: &'static str,
        /// 路由实际指向的 Window 稳定标识。
        actual: &'static str,
        /// 路由目标注册时使用的路径模式。
        actual_path: &'static str,
        /// 无法交给当前工厂创建的规范化具体路径。
        path: String,
    },

    /// GPUI 无法使用当前平台选项创建原生窗口。
    #[error("无法创建 Window `{id}`：{message}")]
    OpenWindow {
        /// 创建失败的 Window 稳定标识。
        id: &'static str,
        /// GPUI 或平台返回的具体错误文本。
        message: String,
    },
}

#[derive(Default)]
struct WindowRoutes {
    routes: HashMap<EntityId, Box<dyn Any>>,
}

impl Global for WindowRoutes {}

/// 一个由 Nexora 创建并绑定强类型路由的独立 Window 页面实例。
///
/// 应用注册表会把该实例的 [`Self::view`] 包裹进 `gpui_component::Root`；路由上下文和
/// `closing` 生命周期仍由原始具体 Entity 管理。
pub struct WindowInstance {
    route: RouteMatch,
    view: AnyView,
}

impl WindowInstance {
    /// 返回该独立窗口实例绑定的完整路由匹配结果。
    pub const fn route(&self) -> &RouteMatch {
        &self.route
    }

    /// 返回可以嵌入 `gpui_component::Root` 的类型擦除窗口页面句柄。
    pub fn view(&self) -> AnyView {
        self.view.clone()
    }
}

/// 使用 Sidebar 插槽派生宏生成的构造器创建并擦除一个 GPUI Entity。
///
/// Header 与 Footer 共用该工厂逻辑；具体插槽种类由各自的 inventory 注册类型区分。
#[doc(hidden)]
pub fn create_sidebar_slot<T>(
    window: &mut Window,
    cx: &mut App,
    constructor: fn(&mut Window, &mut Context<T>) -> T,
) -> AnyView
where
    T: Render,
{
    cx.new(|entity_cx| constructor(window, entity_cx)).into()
}

/// 使用派生宏生成的具体构造器创建并擦除一个 Window Entity。
#[doc(hidden)]
pub fn create_window<W>(
    route: RouteMatch,
    window: &mut Window,
    cx: &mut App,
    constructor: fn(&mut Window, &mut Context<W>) -> W,
) -> Result<WindowInstance, WindowRuntimeError>
where
    W: WindowElement,
{
    validate_window_target::<W>(&route)?;
    let typed_route = typed_window_route::<W>(&route)?;
    let entity = cx.new(|entity_cx| {
        let entity_id = entity_cx.entity_id();
        entity_cx
            .default_global::<WindowRoutes>()
            .routes
            .insert(entity_id, Box::new(typed_route));
        entity_cx
            .on_release(move |_: &mut W, cx: &mut App| {
                if cx.has_global::<WindowRoutes>() {
                    cx.global_mut::<WindowRoutes>().routes.remove(&entity_id);
                }
            })
            // nexora-lint: allow(nexora::detached_lifecycle) reason=窗口路由清理必须持续到对应 Entity 释放，并由 GPUI Entity 生命周期终止
            .detach();
        entity_cx
            .on_release_in(window, |element, window, cx| element.closing(window, cx))
            // nexora-lint: allow(nexora::detached_lifecycle) reason=Window closing 生命周期必须持续到窗口 Entity 释放，并由 GPUI Entity 生命周期终止
            .detach();

        let mut element = constructor(window, entity_cx);
        element.initialize(window, entity_cx);
        element
    });

    Ok(WindowInstance {
        route,
        view: entity.into(),
    })
}

/// 根据 Window 的强类型路由生成自动开窗所需的原生选项。
#[doc(hidden)]
pub fn window_options<W>(route: &RouteMatch, cx: &App) -> Result<WindowOptions, WindowRuntimeError>
where
    W: WindowElement,
{
    validate_window_target::<W>(route)?;
    let route = typed_window_route::<W>(route)?;
    Ok(W::window_options(&route, cx))
}

fn validate_window_target<W>(route: &RouteMatch) -> Result<(), WindowRuntimeError>
where
    W: WindowDefinition,
{
    let metadata = match route.target() {
        RouteTarget::Window(metadata) => metadata,
        RouteTarget::Feature(_) => {
            return Err(WindowRuntimeError::FeatureTarget {
                path: route.concrete_path().to_owned(),
            });
        }
    };
    if metadata != W::METADATA {
        return Err(WindowRuntimeError::WindowTargetMismatch {
            expected: W::METADATA.id(),
            expected_path: W::METADATA.path(),
            actual: metadata.id(),
            actual_path: metadata.path(),
            path: route.concrete_path().to_owned(),
        });
    }
    Ok(())
}

fn typed_window_route<W>(
    route: &RouteMatch,
) -> Result<WindowRoute<W::Path, W::Query>, WindowRuntimeError>
where
    W: WindowDefinition,
{
    Ok(WindowRoute {
        concrete_path: route.concrete_path().to_owned(),
        path: route.path()?,
        query: route.query()?,
    })
}

fn window_route<W>(entity_id: EntityId, cx: &App) -> WindowRoute<W::Path, W::Query>
where
    W: WindowDefinition,
{
    cx.global::<WindowRoutes>()
        .routes
        .get(&entity_id)
        .and_then(|route| route.downcast_ref::<WindowRoute<W::Path, W::Query>>())
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "Window `{}` 的 Entity 没有匹配的强类型路由上下文",
                W::METADATA.id()
            )
        })
}

#[derive(Default)]
struct FeatureRoutes {
    routes: HashMap<EntityId, Box<dyn Any>>,
}

impl Global for FeatureRoutes {}

#[derive(Clone, Copy)]
struct FeatureVTable {
    activate: fn(&AnyView, &mut Window, &mut App),
    deactivate: fn(&AnyView, &mut Window, &mut App),
    change_route: fn(
        &AnyView,
        &RouteMatch,
        &RouteMatch,
        &mut Window,
        &mut App,
    ) -> Result<(), FeatureRuntimeError>,
    close: fn(&AnyView, &mut Window, &mut App),
}

/// 一个由 Nexora 创建并完成类型擦除的 Feature 页面实例。
///
/// 应用壳按规范化具体路径缓存该值，并直接把 [`Self::view`] 返回的 `AnyView` 放入内容区。
/// 生命周期分发仍会下沉到原始 `Entity<F>`，不会丢失具体 Feature 的 `Context<F>`。
pub struct FeatureInstance {
    route: RouteMatch,
    view: AnyView,
    vtable: FeatureVTable,
    active: bool,
    closed: bool,
}

impl FeatureInstance {
    /// 返回该实例当前绑定的路由匹配结果。
    pub const fn route(&self) -> &RouteMatch {
        &self.route
    }

    /// 返回可直接嵌入 GPUI Element 树的类型擦除页面句柄。
    pub fn view(&self) -> AnyView {
        self.view.clone()
    }

    /// 激活尚未处于活动状态的页面实例。
    pub fn activate(&mut self, window: &mut Window, cx: &mut App) {
        if self.active || self.closed {
            return;
        }
        (self.vtable.activate)(&self.view, window, cx);
        self.active = true;
    }

    /// 暂停当前活动页面，但保留 Entity 及其内部状态。
    pub fn deactivate(&mut self, window: &mut Window, cx: &mut App) {
        if !self.active || self.closed {
            return;
        }
        (self.vtable.deactivate)(&self.view, window, cx);
        self.active = false;
    }

    /// 使用同一实例键的新路由更新强类型上下文并触发 `route_changed`。
    ///
    /// # Errors
    ///
    /// 实例已经关闭、新路由属于其他 Feature、具体路径发生变化，或者新参数无法
    /// 反序列化时返回错误。传入与当前值完全相同的路由不会重复触发 `route_changed`。
    pub fn update_route(
        &mut self,
        route: RouteMatch,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<(), FeatureRuntimeError> {
        if self.closed {
            return Err(FeatureRuntimeError::ClosedInstance {
                path: self.route.concrete_path().to_owned(),
            });
        }

        if self.route.target() != route.target()
            || self.route.concrete_path() != route.concrete_path()
        {
            return Err(FeatureRuntimeError::RouteInstanceMismatch {
                expected: self.route.concrete_path().to_owned(),
                actual: route.concrete_path().to_owned(),
            });
        }

        if self.route == route {
            return Ok(());
        }

        (self.vtable.change_route)(&self.view, &self.route, &route, window, cx)?;
        self.route = route;
        Ok(())
    }

    /// 依次停用并关闭页面实例；重复调用不会再次触发生命周期。
    pub fn close(&mut self, window: &mut Window, cx: &mut App) {
        if self.closed {
            return;
        }
        self.deactivate(window, cx);
        (self.vtable.close)(&self.view, window, cx);
        self.closed = true;
    }
}

/// 使用派生宏生成的具体构造器创建并擦除一个 Feature Entity。
#[doc(hidden)]
pub fn create_feature<F>(
    route: RouteMatch,
    window: &mut Window,
    cx: &mut App,
    constructor: fn(&mut Window, &mut Context<F>) -> F,
) -> Result<FeatureInstance, FeatureRuntimeError>
where
    F: FeatureElement,
{
    let metadata = match route.target() {
        RouteTarget::Feature(metadata) => metadata,
        RouteTarget::Window(_) => {
            return Err(FeatureRuntimeError::WindowTarget {
                path: route.concrete_path().to_owned(),
            });
        }
    };
    if metadata != F::METADATA {
        return Err(FeatureRuntimeError::FeatureTargetMismatch {
            expected: F::METADATA.id(),
            expected_path: F::METADATA.path(),
            actual: metadata.id(),
            actual_path: metadata.path(),
            path: route.concrete_path().to_owned(),
        });
    }

    let typed_route = typed_route::<F>(&route)?;
    let entity = cx.new(|entity_cx| {
        let entity_id = entity_cx.entity_id();
        entity_cx
            .default_global::<FeatureRoutes>()
            .routes
            .insert(entity_id, Box::new(typed_route));
        entity_cx
            .on_release(move |_, cx| {
                if cx.has_global::<FeatureRoutes>() {
                    cx.global_mut::<FeatureRoutes>().routes.remove(&entity_id);
                }
            })
            // nexora-lint: allow(nexora::detached_lifecycle) reason=路由清理回调必须持续存活到对应 Entity 释放，且由 GPUI 的 Entity 生命周期负责终止
            .detach();

        let mut feature = constructor(window, entity_cx);
        feature.initialize(window, entity_cx);
        feature
    });

    Ok(FeatureInstance {
        route,
        view: entity.into(),
        vtable: FeatureVTable {
            activate: activate_feature::<F>,
            deactivate: deactivate_feature::<F>,
            change_route: change_feature_route::<F>,
            close: close_feature::<F>,
        },
        active: false,
        closed: false,
    })
}

fn typed_route<F>(
    route: &RouteMatch,
) -> Result<FeatureRoute<F::Path, F::Query>, FeatureRuntimeError>
where
    F: Feature,
{
    Ok(FeatureRoute {
        concrete_path: route.concrete_path().to_owned(),
        path: route.path()?,
        query: route.query()?,
    })
}

fn feature_route<F>(entity_id: EntityId, cx: &App) -> FeatureRoute<F::Path, F::Query>
where
    F: Feature,
{
    cx.global::<FeatureRoutes>()
        .routes
        .get(&entity_id)
        .and_then(|route| route.downcast_ref::<FeatureRoute<F::Path, F::Query>>())
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "Feature `{}` 的 Entity 没有匹配的强类型路由上下文",
                F::METADATA.id()
            )
        })
}

fn feature_entity<F>(view: &AnyView) -> gpui::Entity<F>
where
    F: gpui::Render,
{
    view.clone()
        .downcast::<F>()
        .unwrap_or_else(|_| panic!("Nexora Feature 运行时保存了错误的 Entity 类型"))
}

fn activate_feature<F>(view: &AnyView, window: &mut Window, cx: &mut App)
where
    F: FeatureElement,
{
    feature_entity::<F>(view).update(cx, |feature, cx| feature.activated(window, cx));
}

fn deactivate_feature<F>(view: &AnyView, window: &mut Window, cx: &mut App)
where
    F: FeatureElement,
{
    feature_entity::<F>(view).update(cx, |feature, cx| feature.deactivated(window, cx));
}

fn change_feature_route<F>(
    view: &AnyView,
    previous: &RouteMatch,
    current: &RouteMatch,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), FeatureRuntimeError>
where
    F: FeatureElement,
{
    let previous = typed_route::<F>(previous)?;
    let current = typed_route::<F>(current)?;
    let entity = feature_entity::<F>(view);
    cx.global_mut::<FeatureRoutes>()
        .routes
        .insert(entity.entity_id(), Box::new(current));
    entity.update(cx, |feature, cx| {
        feature.route_changed(&previous, window, cx);
        cx.notify();
    });
    Ok(())
}

fn close_feature<F>(view: &AnyView, window: &mut Window, cx: &mut App)
where
    F: FeatureElement,
{
    feature_entity::<F>(view).update(cx, |feature, cx| feature.closing(window, cx));
}
