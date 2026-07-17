//! 默认角色列表。

use gpui::{IntoElement, RenderOnce, WeakEntity, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _,
    button::Button, h_flex, tag::Tag, v_flex,
};

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
        let cards = self.roles.into_iter().map(|role| {
            let role_id = role.id;
            let page = self.page.clone();
            let selected = self.selected_role_id == Some(role_id);
            v_flex()
                .gap_3()
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(if selected {
                    cx.theme().primary
                } else {
                    cx.theme().border
                })
                .child(
                    h_flex()
                        .justify_between()
                        .gap_3()
                        .child(
                            v_flex()
                                .min_w_0()
                                .gap_1()
                                .child(div().font_semibold().child(role.name.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(role.key.clone()),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(if role.is_system {
                                    Tag::secondary().small().child("内置").into_any_element()
                                } else {
                                    Tag::info().small().child("自定义").into_any_element()
                                })
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{} 项权限", role.permissions.len())),
                                ),
                        ),
                )
                .when_some(role.description, |this, description| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    )
                })
                .child(
                    h_flex().justify_end().child(
                        Button::new(format!("manage-default-role-{role_id}"))
                            .small()
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
                )
        });

        v_flex().gap_3().children(cards)
    }
}
