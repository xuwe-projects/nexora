//! Console 桌面应用定义。
//!
//! 该模块实现 `desktop::Application`，把控制台应用接入统一的桌面启动流程。

use crate::{
    auth, config,
    features::{root::RootView, settings as settings_feature},
};
use actions::{account as account_actions, settings as settings_actions, window as window_actions};
use desktop::{Application, ApplicationOptions};
use gpui::{App, AppContext, Entity, Window, WindowOptions, px, size};
use gpui_component::TitleBar;
use updater::{UpdateChannel, UpdateConfig};

/// Console 本地开发构建使用的默认安装包构建号。
///
/// 正式发布应通过 `BUNDLE_VERSION` 提供持续递增的构建号；未设置时才使用该默认值。
const DEFAULT_BUNDLE_VERSION: u64 = 1;

/// 控制台桌面应用。
///
/// 该类型保存应用启动选项，并负责创建主窗口中的业务根视图。
pub struct Console {
    /// 控制台应用的运行时配置。
    options: ApplicationOptions,
}

impl Default for Console {
    /// 创建与 `Console::new` 完全一致的默认控制台应用。
    ///
    /// 默认值包含窗口尺寸、最小尺寸、激活行为和原生标题栏配置，避免派生默认值绕过应用约定。
    fn default() -> Self {
        Self::new()
    }
}

impl Console {
    /// 创建一个使用默认启动选项的控制台应用。
    ///
    /// 调用方可以继续通过 `with_*` 方法覆盖守护模式、激活行为、窗口尺寸、最小窗口尺寸或窗口选项。
    pub fn new() -> Self {
        Self {
            options: ApplicationOptions {
                window_size: Some(size(px(900.), px(640.))),
                window_min_size: Some(size(px(900.), px(640.))),
                activate: true,
                window_options: Some(WindowOptions {
                    titlebar: Some(TitleBar::title_bar_options()),
                    ..Default::default()
                }),
                daemon_mode: false,
                startup_display_uuid: None,
            },
        }
    }
}

impl Application for Console {
    /// 控制台应用主窗口中的业务根视图类型。
    ///
    /// 该类型负责渲染控制台窗口的最外层业务内容。
    type RootView = RootView;

    /// 返回控制台应用当前的运行时配置。
    ///
    /// 该配置会在调用 `run` 时被桌面运行器读取并消费。
    fn options(&self) -> &ApplicationOptions {
        &self.options
    }

    /// 返回控制台应用运行时配置的可变引用。
    ///
    /// 桌面运行器提供的链式配置方法会通过该引用写入启动参数。
    fn options_mut(&mut self) -> &mut ApplicationOptions {
        &mut self.options
    }

    /// 在主窗口创建前恢复当前操作系统用户的本地偏好。
    ///
    /// 主题会立即应用到组件库；启动显示器 UUID 会交给桌面运行器解析为本次进程的显示器 ID。
    fn initialize(&mut self, cx: &mut App) {
        config::init(cx);
        theme::set_selection(config::theme_selection(cx), cx);
        theme::set_font_size(config::font_size(cx), cx);
        theme::set_component_size(config::component_size(cx), cx);
        self.options.startup_display_uuid = config::startup_display_uuid(cx).map(ToOwned::to_owned);
        actions::init();
        account_actions::bind_keys(cx);
        settings_actions::bind_keys(cx);
        initialize_auth(cx);
        register_account_actions(cx);
    }

    /// 创建控制台应用的根视图实体。
    ///
    /// 当前实现创建 `features::root::RootView`，该实体会由桌面运行器包裹进
    /// `gpui_component::Root` 后作为窗口根节点。
    fn build_root_view(&mut self, window: &mut Window, cx: &mut App) -> Entity<Self::RootView> {
        gpui_component::set_locale("zh-CN");
        settings_feature::init(console_updater_config(), cx);
        window_actions::init("Nexora Console", cx);
        let pinned_tabs = config::pinned_tab_paths(cx);
        cx.new(|cx| {
            let mut root = RootView::with_pinned_paths(pinned_tabs);
            root.initialize_feature_state(window, cx);
            root
        })
    }
}

/// 注册不依赖窗口焦点的账户 action。
///
/// 登录快捷键必须在未登录门禁尚未获得焦点时也可用；已有会话或认证任务执行中时，
/// 该处理器会忽略重复请求。
pub(crate) fn register_account_actions(cx: &mut App) {
    cx.on_action(|_: &account_actions::SignInAccount, cx| {
        let snapshot = auth::snapshot(cx);
        if snapshot.authenticated || snapshot.busy {
            return;
        }

        if let Err(error) = auth::start_login(cx) {
            auth::complete_login(Err(error), cx);
        }
    });
}

fn initialize_auth(cx: &mut App) {
    let config = match auth::config_from_environment() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(error = %error, "Console OIDC 配置无效");
            None
        }
    };
    let store = match config.as_ref() {
        Some(config) => match auth::token_store(config) {
            Ok(store) => Some(store),
            Err(error) => {
                tracing::error!(error = %error, "无法初始化 Console 系统凭据存储");
                None
            }
        },
        None => None,
    };

    auth::init(config, store, cx);
}

/// 创建 Console 使用的更新配置。
///
/// 发布构建通过 `UPDATE_MANIFEST_URL` 编译时环境变量提供当前通道的 `latest.json`
/// 地址；本地开发未设置该变量时，设置页仍可展示版本和更新日志，但不会启用在线更新。
fn console_updater_config() -> Option<UpdateConfig> {
    let manifest_url = option_env!("UPDATE_MANIFEST_URL")?;
    let bundle_version = match option_env!("BUNDLE_VERSION") {
        Some(value) => match value.parse::<u64>() {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(error = %error, "Console 构建号 BUNDLE_VERSION 无效");
                return None;
            }
        },
        None => DEFAULT_BUNDLE_VERSION,
    };
    let mut config = match UpdateConfig::new(
        manifest_url,
        "com.nexora.console",
        env!("CARGO_PKG_VERSION"),
        bundle_version,
        UpdateChannel::Stable,
    ) {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(error = %error, "Console 更新配置无效");
            return None;
        }
    };

    if let Some(team_id) = option_env!("MACOS_TEAM_ID") {
        config = config.with_expected_team_id(team_id);
    }

    Some(config)
}
