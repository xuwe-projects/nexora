//! Console 桌面应用的根视图。
//!
//! 该模块定义主窗口中最外层的业务视图，运行器会将其嵌入 `gpui_component::Root`。

use std::collections::HashMap;

use crate::{
    auth, config,
    features::{
        FeatureId, FeatureItem, FeatureLocation, feature_catalog, feature_registry,
        login::LoginFeature,
        roles::RolesFeature,
        users::{UsersFeature, UsersFeatureEvent},
    },
};
use actions::{
    account::{self as account_actions, AccountActionKind, SignOutAccount},
    settings::OpenSettings,
};
use gpui::{
    Anchor, AnyElement, Context, IntoElement, MouseButton, Render, ScrollHandle, Subscription,
    WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
    avatar::Avatar,
    breadcrumb::{Breadcrumb, BreadcrumbItem},
    button::{Button, ButtonVariants as _, Toggle},
    h_flex,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarHeader, SidebarMenu, SidebarMenuItem,
    },
    tab::{Tab, TabBar},
};
use nexora::{
    AppRegistry, FeatureInstance, FeatureMetadata, FeatureRuntimeError, NavigationGroupMetadata,
    RouteMatch, RouteTarget, RouteTargetKind,
};
use ui::{PanelHeader, layout::WorkspaceLayout};

/// 控制台主窗口的业务根视图。
///
/// 该视图持有当前功能区、标签页和导航历史等控制台业务状态，并向共享 `WorkspaceLayout`
/// 提供侧边导航、标题栏内容和当前 feature 面板；窗口级结构与平台适配由共享布局统一处理。
pub struct RootView {
    /// 当前应用生成并完成校验的统一路由注册表。
    registry: AppRegistry,
    /// 当前在主内容区展示的功能区。
    active_feature: FeatureLocation,
    /// 顶部标签栏中已经打开过的功能区。
    opened_tabs: Vec<FeatureLocation>,
    /// 顶部标签栏中被置顶的功能区。
    pinned_tabs: Vec<FeatureLocation>,
    /// 最近一次右键点击的标签页，用于构建标签页上下文菜单。
    tab_context_feature: Option<FeatureLocation>,
    /// 置顶标签区域的横向滚动句柄，用于从更多菜单选择置顶标签时自动滚动到目标标签。
    pinned_tab_scroll_handle: ScrollHandle,
    /// 普通标签区域的横向滚动句柄，用于在从更多菜单选择标签时自动滚动到目标标签。
    regular_tab_scroll_handle: ScrollHandle,
    /// 当前窗口访问过的功能区历史，用于支持顶部栏前进和后退。
    navigation_history: Vec<FeatureLocation>,
    /// 当前所在的历史游标位置，指向 `navigation_history` 中正在展示的功能区。
    navigation_history_index: usize,
    /// 已经打开过且完成 GPUI 初始化的 Feature 实例，以规范化具体路径作为唯一实例键。
    feature_instances: HashMap<String, FeatureInstance>,
    auth_identity: Option<String>,
    _users_subscription: Option<Subscription>,
    _auth_subscription: Option<Subscription>,
    _release_subscription: Option<Subscription>,
}

#[derive(Clone, Copy)]
enum NavigationEntry {
    Group(NavigationGroupMetadata),
    Feature(FeatureMetadata),
}

impl NavigationEntry {
    fn sort_key(&self) -> (i32, u8, &'static str) {
        match *self {
            Self::Group(group) => (group.order(), 0, group.id()),
            Self::Feature(feature) => (feature.order(), 1, feature.id()),
        }
    }
}

/// Console 将一个已解析路径打开为标签或独立窗口时可能发生的错误。
#[derive(Debug, thiserror::Error)]
pub enum OpenPathError {
    /// 路径本身无效，或者注册表中没有匹配的 Feature/Window。
    #[error(transparent)]
    Resolve {
        /// Nexora 注册表返回的原始路径解析错误。
        #[from]
        source: nexora::ResolveError,
    },
    /// Feature 路由已经匹配，但强类型参数或页面工厂无法创建运行时实例。
    #[error(transparent)]
    FeatureRuntime {
        /// Nexora Feature 运行时返回的具体创建或更新错误。
        #[from]
        source: FeatureRuntimeError,
    },
    /// 注册表包含 Window 元数据，但 Console 尚未绑定对应的窗口打开器。
    #[error("Window `{id}` 尚未绑定应用级打开器")]
    UnsupportedWindow {
        /// 尚未绑定的 Window 稳定标识。
        id: &'static str,
    },
}

impl Default for RootView {
    /// 创建处于首页初始状态的控制台根视图。
    ///
    /// 该实现委托给 `RootView::new`，确保默认标签页和导航历史都包含首页。
    fn default() -> Self {
        Self::new()
    }
}

impl RootView {
    /// 创建一个新的根视图。
    ///
    /// 默认会选中首页功能区，后续用户可以通过侧边栏导航切换到其他 feature。
    pub fn new() -> Self {
        let registry = feature_registry();
        let home = location_for_feature(FeatureId::default(), &registry);

        Self {
            registry,
            active_feature: home.clone(),
            opened_tabs: vec![home.clone()],
            pinned_tabs: Vec::new(),
            tab_context_feature: None,
            pinned_tab_scroll_handle: ScrollHandle::new(),
            regular_tab_scroll_handle: ScrollHandle::new(),
            navigation_history: vec![home],
            navigation_history_index: 0,
            feature_instances: HashMap::new(),
            auth_identity: None,
            _users_subscription: None,
            _auth_subscription: None,
            _release_subscription: None,
        }
    }

    /// 创建一个恢复了指定置顶标签的根视图。
    ///
    /// 该构造器用于应用启动时把用户配置中的置顶标签重新放回标签栏左侧；首页仍保持为默认
    /// 激活页面，避免恢复置顶状态时强行切走用户的启动落点。
    pub fn with_pinned_tabs(pinned_tabs: Vec<FeatureId>) -> Self {
        let mut view = Self::new();
        view.pinned_tabs = pinned_tabs
            .into_iter()
            .filter_map(|feature| FeatureLocation::for_feature(feature, &view.registry))
            .fold(Vec::new(), |mut tabs, feature| {
                if !tabs.contains(&feature) {
                    tabs.push(feature);
                }
                tabs
            });

        for feature in view.pinned_tabs.iter().cloned() {
            if !view.opened_tabs.contains(&feature) {
                view.opened_tabs.push(feature);
            }
        }

        view.reorder_tabs_by_pin();
        view
    }

