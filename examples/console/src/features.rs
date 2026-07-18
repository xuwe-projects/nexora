//! Console 桌面应用的功能模块集合。
//!
//! 每个公开子模块对应一个可独立维护的界面或功能区域。

use gpui::prelude::*;
use gpui_component::{ActiveTheme as _, StyledExt as _};
use nexora::{AppRegistry, Path, ResolveError, RouteMatch, RouteTarget};

/// 控制台应用中的功能区标识。
///
/// Nexora 根据派生元数据自动发现、创建和渲染 Feature；该枚举只保留
/// Console 壳层需要的稳定标识映射，例如偏好兼容、面包屑和少量布局特例。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FeatureId {
    /// 控制台首页，用于展示应用概览和常用入口。
    #[default]
    Home,
    /// 项目功能区，用于展示本地项目或工作区列表。
    Projects,
    /// 项目模板子功能区，用于展示可复用的桌面应用模板。
    ProjectTemplates,
    /// 项目运行环境子功能区，用于展示本地工具链和环境状态。
    ProjectEnvironments,
    /// 任务功能区，用于展示构建、打包、发布等后台任务。
    Tasks,
    /// 用户管理功能区，用于维护本地用户状态并绑定角色。
    Users,
    /// 用户详情功能区，通过动态路径打开但不显示在主导航中。
    UserDetails,
    /// 角色管理功能区，用于维护角色及其直接权限。
    Roles,
    /// 虚拟滚动功能区，用于演示大规模数据表的行列虚拟滚动。
    VirtualScroll,
    /// 报表功能区，用于演示导航很多时的滚动列表入口。
    Reports,
    /// 数据分析功能区，用于演示后续可扩展的分析类入口。
    Analytics,
    /// 发布功能区，用于演示应用发布流水线入口。
    Releases,
    /// 密钥功能区，用于演示凭据和敏感配置入口。
    Secrets,
    /// 集成功能区，用于演示第三方服务连接入口。
    Integrations,
    /// 审计日志功能区，用于演示操作记录和安全追踪入口。
    AuditLogs,
    /// 团队功能区，用于演示成员与权限管理入口。
    Team,
    /// 自动化功能区，用于演示定时任务和规则编排入口。
    Automation,
    /// 通知功能区，用于演示消息中心和事件提醒入口。
    Notifications,
    /// 结算功能区，用于演示订阅、额度和账单入口。
    Billing,
    /// 帮助中心功能区，用于演示文档、支持和反馈入口。
    HelpCenter,
    /// 实验功能区，用于演示开关、灰度和预览能力入口。
    Experiments,
}

