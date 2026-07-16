//! 通过动态 Nexora 路径打开的用户详情演示页面。

use gpui::{AnyElement, Context, IntoElement as _, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, StyledExt as _, alert::Alert, description_list::DescriptionList, v_flex,
};
use nexora::{FeatureContextExt as _, Path, Query};
use serde::Deserialize;

/// 用户详情动态路径中的强类型参数。
#[derive(Debug, Clone, Deserialize)]
pub struct UserDetailsPath {
    /// 要展示的用户标识。
    pub id: String,
}

/// 用户详情页面支持的强类型查询参数。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserDetailsQuery {
    /// 初次打开详情页时要展示的可选分类，例如 `roles`。
    pub tab: Option<String>,
}

/// 用户详情动态路由 Feature。
///
/// 该页面不出现在 Sidebar 中，只能通过 `/users/details/:id` 的具体路径打开。每个具体
/// 用户路径拥有独立标签身份，用于验证 Nexora 的动态参数、标签去重和导航历史语义。
#[derive(Default, nexora::Feature)]
#[nexora(
    title = "用户详情",
    path = "/users/details/:id",
    path_params = UserDetailsPath,
    query_params = UserDetailsQuery,
    icon = "user",
    order = 31,
    navigation = false
)]
pub struct UserDetailsFeature;

impl nexora::FeatureElement for UserDetailsFeature {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let Path(path) = cx.path();
        let Query(query) = cx.query();
        let route = cx.feature_route();

        render_content(path.id, query.tab, route.concrete_path().to_owned(), cx)
    }
}

fn render_content<T>(
    user_id: String,
    tab: Option<String>,
    concrete_path: String,
    cx: &mut Context<T>,
) -> AnyElement
where
    T: 'static,
{
    let tab = tab.unwrap_or_else(|| "未指定".to_owned());
    v_flex()
        .w_full()
        .gap_4()
        .p_5()
        .child(
            v_flex()
                .gap_1()
                .child(div().text_xl().font_bold().child("用户详情"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("这是 navigation = false 的动态 Feature，不会出现在侧边栏中。"),
                ),
        )
        .child(Alert::info(
            "nexora-user-details-route",
            "同一路径会激活已有标签，不同 id 会打开不同标签。",
        ))
        .child(
            DescriptionList::new()
                .columns(1)
                .item("Feature", "user-details", 1)
                .item("具体路径", concrete_path, 1)
                .item("路由参数 id", user_id, 1)
                .item("查询参数 tab", tab, 1)
                .item("导航可见性", "false", 1),
        )
        .into_any_element()
}
