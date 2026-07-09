//! 控制台应用的功能模块集合。
//!
//! 每个公开子模块对应一个可独立维护的界面或功能区域。

/// 控制台应用中的功能区标识。
///
/// 根视图使用该枚举保存当前选中的功能区，并根据它决定主内容区渲染哪个 feature。
/// 新增业务页面时，通常需要在这里增加一个变体，并同步扩展 `feature_catalog`。
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
    /// 设置功能区，用于展示应用级偏好和运行参数。
    Settings,
}

impl FeatureId {
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
            Self::Settings => "设置",
        }
    }

    /// 返回功能区的简短说明。
    ///
    /// 根视图会把该说明展示在顶部标题栏，帮助使用者理解当前页面职责。
    pub fn description(self) -> &'static str {
        match self {
            Self::Home => "查看应用概览和下一步入口",
            Self::Projects => "管理本地桌面应用项目",
            Self::ProjectTemplates => "查看可复用的桌面应用模板",
            Self::ProjectEnvironments => "检查本地开发工具链和运行环境",
            Self::Tasks => "查看构建、打包和发布任务",
            Self::VirtualScroll => "查看大规模股票表格的行列虚拟滚动",
            Self::Reports => "查看业务报表和运行摘要",
            Self::Analytics => "分析产品、构建和使用趋势",
            Self::Releases => "管理版本、渠道和发布流程",
            Self::Secrets => "管理签名、发布和服务凭据",
            Self::Integrations => "配置第三方服务和自动化连接",
            Self::AuditLogs => "追踪关键操作和安全事件",
            Self::Team => "管理团队成员、角色和权限",
            Self::Automation => "配置定时任务、规则和后台编排",
            Self::Notifications => "查看系统消息、构建事件和提醒",
            Self::Billing => "查看订阅、额度和费用信息",
            Self::HelpCenter => "打开文档、支持和问题反馈入口",
            Self::Experiments => "管理实验性功能和预览开关",
            Self::Settings => "调整桌面程序运行配置",
        }
    }
}

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
    /// 调用方通常把这个值保存为当前选中 feature，然后在内容区进行模式匹配。
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
    static CATALOG: [FeatureItem; 17] = [
        FeatureItem::new(FeatureId::Home, "工作台"),
        FeatureItem::with_children(FeatureId::Projects, "工作台", &PROJECT_CHILDREN),
        FeatureItem::new(FeatureId::Tasks, "工作台"),
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
        FeatureItem::new(FeatureId::Settings, "系统"),
    ];

    &CATALOG
}

/// 首页功能模块。
pub mod home;

/// 项目管理功能模块。
pub mod projects;

/// 根视图功能模块。
pub mod root;

/// 应用设置功能模块。
pub mod settings;

/// 任务管理功能模块。
pub mod tasks;

/// 虚拟滚动数据表功能模块。
pub mod virtual_scroll;
