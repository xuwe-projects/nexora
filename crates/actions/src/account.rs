//! 账户相关 action。
//!
//! 该模块提供账户菜单中使用的 GPUI action、默认快捷键以及菜单展示元数据。

use gpui::{Action, App, KeyBinding};

gpui::actions!(
    console_account_menu,
    [
        /// 打开当前登录用户的个人资料页。
        OpenAccountProfile,
        /// 打开账户或应用设置入口。
        OpenAccountSettings,
        /// 退出当前登录账户。
        SignOutAccount
    ]
);

/// 账户菜单使用的 GPUI key context。
///
/// 视图需要在可接收账户快捷键的根元素上设置该 context，菜单才能正确展示和解析这些快捷键。
pub const CONTEXT: &str = "console_account_menu";

/// 账户菜单中的业务动作种类。
///
/// 该枚举描述菜单项背后的意图，调用方可以根据它选择图标、路由或具体处理函数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountActionKind {
    /// 打开当前登录用户的个人资料页。
    Profile,

    /// 打开设置页或账户设置入口。
    Settings,

    /// 退出当前登录状态。
    SignOut,
}

/// 账户菜单中的展示项配置。
///
/// 该类型把菜单文案、默认快捷键和业务动作绑定在一起，避免每个 UI 入口重复维护这些元数据。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountActionSpec {
    kind: AccountActionKind,
    label: String,
    shortcut: Option<&'static str>,
    uses_account_avatar: bool,
}

impl AccountActionSpec {
    /// 创建一个账户菜单动作配置。
    ///
    /// `kind` 表示业务意图；`label` 是菜单文案；`shortcut` 是建议展示给用户的快捷键；
    /// `uses_account_avatar` 表示该项是否应该使用账户头像语义渲染。
    pub fn new(
        kind: AccountActionKind,
        label: impl Into<String>,
        shortcut: Option<&'static str>,
        uses_account_avatar: bool,
    ) -> Self {
        Self {
            kind,
            label: label.into(),
            shortcut,
            uses_account_avatar,
        }
    }

    /// 返回该菜单项对应的业务动作种类。
    ///
    /// UI 层可以根据该值选择图标，应用层可以根据该值分发到不同处理流程。
    pub fn kind(&self) -> AccountActionKind {
        self.kind
    }

    /// 返回该菜单项展示给用户的文案。
    ///
    /// 个人资料项通常使用当前用户名称，其他项使用固定命令文案。
    pub fn label(&self) -> &str {
        self.label.as_str()
    }

    /// 返回该菜单项的默认快捷键文案。
    ///
    /// 该值用于测试和说明命令设计；实际菜单右侧展示由 GPUI 根据绑定的 action 自动生成。
    pub fn shortcut(&self) -> Option<&'static str> {
        self.shortcut
    }

    /// 返回该菜单项是否应该使用账户头像语义。
    ///
    /// 当前只有个人资料入口返回 `true`，便于 UI 使用头像或用户图标强调身份入口。
    pub fn uses_account_avatar(&self) -> bool {
        self.uses_account_avatar
    }

    /// 将菜单配置转换为 GPUI action 对象。
    ///
    /// `PopupMenu`、快捷键系统和命令派发都通过该 action 身份连接到具体处理器。
    pub fn to_action(&self) -> Box<dyn Action> {
        match self.kind {
            AccountActionKind::Profile => Box::new(OpenAccountProfile),
            AccountActionKind::Settings => Box::new(OpenAccountSettings),
            AccountActionKind::SignOut => Box::new(SignOutAccount),
        }
    }
}

/// 返回账户菜单默认动作列表。
///
/// `account_name` 会作为个人资料入口的展示文案，其余动作保持固定顺序：设置、退出登录。
pub fn menu_actions(account_name: impl Into<String>) -> Vec<AccountActionSpec> {
    vec![
        AccountActionSpec::new(
            AccountActionKind::Profile,
            account_name,
            Some("Cmd+Shift+P"),
            true,
        ),
        AccountActionSpec::new(AccountActionKind::Settings, "设置", Some("Cmd+,"), false),
        AccountActionSpec::new(
            AccountActionKind::SignOut,
            "退出登录",
            Some("Cmd+Shift+Q"),
            false,
        ),
    ]
}

/// 注册账户菜单默认快捷键。
///
/// 调用方通常在应用初始化或创建根视图前调用该函数，使菜单项和键盘入口共享相同 action。
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-p", OpenAccountProfile, Some(CONTEXT)),
        KeyBinding::new("cmd-,", OpenAccountSettings, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-q", SignOutAccount, Some(CONTEXT)),
    ]);
}