    /// 创建一个恢复了指定具体路径置顶标签的根视图。
    ///
    /// 无法由当前注册表解析的过期路径会被忽略；动态路径中的参数会保留，因此两个同属一个
    /// feature、但具体路径不同的标签可以分别恢复。
    pub fn with_pinned_paths(pinned_tab_paths: Vec<String>) -> Self {
        let mut view = Self::new();
        view.pinned_tabs = pinned_tab_paths
            .into_iter()
            .filter_map(|path| FeatureLocation::resolve(path.as_str(), &view.registry).ok())
            .fold(Vec::new(), |mut tabs, location| {
                if !tabs.contains(&location) {
                    tabs.push(location);
                }
                tabs
            });

        for location in view.pinned_tabs.iter().cloned() {
            if !view.opened_tabs.contains(&location) {
                view.opened_tabs.push(location);
            }
        }

        view.reorder_tabs_by_pin();
        view
    }

    /// 初始化需要窗口上下文的 feature 状态。
    ///
    /// `DataTable` 的状态属于对应 feature 的生命周期，但必须在根视图构造阶段创建；这样
    /// 后续 `render` 只读取既有 Entity，不会引入长期渲染副作用。
    ///
    /// # Panics
    ///
    /// 首页派生注册缺少运行时工厂，或首页的强类型路由声明无法解析默认路径时 panic；这些
    /// 情况属于开发期 Feature 定义错误，应在应用启动阶段立即暴露。
    pub fn initialize_feature_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let home = self.active_feature.clone();
        self.ensure_feature_instance(&home, window, cx)
            .expect("首页 Feature 应当可以由 Nexora 运行时创建");
        self.feature_instances
            .get_mut(home.path())
            .expect("首页 Feature 实例应当已进入运行时缓存")
            .activate(window, cx);
        self.auth_identity = auth::session_identity(cx);
        self._auth_subscription = Some(auth::observe_session_in(window, cx, |this, window, cx| {
            this.handle_auth_session_change(window, cx)
        }));
        self._release_subscription = Some(cx.on_release_in(window, |this, window, cx| {
            this._users_subscription = None;
            this._auth_subscription = None;
            for (_, mut instance) in this.feature_instances.drain() {
                instance.close(window, cx);
            }
        }));
    }

    fn handle_auth_session_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let identity = auth::session_identity(cx);
        if identity == self.auth_identity {
            return;
        }

        self.auth_identity = identity;
        if let Err(error) = self.rebuild_account_feature_instances(window, cx) {
            tracing::error!(error = %error, "认证身份变化后无法重建账户 Feature 实例");
        }
        cx.notify();
    }

    fn rebuild_account_feature_instances(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        let locations = self
            .opened_tabs
            .iter()
            .filter(|location| {
                matches!(location.feature(), FeatureId::Users | FeatureId::Roles)
                    && (self.feature_instances.contains_key(location.path())
                        || location == &&self.active_feature)
            })
            .cloned()
            .collect::<Vec<_>>();

        for location in &locations {
            self.close_feature_instance(location.path(), window, cx);
        }

        for location in &locations {
            self.ensure_feature_instance(location, window, cx)?;
        }

        if self.auth_identity.is_some()
            && matches!(self.active_feature(), FeatureId::Users | FeatureId::Roles)
        {
            let active_path = self.active_path().to_owned();
            if let Some(instance) = self.feature_instances.get_mut(active_path.as_str()) {
                instance.activate(window, cx);
            }
        }

        Ok(())
    }

    fn ensure_feature_instance(
        &mut self,
        location: &FeatureLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if let Some(instance) = self.feature_instances.get_mut(location.path()) {
            if instance.route() != location.route() {
                instance.update_route(location.route().clone(), window, cx)?;
            }
            return Ok(());
        }

        let instance = self
            .registry
            .create_feature(location.route().clone(), window, cx)?;
        if location.feature() == FeatureId::Users {
            self.bind_users_navigation(instance.view(), window, cx);
        }
        self.feature_instances
            .insert(location.path().to_owned(), instance);
        Ok(())
    }

    fn bind_users_navigation(
        &mut self,
        view: gpui::AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(users) = view.downcast::<UsersFeature>() else {
            tracing::error!("用户 Feature 注册的运行时 Entity 类型不正确");
            return;
        };

        self._users_subscription = Some(cx.subscribe_in(
            &users,
            window,
            |this, _, event: &UsersFeatureEvent, window, cx| match event {
                UsersFeatureEvent::OpenDetails { user_id } => {
                    let Ok(mut url) = url::Url::parse("nexora:///users/details") else {
                        return;
                    };
                    let Ok(mut segments) = url.path_segments_mut() else {
                        return;
                    };
                    segments.push(user_id);
                    drop(segments);
                    if let Err(error) = this.open_path_in(url.path(), window, cx) {
                        tracing::error!(error = %error, "无法打开用户详情动态 Feature");
                    }
                }
            },
        ));
    }

    fn close_feature_instance(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut instance) = self.feature_instances.remove(path) else {
            return;
        };
        let users = instance.route().target().id() == FeatureId::Users.id();
        instance.close(window, cx);
        if users {
            self._users_subscription = None;
        }
    }

    fn close_orphaned_feature_instances(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let removed_paths = self
            .feature_instances
            .keys()
            .filter(|path| {
                !self
                    .opened_tabs
                    .iter()
                    .any(|location| location.path() == path.as_str())
            })
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
        let active = self.active_feature.clone();
        self.ensure_feature_instance(&active, window, cx)?;
        self.close_orphaned_feature_instances(window, cx);

        if previous_active_path != active.path()
            && let Some(previous) = self.feature_instances.get_mut(previous_active_path)
        {
            previous.deactivate(window, cx);
        }
        self.feature_instances
            .get_mut(active.path())
            .expect("当前 Feature 应当已进入运行时缓存")
            .activate(window, cx);
        Ok(())
    }

    fn navigate_to_location_in(
        &mut self,
        location: FeatureLocation,
        record_history: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        self.ensure_feature_instance(&location, window, cx)?;
        let previous_active_path = self.active_path().to_owned();
        self.navigate_to_location(location, record_history);
        self.synchronize_feature_runtime(previous_active_path.as_str(), window, cx)
    }

    fn select_feature_in(
        &mut self,
        feature: FeatureId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if let Some(location) = FeatureLocation::for_feature(feature, &self.registry) {
            self.navigate_to_location_in(location, true, window, cx)?;
        }
        Ok(())
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
        let location = self.navigation_history[target_index].clone();
        self.ensure_feature_instance(&location, window, cx)?;
        self.navigation_history_index = target_index;
        self.navigate_to_location_in(location, false, window, cx)
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
        let location = self.navigation_history[target_index].clone();
        self.ensure_feature_instance(&location, window, cx)?;
        self.navigation_history_index = target_index;
        self.navigate_to_location_in(location, false, window, cx)
    }

    /// 返回当前选中的功能区。
    ///
    /// 该方法主要用于测试和后续 action 处理，避免外部直接访问内部状态字段。
    pub fn active_feature(&self) -> FeatureId {
        self.active_feature.feature()
    }

    /// 返回当前激活标签对应的规范化具体路径。
    ///
    /// 与 [`Self::active_feature`] 不同，该路径会保留动态路由参数，可用于区分同一个 feature
    /// 类型的多个详情标签。
    pub fn active_path(&self) -> &str {
        self.active_feature.path()
    }

    /// 返回当前激活标签的路由匹配结果。
    ///
    /// 调用方应通过 [`RouteMatch::path`] 或 [`RouteMatch::query`] 提取强类型参数。
    pub fn active_route(&self) -> &RouteMatch {
        self.active_feature.route()
    }

    /// 返回顶部标签栏中已经打开的功能区。
    ///
    /// 该列表按用户首次打开页面的顺序保存；重复选择同一个 feature 不会插入重复标签。
    pub fn opened_tabs(&self) -> Vec<FeatureId> {
        self.opened_tabs
            .iter()
            .map(FeatureLocation::feature)
            .collect()
    }

    /// 返回顶部标签栏中所有标签的规范化具体路径。
    ///
    /// 返回顺序与标签栏一致，动态路由参数已经展开到具体路径中。
    pub fn opened_paths(&self) -> Vec<&str> {
        self.opened_tabs.iter().map(FeatureLocation::path).collect()
    }

    /// 返回当前全部置顶标签的具体路径。
    pub fn pinned_paths(&self) -> Vec<&str> {
        self.pinned_tabs.iter().map(FeatureLocation::path).collect()
    }

    /// 返回顶部标签栏中被置顶的功能区。
    ///
    /// 返回顺序就是它们在标签栏左侧展示的顺序；置顶标签会排在普通标签之前。
    pub fn pinned_tabs(&self) -> Vec<FeatureId> {
        self.pinned_tabs
            .iter()
            .map(FeatureLocation::feature)
            .collect()
    }

    /// 返回顶部标签栏中未置顶的普通功能区。
    ///
    /// 普通标签会进入右侧可横向滚动区域；置顶标签则固定展示在左侧，避免被滚动条隐藏。
    pub fn regular_tabs(&self) -> Vec<FeatureId> {
        self.regular_tab_locations()
            .iter()
            .map(FeatureLocation::feature)
            .collect()
    }

    /// 返回当前窗口的功能区访问历史。
    ///
    /// 访问历史用于驱动顶部栏前进和后退按钮；同一个连续功能区不会重复写入历史。
    pub fn navigation_history(&self) -> Vec<FeatureId> {
        self.navigation_history
            .iter()
            .map(FeatureLocation::feature)
            .collect()
    }

    /// 返回浏览式导航历史中的具体路径。
    pub fn navigation_history_paths(&self) -> Vec<&str> {
        self.navigation_history
            .iter()
            .map(FeatureLocation::path)
            .collect()
    }

    /// 判断顶部栏后退按钮当前是否可用。
    ///
    /// 返回 `true` 表示历史游标左侧还有更早访问过的功能区，可以调用 `navigate_back` 返回。
    pub fn can_navigate_back(&self) -> bool {
        self.navigation_history_index > 0
    }

    /// 判断顶部栏前进按钮当前是否可用。
    ///
    /// 返回 `true` 表示用户刚执行过后退，并且历史游标右侧还有可恢复的功能区。
    pub fn can_navigate_forward(&self) -> bool {
        self.navigation_history_index + 1 < self.navigation_history.len()
    }

    /// 判断指定功能区对应的标签页是否已经置顶。
    ///
    /// 置顶状态只影响标签栏排序和批量关闭行为，不改变 feature 自身的业务状态。
    pub fn is_tab_pinned(&self, feature: FeatureId) -> bool {
        self.pinned_tabs
            .iter()
            .any(|location| location.feature() == feature)
    }

    /// 切换当前选中的功能区。
    ///
    /// RootView 只保存导航状态，各个 feature 的业务状态仍应由对应模块自行管理。
    pub fn select_feature(&mut self, feature: FeatureId) {
        if let Some(location) = FeatureLocation::for_feature(feature, &self.registry) {
            self.navigate_to_location(location, true);
        }
    }

    /// 根据具体路径打开并选择一个功能标签。
    ///
    /// 路径会通过当前应用注册表解析；动态占位参数会保存在标签位置中，并参与标签去重和
    /// 导航历史记录。
    ///
    /// # Errors
    ///
    /// 路径没有匹配任何已注册 feature，或路径参数不符合注册模式时返回解析错误。
    pub fn select_path(&mut self, path: &str) -> Result<(), nexora::ResolveError> {
        let location = FeatureLocation::resolve(path, &self.registry)?;
        self.navigate_to_location(location, true);
        Ok(())
    }

    /// 根据内部路径或 custom scheme URI 打开对应 Feature 或独立 Window。
    ///
    /// Feature 会进入当前主窗口的标签与导航历史；Window 不进入导航或标签，而是打开或
    /// 激活对应的原生窗口。返回值可供调用方判断本次路由最终采用了哪种展示方式。
    ///
    /// # Errors
    ///
    /// 路径无法解析、没有注册目标，或者目标 Window 尚未绑定应用级打开器时返回错误。
    pub fn open_path(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Result<RouteTargetKind, OpenPathError> {
        let route = self.registry.resolve(path)?;
        let target = route.target();
        match target {
            RouteTarget::Feature(_) => {
                let location = FeatureLocation::from_route(route)?;
                self.navigate_to_location(location, true);
                cx.notify();
            }
            RouteTarget::Window(metadata) => match metadata.id() {
                "settings" => cx.dispatch_action(&OpenSettings),
                id => return Err(OpenPathError::UnsupportedWindow { id }),
            },
        }

        Ok(target.kind())
    }

    fn open_path_in(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<RouteTargetKind, OpenPathError> {
        let route = self.registry.resolve(path)?;
        let target = route.target();
        match target {
            RouteTarget::Feature(_) => {
                let location = FeatureLocation::from_route(route)?;
                self.navigate_to_location_in(location, true, window, cx)?;
                cx.notify();
            }
            RouteTarget::Window(metadata) => match metadata.id() {
                "settings" => cx.dispatch_action(&OpenSettings),
                id => return Err(OpenPathError::UnsupportedWindow { id }),
            },
        }

        Ok(target.kind())
    }

    /// 按访问历史后退到上一个功能区。
    ///
    /// 后退只移动历史游标，不会追加新的历史记录；如果目标功能区对应标签已被关闭，会自动重新打开。
    pub fn navigate_back(&mut self) {
        if !self.can_navigate_back() {
            return;
        }

        self.navigation_history_index -= 1;
        if let Some(location) = self
            .navigation_history
            .get(self.navigation_history_index)
            .cloned()
        {
            self.navigate_to_location(location, false);
        }
    }

    /// 按访问历史前进到下一个功能区。
    ///
    /// 前进只移动历史游标，不会追加新的历史记录；如果目标功能区对应标签已被关闭，会自动重新打开。
    pub fn navigate_forward(&mut self) {
        if !self.can_navigate_forward() {
            return;
        }

        self.navigation_history_index += 1;
        if let Some(location) = self
            .navigation_history
            .get(self.navigation_history_index)
            .cloned()
        {
            self.navigate_to_location(location, false);
        }
    }

    /// 关闭指定功能区对应的标签页。
    ///
    /// 如果关闭的是当前激活标签，会优先激活原位置右侧的标签；没有右侧标签时回退到左侧标签。
    /// 如果所有标签都被关闭，会重新打开首页，保证应用始终有一个可展示页面。
    pub fn close_tab(&mut self, feature: FeatureId) {
        let Some(location) = self
            .opened_tabs
            .iter()
            .find(|opened| opened.feature() == feature)
            .cloned()
        else {
            return;
        };

        self.close_tab_location(&location);
    }

    /// 关闭指定具体路径对应的标签。
    ///
    /// 路径尚未打开时不执行任何操作。动态 Feature 应优先使用该方法，避免只按类型关闭
    /// 到另一个参数实例。
    pub fn close_path(&mut self, path: &str) {
        let Some(location) = self
            .opened_tabs
            .iter()
            .find(|opened| opened.path() == path)
            .cloned()
        else {
            return;
        };

        self.close_tab_location(&location);
    }

    fn close_tab_location(&mut self, location: &FeatureLocation) {
        let Some(index) = self.tab_index(location) else {
            return;
        };

        let closing_active = &self.active_feature == location;
        self.opened_tabs.remove(index);
        self.pinned_tabs.retain(|pinned| pinned != location);

        if self.opened_tabs.is_empty() {
            self.opened_tabs
                .push(location_for_feature(FeatureId::default(), &self.registry));
        }

        if closing_active {
            let fallback_index = index.min(self.opened_tabs.len().saturating_sub(1));
            if let Some(location) = self.opened_tabs.get(fallback_index).cloned() {
                self.active_feature = location;
            }
        }

        self.ensure_active_tab();
    }

    /// 关闭指定标签页左侧的普通标签页。
    ///
    /// 已置顶标签会被保留，避免批量操作破坏用户显式固定的工作上下文。
    pub fn close_tabs_to_left(&mut self, feature: FeatureId) {
        let Some(location) = self
            .opened_tabs
            .iter()
            .find(|opened| opened.feature() == feature)
            .cloned()
        else {
            return;
        };

        self.close_tabs_to_left_location(&location);
    }

    fn close_tabs_to_left_location(&mut self, location: &FeatureLocation) {
        let Some(index) = self.tab_index(location) else {
            return;
        };

        self.opened_tabs = self
            .opened_tabs
            .iter()
            .enumerate()
            .filter_map(|(tab_index, opened)| {
                (tab_index >= index || opened == location || self.is_location_pinned(opened))
                    .then_some(opened.clone())
            })
            .collect();
        self.ensure_active_or_select(location.clone());
    }

    /// 关闭指定标签页右侧的普通标签页。
    ///
    /// 已置顶标签会被保留，目标标签本身也会始终保留。
    pub fn close_tabs_to_right(&mut self, feature: FeatureId) {
        let Some(location) = self
            .opened_tabs
            .iter()
            .find(|opened| opened.feature() == feature)
            .cloned()
        else {
            return;
        };

        self.close_tabs_to_right_location(&location);
    }

    fn close_tabs_to_right_location(&mut self, location: &FeatureLocation) {
        let Some(index) = self.tab_index(location) else {
            return;
        };

        self.opened_tabs = self
            .opened_tabs
            .iter()
            .enumerate()
            .filter_map(|(tab_index, opened)| {
                (tab_index <= index || opened == location || self.is_location_pinned(opened))
                    .then_some(opened.clone())
            })
            .collect();
        self.ensure_active_or_select(location.clone());
    }

    /// 关闭除指定标签页和置顶标签页之外的其他标签页。
    ///
    /// 当目标标签本身未置顶时，它会保留在置顶标签之后，方便用户继续操作右键选中的页面。
    pub fn close_other_tabs(&mut self, feature: FeatureId) {
        let Some(location) = self
            .opened_tabs
            .iter()
            .find(|opened| opened.feature() == feature)
            .cloned()
        else {
            return;
        };

        self.close_other_tab_locations(&location);
    }

    fn close_other_tab_locations(&mut self, location: &FeatureLocation) {
        self.opened_tabs = self
            .opened_tabs
            .iter()
            .filter(|opened| *opened == location || self.is_location_pinned(opened))
            .cloned()
            .collect();
        self.ensure_active_or_select(location.clone());
        self.reorder_tabs_by_pin();
    }

    fn close_tab_location_in(
        &mut self,
        location: &FeatureLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        let previous_active_path = self.active_path().to_owned();
        self.close_tab_location(location);
        self.synchronize_feature_runtime(previous_active_path.as_str(), window, cx)
    }

    fn close_tabs_to_left_location_in(
        &mut self,
        location: &FeatureLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        let previous_active_path = self.active_path().to_owned();
        self.close_tabs_to_left_location(location);
        self.synchronize_feature_runtime(previous_active_path.as_str(), window, cx)
    }

    fn close_tabs_to_right_location_in(
        &mut self,
        location: &FeatureLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        let previous_active_path = self.active_path().to_owned();
        self.close_tabs_to_right_location(location);
        self.synchronize_feature_runtime(previous_active_path.as_str(), window, cx)
    }

    fn close_other_tab_locations_in(
        &mut self,
        location: &FeatureLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        let previous_active_path = self.active_path().to_owned();
        self.close_other_tab_locations(location);
        self.synchronize_feature_runtime(previous_active_path.as_str(), window, cx)
    }

    /// 切换指定标签页的置顶状态。
    ///
    /// 置顶后标签会移动到标签栏左侧；取消置顶后会回到普通标签区域，但仍保留当前打开状态。
    pub fn toggle_pin_tab(&mut self, feature: FeatureId) {
        let Some(location) = self
            .opened_tabs
            .iter()
            .find(|opened| opened.feature() == feature)
            .cloned()
        else {
            return;
        };

        self.toggle_pin_location(&location);
    }

    /// 切换指定具体路径标签的置顶状态。
    pub fn toggle_pin_path(&mut self, path: &str) {
        let Some(location) = self
            .opened_tabs
            .iter()
            .find(|opened| opened.path() == path)
            .cloned()
        else {
            return;
        };

        self.toggle_pin_location(&location);
    }

    fn toggle_pin_location(&mut self, location: &FeatureLocation) {
        if self.is_location_pinned(location) {
            self.pinned_tabs.retain(|pinned| pinned != location);
        } else {
            self.pinned_tabs.push(location.clone());
        }

        self.reorder_tabs_by_pin();
        self.scroll_tab_into_view(&self.active_feature);
    }

    fn is_location_pinned(&self, location: &FeatureLocation) -> bool {
        self.pinned_tabs.contains(location)
    }

    fn regular_tab_locations(&self) -> Vec<FeatureLocation> {
        self.opened_tabs
            .iter()
            .filter(|location| !self.is_location_pinned(location))
            .cloned()
            .collect()
    }

    fn open_feature_tab(&mut self, location: FeatureLocation) {
        if let Some(index) = self.tab_index(&location) {
            self.opened_tabs[index] = location.clone();
            if let Some(index) = self
                .pinned_tabs
                .iter()
                .position(|pinned| pinned == &location)
            {
                self.pinned_tabs[index] = location;
            }
            return;
        }

        self.opened_tabs.push(location);
        self.reorder_tabs_by_pin();
    }

    fn select_pinned_tab_in(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if let Some(location) = self.pinned_tabs.get(index).cloned() {
            self.navigate_to_location_in(location, true, window, cx)?;
        }
        Ok(())
    }

    fn select_regular_tab_in(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if let Some(location) = self.regular_tab_locations().get(index).cloned() {
            self.navigate_to_location_in(location, true, window, cx)?;
        }
        Ok(())
    }

    fn tab_index(&self, location: &FeatureLocation) -> Option<usize> {
        self.opened_tabs
            .iter()
            .position(|opened| opened == location)
    }

    fn ensure_active_tab(&mut self) {
        if self.opened_tabs.is_empty() {
            self.opened_tabs
                .push(location_for_feature(FeatureId::default(), &self.registry));
        }

        if !self.opened_tabs.contains(&self.active_feature) {
            self.active_feature = self.opened_tabs[0].clone();
        }

        self.pinned_tabs
            .retain(|pinned| self.opened_tabs.contains(pinned));
        self.scroll_tab_into_view(&self.active_feature);
    }

    fn ensure_active_or_select(&mut self, fallback: FeatureLocation) {
        if !self.opened_tabs.contains(&self.active_feature) {
            self.active_feature = fallback;
        }

        self.ensure_active_tab();
    }

    fn reorder_tabs_by_pin(&mut self) {
        let mut pinned = Vec::new();
        for location in self.pinned_tabs.iter().cloned() {
            if self.opened_tabs.contains(&location) && !pinned.contains(&location) {
                pinned.push(location);
            }
        }

        let mut unpinned = self
            .opened_tabs
            .iter()
            .filter(|location| !pinned.contains(location))
            .cloned()
            .collect::<Vec<_>>();

        pinned.append(&mut unpinned);
        self.opened_tabs = pinned;
        self.pinned_tabs
            .retain(|pinned| self.opened_tabs.contains(pinned));
    }

    fn active_pinned_tab_index(&self) -> Option<usize> {
        self.pinned_tab_index(&self.active_feature)
    }

    fn pinned_tab_index(&self, location: &FeatureLocation) -> Option<usize> {
        self.pinned_tabs
            .iter()
            .position(|pinned| pinned == location)
    }

    fn regular_tab_index(&self, location: &FeatureLocation) -> Option<usize> {
        self.opened_tabs
            .iter()
            .filter(|opened| !self.is_location_pinned(opened))
            .position(|opened| opened == location)
    }

    fn active_regular_tab_index(&self) -> Option<usize> {
        self.regular_tab_index(&self.active_feature)
    }

    fn scroll_tab_into_view(&self, location: &FeatureLocation) {
        if let Some(index) = self.pinned_tab_index(location) {
            self.pinned_tab_scroll_handle.scroll_to_item(index);
        } else if let Some(index) = self.regular_tab_index(location) {
            self.regular_tab_scroll_handle.scroll_to_item(index);
        }
    }

    fn persist_pinned_tabs(&self, cx: &mut Context<Self>) {
        let paths = self
            .pinned_tabs
            .iter()
            .map(|location| location.path().to_owned())
            .collect::<Vec<_>>();
        config::persist_pinned_tab_paths(paths.as_slice(), cx);
    }

    fn navigate_to_location(&mut self, location: FeatureLocation, record_history: bool) {
        let same_instance = self.active_feature == location;
        self.open_feature_tab(location.clone());

        if same_instance {
            self.active_feature = location.clone();
            if let Some(current) = self
                .navigation_history
                .get_mut(self.navigation_history_index)
                && *current == location
            {
                *current = location.clone();
            }
            self.scroll_tab_into_view(&location);
            return;
        }

        self.active_feature = location.clone();
        if record_history {
            self.push_navigation_history(location.clone());
        }
        self.scroll_tab_into_view(&location);
    }

    fn push_navigation_history(&mut self, location: FeatureLocation) {
        if self.navigation_history.get(self.navigation_history_index) == Some(&location) {
            return;
        }

        self.navigation_history
            .truncate(self.navigation_history_index + 1);
        self.navigation_history.push(location);
        self.navigation_history_index = self.navigation_history.len().saturating_sub(1);
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let sidebar_border = cx.theme().sidebar_border;
        let sections = self.navigation_sections();
        let mut navigation_menus = Vec::new();
        for (_, items) in sections
            .iter()
            .filter(|(section, _)| *section != "扩展示例")
        {
            let menu = self.render_nav_menu(items, cx);
            let menu = if navigation_menus.is_empty() {
                menu
            } else {
                menu.pt_3().border_t_1().border_color(sidebar_border)
            };
            navigation_menus.push(menu);
        }
        let extension_items = sections
            .iter()
            .find(|(section, _)| *section == "扩展示例")
            .map(|(_, items)| items.as_slice())
            .unwrap_or_default();
        let extension_active = extension_items.iter().copied().any(|item| match item {
            NavigationEntry::Feature(feature) => {
                FeatureId::from_id(feature.id()) == Some(self.active_feature())
            }
            NavigationEntry::Group(group) => self
                .registry
                .navigation_group_ancestors(self.active_feature().id())
                .iter()
                .any(|ancestor| ancestor.id() == group.id()),
        });
        if !extension_items.is_empty() {
            navigation_menus.push(
                SidebarMenu::new()
                    .pt_3()
                    .border_t_1()
                    .border_color(sidebar_border)
                    .child(
                        SidebarMenuItem::new("更多功能")
                            .icon(IconName::Frame)
                            .default_open(extension_active)
                            .click_to_toggle(true)
                            .children(
                                extension_items
                                    .iter()
                                    .copied()
                                    .map(|item| self.render_navigation_entry(item, cx)),
                            ),
                    ),
            );
        }
        let theme = cx.theme();

        Sidebar::new("console-sidebar")
            .size_full()
            .collapsible(SidebarCollapsible::None)
            .header(
                SidebarHeader::new()
                    .p_0()
                    .py_1()
                    .pb_3()
                    .border_b_1()
                    .border_color(sidebar_border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size_7()
                                    .flex_shrink_0()
                                    .rounded_sm()
                                    .bg(theme.tokens.sidebar_primary)
                                    .text_color(theme.sidebar_primary_foreground)
                                    .child(Icon::new(IconName::ChartPie).size_4()),
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
                                            .child("Nexora Console"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.sidebar_foreground.opacity(0.66))
                                            .child("Desktop workspace"),
                                    ),
                            ),
                    ),
            )
            .children(navigation_menus)
            .footer(
                div()
                    .w_full()
                    .pt_3()
                    .border_t_1()
                    .border_color(sidebar_border)
                    .child(self.render_account_footer(cx)),
            )
            .into_any_element()
    }

    fn navigation_sections(&self) -> Vec<(&'static str, Vec<NavigationEntry>)> {
        let mut roots = self
            .registry
            .navigation_groups()
            .iter()
            .copied()
            .filter(|group| group.parent().is_none())
            .map(NavigationEntry::Group)
            .chain(
                self.registry
                    .navigation_features()
                    .filter(|feature| feature.group().is_none())
                    .map(NavigationEntry::Feature),
            )
            .collect::<Vec<_>>();
        roots.sort_by_key(NavigationEntry::sort_key);
        roots.into_iter().fold(Vec::new(), |mut sections, entry| {
            let section = match entry {
                NavigationEntry::Group(group) => group.section(),
                NavigationEntry::Feature(feature) => self.registry.feature_section(feature),
            };
            if let Some((_, items)) = sections
                .iter_mut()
                .find(|(existing, _)| *existing == section)
            {
                items.push(entry);
            } else {
                sections.push((section, vec![entry]));
            }
            sections
        })
    }

    fn render_nav_menu(&self, items: &[NavigationEntry], cx: &mut Context<Self>) -> SidebarMenu {
        SidebarMenu::new().children(
            items
                .iter()
                .copied()
                .map(|item| self.render_navigation_entry(item, cx)),
        )
    }

    fn render_account_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = auth::snapshot(cx);
        let menu_items = if snapshot.authenticated {
            account_actions::menu_actions()
        } else {
            account_actions::signed_out_menu_actions()
        };
        let action_context = cx.focus_handle();
        let display_name = snapshot.display_name.clone();
        let avatar = if let Some(avatar_url) = snapshot.avatar_url.clone() {
            Avatar::new()
                .name(display_name.clone())
                .src(avatar_url)
                .small()
        } else {
            Avatar::new().name(display_name.clone()).small()
        };

        SidebarFooter::new()
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
                    menu.action_context(action_context.clone()).min_w(220.),
                    |menu, item| {
                        let menu_item = gpui_component::menu::PopupMenuItem::new(item.label())
                            .icon(account_icon(item.kind()));
                        let menu_item = match item.kind() {
                            AccountActionKind::SignIn => menu_item.on_click(|_, _, cx| {
                                tracing::debug!("Console OIDC 登录菜单已点击");
                                if let Err(error) = auth::start_login(cx) {
                                    tracing::error!(error = %error, "Console OIDC 无法开始登录");
                                    auth::complete_login(Err(error), cx);
                                }
                            }),
                            AccountActionKind::SignOut => {
                                menu_item.on_click(|_, _, cx| auth::sign_out(cx))
                            }
                            AccountActionKind::Settings => menu_item.action(item.to_action()),
                        };
                        menu.item(menu_item)
                    },
                )
            })
    }

    fn sign_out_account(&mut self, _: &SignOutAccount, _: &mut Window, cx: &mut Context<Self>) {
        auth::sign_out(cx);
        cx.notify();
    }

    fn render_navigation_entry(
        &self,
        entry: NavigationEntry,
        cx: &mut Context<Self>,
    ) -> SidebarMenuItem {
        match entry {
            NavigationEntry::Group(group) => self.render_nav_group(group, cx),
            NavigationEntry::Feature(feature) => self.render_nav_feature(feature, cx),
        }
    }

    fn render_nav_group(
        &self,
        group: NavigationGroupMetadata,
        cx: &mut Context<Self>,
    ) -> SidebarMenuItem {
        let mut children = self
            .registry
            .groups_in_group(group.id())
            .map(NavigationEntry::Group)
            .chain(
                self.registry
                    .features_in_group(group.id())
                    .map(NavigationEntry::Feature),
            )
            .collect::<Vec<_>>();
        children.sort_by_key(NavigationEntry::sort_key);
        let children = children
            .into_iter()
            .map(|entry| self.render_navigation_entry(entry, cx))
            .collect::<Vec<_>>();
        let active_id = self.active_feature().id();
        let default_open = self
            .registry
            .navigation_group_ancestors(active_id)
            .iter()
            .any(|ancestor| ancestor.id() == group.id());

        SidebarMenuItem::new(group.title())
            .icon(feature_icon(group.icon()))
            .default_open(default_open)
            .click_to_toggle(true)
            .children(children)
    }

    fn render_nav_feature(&self, item: FeatureMetadata, cx: &mut Context<Self>) -> SidebarMenuItem {
        let feature = FeatureId::from_id(item.id())
            .expect("Nexora 注册表中的 Feature 必须具有 Console FeatureId");
        let active = self.active_feature() == feature;

        SidebarMenuItem::new(item.title())
            .icon(feature_icon(item.icon()))
            .active(active)
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Err(error) = this.select_feature_in(feature, window, cx) {
                    tracing::error!(error = %error, "无法打开侧边栏 Feature");
                }
                cx.notify();
            }))
    }

    fn render_tab(location: FeatureLocation, is_pinned: bool, root_view: WeakEntity<Self>) -> Tab {
        let action_root = root_view.clone();
        let context_root = root_view.clone();
        let action_location = location.clone();
        let action = if is_pinned {
            Toggle::new(format!("pin-tab-{}", location.path()))
                .xsmall()
                .checked(true)
                .icon(IconName::StarFill)
                .tooltip("取消置顶")
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    _ = action_root.update(cx, |this, cx| {
                        this.toggle_pin_location(&action_location);
                        this.persist_pinned_tabs(cx);
                        cx.notify();
                    });
                })
                .into_any_element()
        } else {
            Button::new(format!("close-tab-{}", location.path()))
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("关闭标签")
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    _ = action_root.update(cx, |this, cx| {
                        if let Err(error) = this.close_tab_location_in(&action_location, window, cx)
                        {
                            tracing::error!(error = %error, "无法关闭 Feature 标签");
                        }
                        this.persist_pinned_tabs(cx);
                        cx.notify();
                    });
                })
                .into_any_element()
        };

        Tab::new()
            .px_1()
            .prefix(feature_icon(location.icon()))
            .label(location.title())
            .suffix(h_flex().gap_1().child(action))
            .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                _ = context_root.update(cx, |this, _| {
                    this.tab_context_feature = Some(location.clone());
                });
            })
    }

    fn render_title_bar_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let pinned_tabs = self.pinned_tabs.clone();
        let regular_tabs = self.regular_tab_locations();
        let active_pinned_tab_index = self.active_pinned_tab_index();
        let active_regular_tab_index = self.active_regular_tab_index();
        let root_view = cx.entity().downgrade();
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
                    .id("console-open-tabs-zone")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .id("console-open-tabs-strip")
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
                                                if let Err(error) =
                                                    this.navigate_back_in(window, cx)
                                                {
                                                    tracing::error!(error = %error, "无法后退到历史 Feature");
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
                                                if let Err(error) =
                                                    this.navigate_forward_in(window, cx)
                                                {
                                                    tracing::error!(error = %error, "无法前进到历史 Feature");
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .when(!pinned_tabs.is_empty(), |this| {
                                this.child(
                                    div()
                                        .id("console-pinned-tabs-zone")
                                        .flex_none()
                                        .max_w(px(220.0))
                                        .min_w_0()
                                        .h_full()
                                        .overflow_hidden()
                                        .child(
                                            TabBar::new("console-pinned-tabs")
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
                                                        if let Err(error) = this
                                                            .select_pinned_tab_in(*index, window, cx)
                                                        {
                                                            tracing::error!(error = %error, "无法打开置顶 Feature 标签");
                                                        }
                                                        cx.notify();
                                                    },
                                                ))
                                                .children(pinned_tabs.iter().cloned().map(
                                                    |location| {
                                                        Self::render_tab(
                                                            location,
                                                            true,
                                                            root_view.clone(),
                                                        )
                                                    },
                                                )),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .id("console-regular-tabs-zone")
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_hidden()
                                    .child(
                                        TabBar::new("console-regular-tabs")
                                            .w_full()
                                            .h_full()
                                            .track_scroll(&self.regular_tab_scroll_handle)
                                            .menu(!regular_tabs.is_empty())
                                            .when_some(active_regular_tab_index, |this, index| {
                                                this.selected_index(index)
                                            })
                                            .on_click(cx.listener(|this, index: &usize, window, cx| {
                                                if let Err(error) = this
                                                    .select_regular_tab_in(*index, window, cx)
                                                {
                                                    tracing::error!(error = %error, "无法打开普通 Feature 标签");
                                                }
                                                cx.notify();
                                            }))
                                            .children(regular_tabs.iter().cloned().map(
                                                |location| {
                                                    Self::render_tab(
                                                        location,
                                                        false,
                                                        root_view.clone(),
                                                    )
                                                },
                                            )),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("console-open-tabs-bottom-mask")
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(1.0))
                            .bg(title_bar_background),
                    )
                    .context_menu({
                        let root_view = root_view.clone();
                        move |menu, _, cx| {
                            let Some(root) = root_view.upgrade() else {
                                return menu;
                            };
                            let Some(location) = root
                                .update(cx, |this, _| this.tab_context_feature.take())
                            else {
                                return menu;
                            };

                            let Some((pinned, can_close_left, can_close_right, can_close_other)) = ({
                                let root = root.read(cx);
                                let Some(index) = root.tab_index(&location) else {
                                    return menu;
                                };
                                let can_close_left = root
                                    .opened_tabs
                                    .iter()
                                    .take(index)
                                    .any(|opened| !root.is_location_pinned(opened));
                                let can_close_right = root
                                    .opened_tabs
                                    .iter()
                                    .skip(index + 1)
                                    .any(|opened| !root.is_location_pinned(opened));
                                let can_close_other = root.opened_tabs.iter().any(|opened| {
                                    opened != &location && !root.is_location_pinned(opened)
                                });

                                Some((
                                    root.is_location_pinned(&location),
                                    can_close_left,
                                    can_close_right,
                                    can_close_other,
                                ))
                            })
                            else {
                                return menu;
                            };

                            menu.min_w(220.)
                                .item(PopupMenuItem::new("关闭").icon(IconName::Close).on_click({
                                    let root_view = root_view.clone();
                                    let location = location.clone();
                                    move |_, window, cx| {
                                        _ = root_view.update(cx, |this, cx| {
                                            if let Err(error) =
                                                this.close_tab_location_in(&location, window, cx)
                                            {
                                                tracing::error!(error = %error, "无法关闭 Feature 标签");
                                            }
                                            this.persist_pinned_tabs(cx);
                                            cx.notify();
                                        });
                                    }
                                }))
                                .separator()
                                .item(
                                    PopupMenuItem::new("关闭左侧标签页")
                                        .icon(IconName::ArrowLeft)
                                        .disabled(!can_close_left)
                                        .on_click({
                                            let root_view = root_view.clone();
                                            let location = location.clone();
                                            move |_, window, cx| {
                                                _ = root_view.update(cx, |this, cx| {
                                                    if let Err(error) = this
                                                        .close_tabs_to_left_location_in(
                                                            &location, window, cx,
                                                        )
                                                    {
                                                        tracing::error!(error = %error, "无法关闭左侧 Feature 标签");
                                                    }
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("关闭右侧标签页")
                                        .icon(IconName::ArrowRight)
                                        .disabled(!can_close_right)
                                        .on_click({
                                            let root_view = root_view.clone();
                                            let location = location.clone();
                                            move |_, window, cx| {
                                                _ = root_view.update(cx, |this, cx| {
                                                    if let Err(error) = this
                                                        .close_tabs_to_right_location_in(
                                                            &location, window, cx,
                                                        )
                                                    {
                                                        tracing::error!(error = %error, "无法关闭右侧 Feature 标签");
                                                    }
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("关闭其他标签页")
                                        .disabled(!can_close_other)
                                        .on_click({
                                            let root_view = root_view.clone();
                                            let location = location.clone();
                                            move |_, window, cx| {
                                                _ = root_view.update(cx, |this, cx| {
                                                    if let Err(error) = this
                                                        .close_other_tab_locations_in(
                                                            &location, window, cx,
                                                        )
                                                    {
                                                        tracing::error!(error = %error, "无法关闭其他 Feature 标签");
                                                    }
                                                    cx.notify();
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
                                        let root_view = root_view.clone();
                                        let location = location.clone();
                                        move |_, _, cx| {
                                            _ = root_view.update(cx, |this, cx| {
                                                this.toggle_pin_location(&location);
                                                this.persist_pinned_tabs(cx);
                                                cx.notify();
                                            });
                                        }
                                    }),
                                )
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

    fn render_panel_header(&self, cx: &mut Context<Self>) -> PanelHeader {
        let active_location = self.active_feature.clone();
        let breadcrumb = feature_breadcrumb_path(&active_location).into_iter().fold(
            Breadcrumb::new(),
            |breadcrumb, (label, target)| {
                let item = match target {
                    Some(target) => BreadcrumbItem::new(label).on_click(cx.listener(
                        move |this, _, window, cx| {
                            if let Err(error) = this.select_feature_in(target, window, cx) {
                                tracing::error!(error = %error, "无法打开面包屑 Feature");
                            }
                            cx.notify();
                        },
                    )),
                    None => BreadcrumbItem::new(label),
                };

                breadcrumb.child(item)
            },
        );
        let pinned = self.is_location_pinned(&active_location);

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
                    this.toggle_pin_location(&active_location);
                    this.persist_pinned_tabs(cx);
                    cx.notify();
                })),
        )
    }

    fn render_active_feature(&self) -> AnyElement {
        self.feature_instances
            .get(self.active_path())
            .map(|instance| instance.view().into_any_element())
            .unwrap_or_else(|| div().child("Feature 页面尚未初始化").into_any_element())
    }

    fn render_active_panel_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.active_feature() != FeatureId::Roles {
            return None;
        }

        self.feature_instances
            .get(self.active_path())?
            .view()
            .downcast::<RolesFeature>()
            .ok()
            .map(|roles| roles.read(cx).panel_dialog())
    }
}

impl Render for RootView {
    /// 将根视图渲染为 GPUI 元素树。
    ///
    /// 渲染时会把控制台专属的导航、标签栏和当前 feature 页面传入共享工作区布局，
    /// 体现多个业务模块共同构成桌面程序、窗口结构由公共 UI crate 复用的职责边界。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if auth::snapshot(cx).authenticated {
            let sidebar = self.render_sidebar(cx);
            let title_bar_content = self.render_title_bar_content(cx);
            let panel_header = self.render_panel_header(cx);
            let content_scrollable = self.active_feature() != FeatureId::VirtualScroll;
            let panel_overlay = self.render_active_panel_overlay(cx);
            let active_feature = self.render_active_feature();

            let layout = WorkspaceLayout::new(sidebar, title_bar_content, active_feature)
                .with_sidebar_width(px(224.0))
                .with_sidebar_width_range(px(208.0)..px(300.0))
                .with_panel_header(panel_header)
                .with_content_scrollable(content_scrollable);
            let layout = if let Some(panel_overlay) = panel_overlay {
                layout.with_panel_overlay(panel_overlay)
            } else {
                layout
            };

            layout.render(window, cx)
        } else {
            LoginFeature::render(window, cx)
        };
        let window_layers = ui::window_layers(window, cx);

        div()
            .relative()
            .key_context(account_actions::CONTEXT)
            .on_action(cx.listener(Self::sign_out_account))
            .size_full()
            .child(content)
            .children(window_layers)
            .into_any_element()
    }
}

fn feature_breadcrumb_path(location: &FeatureLocation) -> Vec<(String, Option<FeatureId>)> {
    let feature = location.feature();
    let Some(item) = feature_catalog()
        .iter()
        .copied()
        .find(|item| item.contains(feature))
    else {
        return vec![(location.title(), None)];
    };
    let section_target = feature_catalog()
        .iter()
        .copied()
        .find(|candidate| candidate.section() == item.section())
        .map(FeatureItem::id);
    let mut path = vec![(item.section().to_owned(), section_target)];

    if item.id() != feature {
        path.push((item.id().title().to_owned(), Some(item.id())));
    }
    path.push((location.title(), None));
    path
}

fn location_for_feature(feature: FeatureId, registry: &AppRegistry) -> FeatureLocation {
    FeatureLocation::for_feature(feature, registry)
        .expect("静态功能区必须在 Nexora 注册表中拥有无参数路径")
}

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
