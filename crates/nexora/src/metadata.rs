//! Feature 与独立窗口的静态元数据契约。

/// 可以在主窗口标签系统中打开的业务功能。
///
/// `#[derive(nexora::Feature)]` 会根据 `#[nexora(...)]` 属性生成该 trait、把页面工厂
/// 提交到自动注册表，并生成转发到 [`crate::FeatureElement::render`] 的 GPUI `Render`
/// 实现。动态路径和查询字符串分别通过 [`Self::Path`]、[`Self::Query`] 声明业务结构，
/// 实例只会在两类参数都完成反序列化后创建。
pub trait Feature: 'static {
    /// 当前 Feature 的动态路径参数类型；没有动态参数时由派生宏设为 [`crate::NoPath`]。
    type Path: serde::de::DeserializeOwned + Clone + 'static;

    /// 当前 Feature 的查询参数类型；没有查询结构时由派生宏设为 [`crate::NoQuery`]。
    type Query: serde::de::DeserializeOwned + Clone + 'static;

    /// 当前功能的稳定静态元数据。
    const METADATA: FeatureMetadata;

    /// 当前 Feature 的类型擦除运行时注册信息。
    ///
    /// 派生宏会覆盖默认值并写入页面工厂；手写元数据实现仍可以保持 `None`，用于只需要
    /// 路由校验、不需要自动创建 GPUI Entity 的测试或工具。
    #[doc(hidden)]
    const REGISTRATION: Option<crate::__private::FeatureRegistration> = None;
}

/// 业务 Feature 的稳定描述。
///
/// 描述会同时用于路径匹配、导航生成、标签标题和父子目录组织。`path` 是应用内部逻辑
/// 路径，不包含 custom scheme、查询参数或片段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FeatureMetadata {
    id: &'static str,
    title: &'static str,
    path: &'static str,
    section: Option<&'static str>,
    icon: Option<&'static str>,
    parent: Option<&'static str>,
    order: i32,
    navigation: bool,
    content_scrollable: bool,
}

impl FeatureMetadata {
    /// 创建一份 Feature 静态描述。
    ///
    /// 该构造函数主要供 Nexora 派生宏和生成代码使用。路径合法性、动态参数与导航策略
    /// 以及跨 Feature/Window 的冲突会在 [`crate::AppRegistryBuilder::build`] 时统一校验。
    #[allow(
        clippy::too_many_arguments,
        reason = "派生宏需要以 const 形式一次写入完整 Feature 元数据"
    )]
    pub const fn new(
        id: &'static str,
        title: &'static str,
        path: &'static str,
        section: Option<&'static str>,
        icon: Option<&'static str>,
        parent: Option<&'static str>,
        order: i32,
        navigation: bool,
    ) -> Self {
        Self {
            id,
            title,
            path,
            section,
            icon,
            parent,
            order,
            navigation,
            content_scrollable: true,
        }
    }

    /// 返回不会随展示文案变化的 Feature 标识。
    pub const fn id(self) -> &'static str {
        self.id
    }

    /// 返回导航和标签中使用的展示标题。
    pub const fn title(self) -> &'static str {
        self.title
    }

    /// 返回用于内部导航和 deeplink 匹配的路径模式。
    pub const fn path(self) -> &'static str {
        self.path
    }

    /// 返回侧边栏中的可选分组名称。
    pub const fn section(self) -> Option<&'static str> {
        self.section
    }

    /// 返回由具体 UI 层解释的可选图标标识。
    pub const fn icon(self) -> Option<&'static str> {
        self.icon
    }

    /// 返回父 Feature 的稳定标识，用于生成二级导航。
    pub const fn parent(self) -> Option<&'static str> {
        self.parent
    }

    /// 返回同级导航项的稳定排序值。
    pub const fn order(self) -> i32 {
        self.order
    }

    /// 返回当前 Feature 是否应该出现在主导航中。
    ///
    /// `false` 只隐藏导航入口，不影响通过路径打开标签、写入历史或参与 deeplink。
    pub const fn navigation(self) -> bool {
        self.navigation
    }

    /// 设置应用 Shell 是否应为该 Feature 提供外层内容滚动。
    ///
    /// 普通页面保留构造时的默认值 `true` 即可。虚拟列表、编辑器或其他自行管理滚动视口的
    /// 页面应传入 `false`，避免外层与内部组件形成嵌套滚动区域。
    #[must_use]
    pub const fn with_content_scrollable(mut self, content_scrollable: bool) -> Self {
        self.content_scrollable = content_scrollable;
        self
    }

    /// 返回应用 Shell 是否应为当前 Feature 提供外层内容滚动。
    ///
    /// 默认值为 `true`，适合由 Shell 统一承载滚动的普通页面。虚拟列表、编辑器或其他自行
    /// 管理滚动视口的页面应通过 `#[nexora(content_scrollable = false)]` 关闭外层滚动，避免
    /// 形成嵌套滚动区域并破坏内部组件的尺寸计算与滚动交互。
    pub const fn content_scrollable(self) -> bool {
        self.content_scrollable
    }
}