impl FeatureId {
    /// 返回用于用户偏好持久化的稳定功能区标识。
    ///
    /// 该标识不会使用展示标题，避免界面文案调整后已保存的置顶标签失效。
    pub const fn id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Projects => "projects",
            Self::ProjectTemplates => "project-templates",
            Self::ProjectEnvironments => "project-environments",
            Self::Tasks => "tasks",
            Self::Users => "users",
            Self::UserDetails => "user-details",
            Self::Roles => "roles",
            Self::VirtualScroll => "virtual-scroll",
            Self::Reports => "reports",
            Self::Analytics => "analytics",
            Self::Releases => "releases",
            Self::Secrets => "secrets",
            Self::Integrations => "integrations",
            Self::AuditLogs => "audit-logs",
            Self::Team => "team",
            Self::Automation => "automation",
            Self::Notifications => "notifications",
            Self::Billing => "billing",
            Self::HelpCenter => "help-center",
            Self::Experiments => "experiments",
        }
    }

    /// 根据用户偏好中的稳定标识恢复功能区。
    ///
    /// 已被新版本移除或无法识别的标识返回 `None`，调用方可以忽略过期的置顶记录。
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "home" => Some(Self::Home),
            "projects" => Some(Self::Projects),
            "project-templates" => Some(Self::ProjectTemplates),
            "project-environments" => Some(Self::ProjectEnvironments),
            "tasks" => Some(Self::Tasks),
            "users" => Some(Self::Users),
            "user-details" => Some(Self::UserDetails),
            "roles" => Some(Self::Roles),
            "virtual-scroll" => Some(Self::VirtualScroll),
            "reports" => Some(Self::Reports),
            "analytics" => Some(Self::Analytics),
            "releases" => Some(Self::Releases),
            "secrets" => Some(Self::Secrets),
            "integrations" => Some(Self::Integrations),
            "audit-logs" => Some(Self::AuditLogs),
            "team" => Some(Self::Team),
            "automation" => Some(Self::Automation),
            "notifications" => Some(Self::Notifications),
            "billing" => Some(Self::Billing),
            "help-center" => Some(Self::HelpCenter),
            "experiments" => Some(Self::Experiments),
            _ => None,
        }
    }

    /// 返回功能区在导航和标题区域展示的名称。
    ///
    /// 该名称应保持短小，适合放在侧边栏和页面标题中。
    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "首页",
            Self::Projects => "项目",
            Self::ProjectTemplates => "模板项目",
            Self::ProjectEnvironments => "运行环境",
            Self::Tasks => "任务",
            Self::Users => "用户管理",
            Self::UserDetails => "用户详情",
            Self::Roles => "角色管理",
            Self::VirtualScroll => "虚拟滚动",
            Self::Reports => "报表",
            Self::Analytics => "数据分析",
            Self::Releases => "发布",
            Self::Secrets => "密钥",
            Self::Integrations => "集成",
            Self::AuditLogs => "审计日志",
            Self::Team => "团队",
            Self::Automation => "自动化",
            Self::Notifications => "通知",
            Self::Billing => "结算",
            Self::HelpCenter => "帮助中心",
            Self::Experiments => "实验功能",
        }
    }

    /// 返回当前功能区对应的稳定逻辑路径模式。
    ///
    /// 静态 Feature 可以直接使用该值打开标签；`UserDetails` 的路径包含 `:id`，必须先
    /// 用具体用户标识替换参数后再交给应用注册表解析。
    pub const fn path(self) -> &'static str {
        match self {
            Self::Home => "/",
            Self::Projects => "/projects",
            Self::ProjectTemplates => "/projects/templates",
            Self::ProjectEnvironments => "/projects/environments",
            Self::Tasks => "/tasks",
            Self::Users => "/users",
            Self::UserDetails => "/users/details/:id",
            Self::Roles => "/roles",
            Self::VirtualScroll => "/examples/virtual-scroll",
            Self::Reports => "/examples/reports",
            Self::Analytics => "/examples/analytics",
            Self::Releases => "/examples/releases",
            Self::Secrets => "/examples/secrets",
            Self::Integrations => "/examples/integrations",
            Self::AuditLogs => "/examples/audit-logs",
            Self::Team => "/examples/team",
            Self::Automation => "/examples/automation",
            Self::Notifications => "/examples/notifications",
            Self::Billing => "/examples/billing",
            Self::HelpCenter => "/examples/help-center",
            Self::Experiments => "/examples/experiments",
        }
    }
}

/// 主窗口中一个已经解析完成的 Feature 标签位置。
///
/// 标签身份使用具体路径而不是 Feature 类型，因此 `/users/details/1` 和
/// `/users/details/2` 可以同时打开，并分别保存自己的路由参数和历史位置。
#[derive(Debug, Clone)]
pub struct FeatureLocation {
    feature: FeatureId,
    route: RouteMatch,
}

impl FeatureLocation {
    /// 使用应用注册表解析路径并创建 Feature 标签位置。
    ///
    /// # Errors
    ///
    /// 路径无法解析、没有匹配项，或者匹配目标不是 Console 已知 Feature 时返回
    /// [`ResolveError`]。
    pub fn resolve(location: &str, registry: &AppRegistry) -> Result<Self, ResolveError> {
        Self::from_route(registry.resolve(location)?)
    }

