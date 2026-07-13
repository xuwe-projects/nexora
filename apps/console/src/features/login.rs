//! Console 未登录认证门禁接入。
//!
//! 通用视觉由 `ui::LoginGate` 提供；本模块只负责把 Console 的 OIDC 状态、登录动作、
//! 设置窗口和外部支持地址装配到共享组件中。

use actions::settings::OpenSettings;
use gpui::{AnyElement, Context, IntoElement as _, Window};
use ui::LoginGate;

use crate::auth;

const PRIVACY_URL: &str = "https://xuwe.cc/privacy";
const HELP_URL: &str = "https://github.com/xuwe-projects/desktop-template/issues";

/// Console 未登录状态使用的认证页面适配器。
pub struct LoginFeature;

impl LoginFeature {
    /// 读取 Console 认证快照并渲染共享登录门禁。
    pub fn render<T>(_window: &mut Window, cx: &mut Context<T>) -> AnyElement
    where
        T: 'static,
    {
        let snapshot = auth::snapshot(cx);
        let status = visible_status(snapshot.status.as_ref()).then_some(snapshot.status.clone());

        LoginGate::new(
            "铉微 Console",
            format!("Console {}", env!("CARGO_PKG_VERSION")),
            |_, _, cx| {
                if let Err(error) = auth::start_login(cx) {
                    auth::complete_login(Err(error), cx);
                }
            },
            |_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx),
        )
        .configured(snapshot.configured)
        .busy(snapshot.busy)
        .status(status)
        .privacy_url(PRIVACY_URL)
        .help_url(HELP_URL)
        .into_any_element()
    }
}

fn visible_status(status: &str) -> bool {
    !status.is_empty() && status != "未登录" && status != "未配置 OIDC" && status != "已退出登录"
}