/// 通过独立原生窗口打开的业务界面。
///
/// Window 永远不会进入主侧边栏或标签栏。启用 `desktop` Cargo feature 后，
/// `#[derive(nexora::Window)]` 会把路径、查询类型和窗口工厂一并提交给运行时，并生成
/// 转发到 [`crate::WindowElement::render`] 的 GPUI `Render` 实现。
pub trait Window: 'static {
    /// 当前 Window 的动态路径参数类型；没有动态参数时由派生宏设为 [`crate::NoPath`]。
    type Path: serde::de::DeserializeOwned + Clone + 'static;

    /// 当前 Window 的查询参数类型；没有查询结构时由派生宏设为 [`crate::NoQuery`]。
    type Query: serde::de::DeserializeOwned + Clone + 'static;

    /// 当前独立窗口的稳定静态元数据。
    const METADATA: WindowMetadata;

    /// 当前 Window 的类型擦除运行时注册信息。
    ///
    /// 派生宏会在启用桌面能力时写入窗口工厂；手写元数据实现可以保留默认值，继续只参与
    /// 路由校验和工具查询。
    #[doc(hidden)]
    const REGISTRATION: Option<crate::__private::WindowRegistration> = None;
}

/// Account 桌面客户端未认证时使用的应用级登录页面。
///
/// `#[derive(nexora::LoginFeature)]` 会为直接实现 [`gpui::Render`] 的具体类型生成本标记
/// 实现并提交用户覆盖注册。每个最终应用最多只能定义一个覆盖项；没有定义时框架使用
/// `desktop` 自带的默认登录页面。Login Feature 不属于路径路由、主导航或标签页。
#[cfg(feature = "desktop")]
pub trait LoginFeature: gpui::Render + 'static {
    /// 派生宏生成的类型擦除登录页面注册记录。
    #[doc(hidden)]
    const REGISTRATION: crate::__private::LoginFeatureRegistration;
}

/// 应用级设置窗口的单例标记契约。
///
/// `#[derive(nexora::SettingsWindow)]` 会固定使用 `settings` 标识与 `/settings` 路径，
/// 并生成 [`Window`]、GPUI [`gpui::Render`] 和本标记实现。应用继续通过
/// [`crate::WindowElement`] 定义窗口内容、初始化生命周期与原生窗口选项。每个最终应用
/// 最多只能定义一个覆盖项；没有定义时框架使用桌面能力自带的默认设置窗口。
pub trait SettingsWindow: Window {
    /// 派生宏生成的类型擦除设置窗口注册记录。
    #[doc(hidden)]
    const REGISTRATION: crate::__private::SettingsWindowRegistration;
}

/// 主窗口 Sidebar 顶部自定义内容的标记契约。
///
/// 实现类型直接使用 GPUI [`gpui::Render`] 描述自己的状态与界面；
/// `#[derive(nexora::SidebarHeader)]` 只负责生成本标记实现和自动注册工厂，不会代理或
/// 改写用户的 `Render` 实现。
pub trait SidebarHeader: gpui::Render + 'static {
    /// 派生宏生成的类型擦除 Header 注册记录。
    #[doc(hidden)]
    const REGISTRATION: crate::__private::SidebarHeaderRegistration;
}

/// 主窗口 Sidebar 底部自定义内容的标记契约。
///
/// 实现类型直接使用 GPUI [`gpui::Render`] 描述自己的状态与界面；
/// `#[derive(nexora::SidebarFooter)]` 只负责生成本标记实现和自动注册工厂，不会代理或
/// 改写用户的 `Render` 实现。
pub trait SidebarFooter: gpui::Render + 'static {
    /// 派生宏生成的类型擦除 Footer 注册记录。
    #[doc(hidden)]
    const REGISTRATION: crate::__private::SidebarFooterRegistration;
}

/// 独立窗口的稳定描述。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowMetadata {
    id: &'static str,
    title: &'static str,
    path: &'static str,
    icon: Option<&'static str>,
    order: i32,
}

impl WindowMetadata {
    /// 创建一份独立窗口静态描述。
    ///
    /// 该构造函数主要供 Nexora 派生宏和生成代码使用；路径和冲突仍由统一应用注册表校验。
    pub const fn new(
        id: &'static str,
        title: &'static str,
        path: &'static str,
        icon: Option<&'static str>,
        order: i32,
    ) -> Self {
        Self {
            id,
            title,
            path,
            icon,
            order,
        }
    }

    /// 返回不会随窗口标题变化的稳定标识。
    pub const fn id(self) -> &'static str {
        self.id
    }

    /// 返回窗口标题。
    pub const fn title(self) -> &'static str {
        self.title
    }

    /// 返回用于代码调用和 deeplink 匹配的路径模式。
    pub const fn path(self) -> &'static str {
        self.path
    }

    /// 返回由具体 UI 层解释的可选图标标识。
    pub const fn icon(self) -> Option<&'static str> {
        self.icon
    }

    /// 返回窗口定义的稳定排序值。
    pub const fn order(self) -> i32 {
        self.order
    }
}