    /// 从注册表已经解析完成的路由创建 Feature 标签位置。
    pub(crate) fn from_route(route: RouteMatch) -> Result<Self, ResolveError> {
        let RouteTarget::Feature(metadata) = route.target() else {
            return Err(ResolveError::NotFound {
                path: route.concrete_path().to_owned(),
            });
        };
        let Some(feature) = FeatureId::from_id(metadata.id()) else {
            return Err(ResolveError::NotFound {
                path: route.concrete_path().to_owned(),
            });
        };

        Ok(Self { feature, route })
    }

    /// 为不含动态参数的 Feature 创建其默认标签位置。
    ///
    /// 动态 Feature 没有可直接打开的具体路径，或者注册表尚未包含对应定义时返回 `None`。
    pub fn for_feature(feature: FeatureId, registry: &AppRegistry) -> Option<Self> {
        if feature.path().contains(':') {
            return None;
        }

        Self::resolve(feature.path(), registry).ok()
    }

    /// 返回标签所展示的功能区类型。
    pub const fn feature(&self) -> FeatureId {
        self.feature
    }

    /// 返回标签实例的具体路径，也是默认的标签去重键。
    pub fn path(&self) -> &str {
        self.route.concrete_path()
    }

    /// 返回适合标签栏展示的标题。
    ///
    /// 动态用户详情会附加具体用户标识，避免同时打开多个详情标签时无法区分。
    pub fn title(&self) -> String {
        if self.feature == FeatureId::UserDetails
            && let Ok(Path(path)) = self.route.path::<user_details::UserDetailsPath>()
        {
            return format!("{} · {}", self.feature.title(), path.id);
        }

        self.route.target().title().to_owned()
    }

    /// 返回当前标签位置对应的强类型路由匹配结果。
    ///
    /// 页面应通过 [`RouteMatch::path`] 和 [`RouteMatch::query`] 将参数提取为业务结构体，
    /// 避免按字符串键读取动态路径或查询参数。
    pub const fn route(&self) -> &RouteMatch {
        &self.route
    }

    /// 返回当前 Feature 元数据声明的可选图标标识。
    pub fn icon(&self) -> Option<&'static str> {
        self.route.target().icon()
    }
}

impl PartialEq for FeatureLocation {
    fn eq(&self, other: &Self) -> bool {
        self.path() == other.path()
    }
}

impl Eq for FeatureLocation {}

/// 控制台侧边栏中的功能导航项。
///
/// 该类型把功能标识和导航分组信息放在一起，RootView 可以直接消费它来生成侧边栏。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureItem {
    id: FeatureId,
    section: &'static str,
    children: &'static [FeatureChildItem],
}

impl FeatureItem {
    const fn new(id: FeatureId, section: &'static str) -> Self {
        Self {
            id,
            section,
            children: &[],
        }
    }

    const fn with_children(
        id: FeatureId,
        section: &'static str,
        children: &'static [FeatureChildItem],
    ) -> Self {
        Self {
            id,
            section,
            children,
        }
    }

    /// 返回该导航项对应的功能区标识。
    ///
    /// 该值仅用于 Console 壳层的面包屑与导航辅助状态；内容区直接渲染
    /// Nexora 运行时创建的 `FeatureInstance`，不再按此枚举分支渲染。
    pub fn id(self) -> FeatureId {
        self.id
    }

    /// 返回该导航项所属的侧边栏分组名称。
    ///
    /// 模板示例用它区分主要工作区和应用配置区，真实项目可以按业务域重新分组。
    pub fn section(self) -> &'static str {
        self.section
    }

    /// 返回该导航项下面的二级导航项。
    ///
    /// 没有二级导航时返回空切片；RootView 会根据该值决定是否渲染子菜单。
    pub fn children(self) -> &'static [FeatureChildItem] {
        self.children
    }

    /// 判断指定功能区是否属于该导航项。
    ///
    /// 该方法会同时检查自身功能区和所有二级导航项，便于父级菜单在子项激活时保持展开和高亮。
    pub fn contains(self, feature: FeatureId) -> bool {
        self.id == feature || self.children.iter().any(|child| child.id() == feature)
    }
}

