//! 账户相关 action。
//!
//! 该模块提供账户菜单中使用的 GPUI action、默认快捷键以及菜单展示元数据。

use gpui::{Action, App, KeyBinding};

use crate::settings::{self, OpenSettings};

gpui::actions!(
    console_account_menu,
    [
        /// 打开系统浏览器并登录当前账户。
        SignInAccount,
        /// 退出当前登录账户。
        SignOutAccount
    ]
);

/// 账户菜单中需要登录态的 GPUI key context。
///
/// 视图需要在已登录工作区的根元素上设置该 context，退出登录快捷键才会生效。
/// 登录快捷键是应用级入口，不依赖焦点或该 context。
pub const CONTEXT: &str = "console_account_menu";

/// 账户菜单中的业务动作种类。
///
/// 该枚举描述菜单项背后的意图，调用方可以根据它选择图标、路由或具体处理函数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountActionKind {
    /// 打开系统浏览器并登录当前账户。
    SignIn,

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
}

impl AccountActionSpec {
    /// 创建一个账户菜单动作配置。
    ///
    /// `kind` 表示业务意图；`label` 是菜单文案；`shortcut` 是建议展示给用户的快捷键。
    pub fn new(
        kind: AccountActionKind,
        label: impl Into<String>,
        shortcut: Option<&'static str>,
    ) -> Self {
        Self {
            kind,
            label: label.into(),
            shortcut,
        }
    }

    /// 返回该菜单项对应的业务动作种类。
    ///
    /// UI 层可以根据该值选择图标，应用层可以根据该值分发到不同处理流程。
    pub fn kind(&self) -> AccountActionKind {
        self.kind
    }

    /// 返回该菜单项展示给用户的文案。
    pub fn label(&self) -> &str {
        self.label.as_str()
    }

    /// 返回该菜单项的默认快捷键文案。
    ///
    /// 该值用于测试和说明命令设计；实际菜单右侧展示由 GPUI 根据绑定的 action 自动生成。
    pub fn shortcut(&self) -> Option<&'static str> {
        self.shortcut
    }

    /// 将菜单配置转换为 GPUI action 对象。
    ///
    /// `PopupMenu`、快捷键系统和命令派发都通过该 action 身份连接到具体处理器。
    pub fn to_action(&self) -> Box<dyn Action> {
        match self.kind {
            AccountActionKind::SignIn => Box::new(SignInAccount),
            AccountActionKind::Settings => Box::new(OpenSettings),
            AccountActionKind::SignOut => Box::new(SignOutAccount),
        }
    }
}

/// 返回账户菜单默认动作列表。
///
/// 当前账户名称由账户栏本身展示；菜单只暴露已经实现的设置和退出操作。
pub fn menu_actions() -> Vec<AccountActionSpec> {
    vec![
        AccountActionSpec::new(
            AccountActionKind::Settings,
            "设置",
            Some(settings::shortcut_label()),
        ),
        AccountActionSpec::new(AccountActionKind::SignOut, "退出登录", Some("Cmd+Shift+Q")),
    ]
}

/// 返回未登录时账户菜单默认动作列表。
///
/// 第一项会触发浏览器 OIDC 登录，后续仍提供设置入口，便于用户检查认证配置。
pub fn signed_out_menu_actions() -> Vec<AccountActionSpec> {
    vec![
        AccountActionSpec::new(AccountActionKind::SignIn, "登录", Some("Cmd+Shift+L")),
        AccountActionSpec::new(
            AccountActionKind::Settings,
            "设置",
            Some(settings::shortcut_label()),
        ),
    ]
}

/// 注册账户菜单默认快捷键。
///
/// 调用方通常在应用初始化或创建根视图前调用该函数，使菜单项和键盘入口共享相同 action。
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-l", SignInAccount, None),
        KeyBinding::new("cmd-shift-q", SignOutAccount, Some(CONTEXT)),
    ]);
}
