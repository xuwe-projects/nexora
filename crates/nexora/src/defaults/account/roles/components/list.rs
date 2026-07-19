//! 默认角色列表。

use gpui::{IntoElement, RenderOnce, WeakEntity, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _,
    button::Button, h_flex, tag::Tag, v_flex,
};
use ui::Card;

use crate::desktop::contract::RoleResponse;

use super::RolesPage;

#[derive(IntoElement)]
pub(in crate::defaults::account::roles) struct RolesList {
    roles: Vec<RoleResponse>,
    selected_role_id: Option<i64>,
    busy: bool,
    page: WeakEntity<RolesPage>,
}

impl RolesList {
    pub(super) fn new(
        roles: Vec<RoleResponse>,
        selected_role_id: Option<i64>,
        busy: bool,
        page: WeakEntity<RolesPage>,
    ) -> Self {
        Self {
            roles,
            selected_role_id,
            busy,
            page,
        }
    }
}

impl RenderOnce for RolesList {
    fn render(self, _window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let cards = self.roles.into_iter().map(|role| {
            let role_id = role.id;
            let page = self.page.clone();
            let selected = self.selected_role_id == Some(role_id);
            Card::new()
                .w_full()
                .p_3()
                .border_color(if selected {
                    cx.theme().primary
                } else {
                    cx.theme().border
                })
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .justify_between()
                        .gap_3()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .min_w_0()
                                        .gap_2()
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .font_semibold()
                                                .child(role.name.clone()),
                                        )
                                        .child(if role.key == "admin" {
                                            Tag::info()
                                                .small()
                                                .child("系统管理员")
                                                .into_any_element()
                                        } else if role.is_system {
                                            Tag::secondary()
                                                .small()
                                                .child("内置")
                                                .into_any_element()
                                        } else {
                                            Tag::new().small().child("自定义").into_any_element()
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .truncate()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(role.key.clone()),
                                )
                                .when_some(role.description.clone(), |this, description| {
                                    this.child(
                                        div()
                                            .text_sm()
                                            .truncate()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(description),
                                    )
                                }),
                        )
                        .child(
                            h_flex()
                                .flex_shrink_0()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{} 项权限", role.permissions.len())),
                                )
                                .child(
                                    Button::new(format!("manage-default-role-{role_id}"))
                                        .with_size(component_size)
                                        .outline()
                                        .selected(selected)
                                        .disabled(self.busy)
                                        .label(if selected {
                                            "正在管理"
                                        } else {
                                            "管理角色"
                                        })
                                        .on_click(move |_, window, cx| {
                                            _ = page.update(cx, |page, cx| {
                                                page.select_role(role_id, window, cx);
                                            });
                                        }),
                                ),
                        ),
                )
        });

        v_flex().gap_3().children(cards)
    }
}