/// 控制台侧边栏中的二级导航项。
///
/// 该类型用于表达某个 feature 下更细粒度的页面入口，例如项目功能区下的模板项目和运行环境。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureChildItem {
    id: FeatureId,
    title: &'static str,
}

impl FeatureChildItem {
    const fn new(id: FeatureId, title: &'static str) -> Self {
        Self { id, title }
    }

    /// 返回该二级导航项对应的功能区标识。
    ///
    /// RootView 会把这个值保存为当前选中 feature，并据此切换顶部标题和主内容面板。
    pub fn id(self) -> FeatureId {
        self.id
    }

    /// 返回该二级导航项在侧边栏中展示的名称。
    ///
    /// 该名称可以比功能区标题更贴近导航语境，例如用“项目概览”区分父级“项目”。
    pub fn title(self) -> &'static str {
        self.title
    }
}

/// 返回控制台应用默认的功能导航目录。
///
/// 返回值顺序就是侧边栏渲染顺序。该函数保持静态数据，避免每次渲染时重新分配导航项。
pub fn feature_catalog() -> &'static [FeatureItem] {
    static PROJECT_CHILDREN: [FeatureChildItem; 3] = [
        FeatureChildItem::new(FeatureId::Projects, "项目概览"),
        FeatureChildItem::new(FeatureId::ProjectTemplates, "模板项目"),
        FeatureChildItem::new(FeatureId::ProjectEnvironments, "运行环境"),
    ];
    static CATALOG: [FeatureItem; 18] = [
        FeatureItem::new(FeatureId::Home, "工作台"),
        FeatureItem::with_children(FeatureId::Projects, "工作台", &PROJECT_CHILDREN),
        FeatureItem::new(FeatureId::Tasks, "工作台"),
        FeatureItem::new(FeatureId::Users, "访问控制"),
        FeatureItem::new(FeatureId::Roles, "访问控制"),
        FeatureItem::new(FeatureId::VirtualScroll, "扩展示例"),
        FeatureItem::new(FeatureId::Reports, "扩展示例"),
        FeatureItem::new(FeatureId::Analytics, "扩展示例"),
        FeatureItem::new(FeatureId::Releases, "扩展示例"),
        FeatureItem::new(FeatureId::Secrets, "扩展示例"),
        FeatureItem::new(FeatureId::Integrations, "扩展示例"),
        FeatureItem::new(FeatureId::AuditLogs, "扩展示例"),
        FeatureItem::new(FeatureId::Team, "扩展示例"),
        FeatureItem::new(FeatureId::Automation, "扩展示例"),
        FeatureItem::new(FeatureId::Notifications, "扩展示例"),
        FeatureItem::new(FeatureId::Billing, "扩展示例"),
        FeatureItem::new(FeatureId::HelpCenter, "扩展示例"),
        FeatureItem::new(FeatureId::Experiments, "扩展示例"),
    ];

    &CATALOG
}

/// 按目录中的连续分组返回侧边栏功能项。
///
/// 侧边栏通过该迭代器生成分组，确保新增 section 后不需要再同步维护一份硬编码筛选列表。
pub fn feature_catalog_sections() -> impl Iterator<Item = (&'static str, &'static [FeatureItem])> {
    feature_catalog()
        .chunk_by(|current, next| current.section() == next.section())
        .filter_map(|items| items.first().map(|first| (first.section(), items)))
}

#[derive(nexora::NavigationGroup)]
#[nexora(
    id = "project-management",
    title = "项目管理",
    section = "工作台",
    icon = "folder-open",
    order = 10
)]
struct ProjectManagementGroup;

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "模板项目",
    path = "/projects/templates",
    group = "project-management",
    icon = "folder-open",
    order = 11
)]
struct ProjectTemplatesFeature;

impl nexora::FeatureElement for ProjectTemplatesFeature {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        projects::render_content(cx)
    }
}

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "运行环境",
    path = "/projects/environments",
    group = "project-management",
    icon = "folder-open",
    order = 12
)]
struct ProjectEnvironmentsFeature;

