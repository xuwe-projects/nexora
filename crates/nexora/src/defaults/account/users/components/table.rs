//! 默认用户列表表格。

use gpui::{IntoElement, RenderOnce, WeakEntity, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::Button,
    h_flex,
    table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow},
    v_flex,
};

use crate::desktop::contract::{UserResponse, UserStatus};

use super::UsersPage;

#[derive(IntoElement)]
pub(in crate::defaults::account::users) struct UsersTable {
    users: Vec<UserResponse>,
    busy_user_id: Option<String>,
    role_editor_busy: bool,
    can_change_status: bool,
    can_manage_roles: bool,
    page: WeakEntity<UsersPage>,
}

impl UsersTable {
    pub(super) fn new(
        users: Vec<UserResponse>,
        busy_user_id: Option<String>,
        role_editor_busy: bool,
        can_change_status: bool,
        can_manage_roles: bool,
        page: WeakEntity<UsersPage>,
    ) -> Self {
        Self {
            users,
            busy_user_id,
            role_editor_busy,
            can_change_status,
            can_manage_roles,
            page,
        }
    }
}

impl RenderOnce for UsersTable {
    fn render(self, _window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
        let rows = self.users.into_iter().map(|user| {
            let role_user_id = user.id.clone();
            let status_user_id = user.id.clone();
            let role_page = self.page.clone();
            let status_page = self.page.clone();
            let mutation_busy = self.busy_user_id.is_some();
            let current_user_busy = self.busy_user_id.as_deref() == Some(user.id.as_str());
            let is_active = user.status == UserStatus::Active;
            let status_label = if is_active { "已启用" } else { "已停用" };
            let status_action = if is_active { "停用" } else { "启用" };
            let target_status = if is_active {
                UserStatus::Suspended
            } else {
                UserStatus::Active
            };
            let account_label = if user.is_super_admin {
                format!("{} · 超级管理员", user.display_name)
            } else {
                user.display_name.clone()
            };

            TableRow::new()
                .child(
                    TableCell::new().child(
                        v_flex().gap_1().child(account_label).child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(user.id),
                        ),
                    ),
                )
                .child(TableCell::new().child(user.email.unwrap_or_else(|| "—".to_owned())))
                .child(TableCell::new().w(px(88.)).child(status_label))
                .child(TableCell::new().child(user.identity_id))
                .child(
                    TableCell::new().w(px(220.)).child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new(format!("default-user-roles-{role_user_id}"))
                                    .small()
                                    .label("管理角色")
                                    .disabled(
                                        user.is_super_admin
                                            || mutation_busy
                                            || self.role_editor_busy
                                            || !self.can_manage_roles,
                                    )
                                    .tooltip(if self.can_manage_roles {
                                        "完整替换用户的直接角色集合"
                                    } else {
                                        "需要 users:roles.write 与 roles:read 权限"
                                    })
                                    .on_click(move |_, _, cx| {
                                        _ = role_page.update(cx, |page, cx| {
                                            page.manage_roles(role_user_id.clone(), cx);
                                        });
                                    }),
                            )
                            .child(
                                Button::new(format!("default-user-status-{status_user_id}"))
                                    .small()
                                    .outline()
                                    .label(status_action)
                                    .loading(current_user_busy)
                                    .disabled(
                                        user.is_super_admin
                                            || mutation_busy
                                            || !self.can_change_status,
                                    )
                                    .tooltip(if self.can_change_status {
                                        status_action
                                    } else {
                                        "需要 users:status.write 权限"
                                    })
                                    .on_click(move |_, _, cx| {
                                        _ = status_page.update(cx, |page, cx| {
                                            page.set_user_status(
                                                status_user_id.clone(),
                                                target_status,
                                                cx,
                                            );
                                        });
                                    }),
                            ),
                    ),
                )
        });

        Table::new()
            .child(
                TableHeader::new().child(
                    TableRow::new()
                        .child(TableHead::new().child("用户"))
                        .child(TableHead::new().child("邮箱"))
                        .child(TableHead::new().w(px(88.)).child("状态"))
                        .child(TableHead::new().child("Identity ID"))
                        .child(TableHead::new().w(px(220.)).child("操作")),
                ),
            )
            .child(TableBody::new().children(rows))
    }
}