impl nexora::FeatureElement for ProjectEnvironmentsFeature {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        projects::render_content(cx)
    }
}

#[rustfmt::skip]
macro_rules! overflow_feature {
    ($name:ident, $title:literal, $path:literal, $icon:literal, $order:literal) => {
        #[derive(Default, nexora::Feature)]
        #[nexora(
            title = $title,
            path = $path,
            section = "扩展示例",
            icon = $icon,
            order = $order
        )]
        struct $name;

        impl nexora::FeatureElement for $name {
            fn render(
                &mut self,
                _window: &mut gpui::Window,
                cx: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                let theme = cx.theme();

                gpui::div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_5()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.tokens.group_box)
                    .child(
                        gpui::div()
                            .text_lg()
                            .font_bold()
                            .text_color(theme.foreground)
                            .child($title),
                    )
                    .child(
                        gpui::div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("这是用于演示导航项增多后 Sidebar 中间区域滚动行为的占位页面。"),
                    )
            }
        }
    };
}

overflow_feature!(ReportsFeature, "报表", "/examples/reports", "chart-pie", 60);
overflow_feature!(
    AnalyticsFeature,
    "数据分析",
    "/examples/analytics",
    "inspector",
    70
);
overflow_feature!(ReleasesFeature, "发布", "/examples/releases", "globe", 80);
overflow_feature!(SecretsFeature, "密钥", "/examples/secrets", "eye-off", 90);
overflow_feature!(
    IntegrationsFeature,
    "集成",
    "/examples/integrations",
    "building-2",
    100
);
overflow_feature!(
    AuditLogsFeature,
    "审计日志",
    "/examples/audit-logs",
    "book-open",
    110
);
overflow_feature!(TeamFeature, "团队", "/examples/team", "user", 120);
overflow_feature!(
    AutomationFeature,
    "自动化",
    "/examples/automation",
    "bot",
    130
);
overflow_feature!(
    NotificationsFeature,
    "通知",
    "/examples/notifications",
    "bell",
    140
);
overflow_feature!(
    BillingFeature,
    "结算",
    "/examples/billing",
    "building-2",
    150
);
overflow_feature!(
    HelpCenterFeature,
    "帮助中心",
    "/examples/help-center",
    "info",
    160
);
overflow_feature!(
    ExperimentsFeature,
    "实验功能",
    "/examples/experiments",
    "palette",
    170
);

/// 构建 Console 使用的统一 Feature 与 Window 注册表。
///
/// 注册顺序由派生元数据中的 `order` 决定；Feature 和 Window 共用同一套路由冲突检查。
///
/// # Panics
///
/// 当编译进 Console 的静态路由定义存在重复标识、非法父级或路径冲突时 panic。此类错误
/// 属于开发期配置错误，并由对应注册表测试提前覆盖。
pub(crate) fn feature_registry() -> AppRegistry {
    AppRegistry::discover().expect("Console 的静态 Nexora 路由定义应当有效")
}

/// 首页功能模块。
#[path = "features/home.rs"]
pub mod home;

/// 未登录时展示的独立认证门禁。
#[path = "features/login.rs"]
pub mod login;

/// 项目管理功能模块。
#[path = "features/projects.rs"]
pub mod projects;

/// 角色与权限管理功能模块。
#[path = "features/roles.rs"]
pub mod roles;

/// 根视图功能模块。
#[path = "features/root.rs"]
pub mod root;

/// 应用设置功能模块。
#[path = "features/settings.rs"]
pub mod settings;

/// 任务管理功能模块。
#[path = "features/tasks.rs"]
pub mod tasks;

/// 用户与用户角色管理功能模块。
#[path = "features/users.rs"]
pub mod users;

/// 通过动态路径打开的用户详情功能模块。
#[path = "features/user_details.rs"]
pub mod user_details;

/// 虚拟滚动数据表功能模块。
#[path = "features/virtual_scroll.rs"]
pub mod virtual_scroll;
