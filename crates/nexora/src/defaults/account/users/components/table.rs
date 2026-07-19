//! 默认用户列表数据表。

use std::collections::BTreeSet;

use gpui::{App, Context, Entity, IntoElement, Render, WeakEntity, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    avatar::Avatar,
    button::Button,
    h_flex,
    table::{Column, DataTable, TableState},
    tag::Tag,
    v_flex,
};

use crate::{
    defaults::account::has_permission,
    desktop::contract::{UserResponse, UserStatus, UserType},
};
use ui::{Card, CrudTableDelegate, TableCell};

use super::UsersPage;

const USER_TABLE_ROW_HEIGHT: f32 = 52.0;

pub(in crate::defaults::account::users) struct UsersTable {
    state: Entity<TableState<UsersTableDelegate>>,
    page: WeakEntity<UsersPage>,
}

type UsersTableDelegate = CrudTableDelegate<UserTableRow>;

impl UsersTable {
    pub(super) fn new(
        page: WeakEntity<UsersPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let action_page = page.clone();
        let load_page = page.clone();
        let delegate = CrudTableDelegate::new(Vec::new())
            .row_id(|row: &UserTableRow| format!("default-user-row-{}", row.source.id))
            .empty_title("暂无用户")
            .empty_description("点击右上角“创建用户”添加第一个用户")
            .action_column(
                Column::new("actions", "操作")
                    .width(px(184.))
                    .min_width(px(180.))
                    .max_width(px(220.))
                    .selectable(false),
                move |row: &UserTableRow, window, cx| {
                    UserTableRow::render_actions(row, action_page.clone(), window, cx)
                },
            )
            .on_load_more(move |_, cx| {
                _ = load_page.update(cx, UsersPage::load_next_page);
            });
        let state = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .sortable(false)
                .col_movable(true)
                .col_resizable(true)
                .col_selectable(false)
                .row_selectable(false)
        });
        Self { state, page }
    }

    pub(super) fn replace_rows(
        &mut self,
        users: Vec<UserResponse>,
        total: i64,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| {
            let delegate = state.delegate_mut();
            delegate.replace_rows(users.into_iter().map(UserTableRow::from).collect());
            delegate.set_total(usize::try_from(total.max(0)).unwrap_or(usize::MAX));
            delegate.set_loading(false);
            delegate.set_loading_more(false);
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn append_rows(
        &mut self,
        users: Vec<UserResponse>,
        total: i64,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| {
            let delegate = state.delegate_mut();
            let existing_ids = delegate
                .rows()
                .iter()
                .map(|row| row.source.id.clone())
                .collect::<BTreeSet<_>>();
            delegate.append_rows(
                users
                    .into_iter()
                    .filter(|user| !existing_ids.contains(&user.id))
                    .map(UserTableRow::from),
            );
            delegate.set_total(usize::try_from(total.max(0)).unwrap_or(usize::MAX));
            delegate.set_loading(false);
            delegate.set_loading_more(false);
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn update_user(&mut self, updated: UserResponse, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            if let Some(row) = state
                .delegate_mut()
                .rows_mut()
                .iter_mut()
                .find(|row| row.source.id == updated.id)
            {
                *row = UserTableRow::from(updated);
                cx.notify();
            }
        });
    }

    pub(super) fn len(&self, cx: &App) -> usize {
        self.state.read(cx).delegate().rows().len()
    }

    pub(super) fn refresh(&self, cx: &mut Context<Self>) {
        let page_loading = self
            .page
            .upgrade()
            .is_some_and(|page| page.read(cx).is_loading());
        self.state.update(cx, |state, cx| {
            let delegate = state.delegate_mut();
            delegate.set_loading(page_loading && delegate.rows().is_empty());
            delegate.set_loading_more(page_loading);
            cx.notify();
        });
        cx.notify();
    }
}

impl Render for UsersTable {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w_full().flex_1().min_h_0().child(
            Card::new().size_full().overflow_hidden().child(
                DataTable::new(&self.state)
                    .stripe(true)
                    .bordered(true)
                    .with_size(px(USER_TABLE_ROW_HEIGHT)),
            ),
        )
    }
}

#[derive(Clone, nexora::CrudTableRow)]
struct UserTableRow {
    #[nexora(skip)]
    source: UserResponse,
    #[nexora(column(
        key = "user",
        name = "用户",
        width = 340.,
        min_width = 280.,
        max_width = 520.,
        render = Self::render_user,
        text = Self::display_name_text
    ))]
    display_name: String,
    #[nexora(column(
        key = "type",
        name = "类型",
        width = 96.,
        min_width = 84.,
        max_width = 120.,
        align = "center",
        render = Self::render_user_type,
        text = Self::user_type_text,
        resizable = false
    ))]
    user_type: UserType,
    #[nexora(column(
        key = "username",
        name = "登录用户名",
        width = 160.,
        min_width = 120.,
        max_width = 240.,
        align = "center",
        render = Self::render_username
    ))]
    username: String,
    #[nexora(column(
        key = "email",
        name = "邮箱",
        width = 260.,
        min_width = 180.,
        max_width = 360.,
        align = "center",
        render = Self::render_email
    ))]
    email: String,
    #[nexora(column(
        key = "status",
        name = "状态",
        width = 76.,
        min_width = 76.,
        max_width = 76.,
        align = "center",
        render = Self::render_status,
        text = Self::status_text,
        resizable = false
    ))]
    status: UserStatus,
}

impl From<UserResponse> for UserTableRow {
    fn from(user: UserResponse) -> Self {
        Self {
            display_name: user.display_name.clone(),
            user_type: user.user_type,
            username: user.username.clone().unwrap_or_else(|| "未绑定".to_owned()),
            email: user.email.clone().unwrap_or_else(|| "—".to_owned()),
            status: user.status,
            source: user,
        }
    }
}

impl UserTableRow {
    fn display_name_text(row: &Self, _cx: &App) -> String {
        row.display_name.clone()
    }

    fn user_type_text(row: &Self, _cx: &App) -> String {
        match row.user_type {
            UserType::Human => "人员".to_owned(),
            UserType::ServiceAccount => "服务账号".to_owned(),
        }
    }

    fn status_text(row: &Self, _cx: &App) -> String {
        match row.status {
            UserStatus::Active => "已启用".to_owned(),
            UserStatus::Suspended => "已停用".to_owned(),
        }
    }

    fn render_user(row: &Self, _window: &mut Window, cx: &mut App) -> TableCell {
        let user = &row.source;
        let avatar = Avatar::new().name(user.display_name.clone()).small();
        let avatar = if let Some(avatar_url) = user.avatar_url.clone() {
            avatar.src(avatar_url)
        } else {
            avatar
        };

        TableCell::new(
            h_flex().h_full().min_w_0().gap_2().child(avatar).child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_1()
                            .child(div().min_w_0().truncate().child(user.display_name.clone()))
                            .when(user.is_super_admin, |this| {
                                this.child(Tag::info().small().rounded_full().child("超级管理员"))
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(user.id.clone()),
                    ),
            ),
        )
    }

    fn render_user_type(row: &Self, _window: &mut Window, _cx: &mut App) -> TableCell {
        let tag = match row.user_type {
            UserType::Human => Tag::secondary().small().rounded_full().child("人员"),
            UserType::ServiceAccount => Tag::new().small().rounded_full().child("服务账号"),
        };
        TableCell::new(tag).center()
    }

    fn render_username(row: &Self, _window: &mut Window, _cx: &mut App) -> TableCell {
        TableCell::new(div().min_w_0().truncate().child(row.username.clone())).center()
    }

    fn render_email(row: &Self, _window: &mut Window, _cx: &mut App) -> TableCell {
        TableCell::new(div().min_w_0().truncate().child(row.email.clone())).center()
    }

    fn render_status(row: &Self, _window: &mut Window, _cx: &mut App) -> TableCell {
        let tag = match row.status {
            UserStatus::Active => Tag::success().small().rounded_full().child("已启用"),
            UserStatus::Suspended => Tag::warning().small().rounded_full().child("已停用"),
        };
        TableCell::new(tag).center()
    }

    fn user_is_service_account(user: &UserResponse) -> bool {
        user.user_type == UserType::ServiceAccount
    }

    fn render_actions(
        row: &Self,
        page: WeakEntity<UsersPage>,
        _window: &mut Window,
        cx: &mut App,
    ) -> TableCell {
        let user = &row.source;
        let role_user_id = user.id.clone();
        let status_user_id = user.id.clone();
        let role_page = page.clone();
        let status_page = page.clone();
        let mutation_busy = page
            .upgrade()
            .is_some_and(|page| page.read(cx).has_active_mutation(cx));
        let current_user_busy = page
            .upgrade()
            .is_some_and(|page| page.read(cx).is_user_busy(user.id.as_str()));
        let can_manage_roles =
            has_permission(cx, "users:roles.write") && has_permission(cx, "roles:read");
        let can_change_status = has_permission(cx, "users:status.write");
        let is_service_account = Self::user_is_service_account(user);
        let is_active = user.status == UserStatus::Active;
        let status_action = if is_active { "停用" } else { "启用" };
        let target_status = if is_active {
            UserStatus::Suspended
        } else {
            UserStatus::Active
        };

        let role_tooltip = if is_service_account {
            "服务账号不能在这里操作"
        } else if user.is_super_admin {
            "超级管理员不能修改角色"
        } else if can_manage_roles {
            "管理用户角色"
        } else {
            "当前账号不能管理角色"
        };
        let status_tooltip = if is_service_account {
            "服务账号不能在这里操作"
        } else if user.is_super_admin {
            "超级管理员不能修改状态"
        } else if can_change_status {
            status_action
        } else {
            "当前账号不能修改状态"
        };

        TableCell::new(
            h_flex()
                .gap_2()
                .child(
                    Button::new(format!("default-user-roles-{role_user_id}"))
                        .small()
                        .label("管理角色")
                        .disabled(
                            is_service_account
                                || user.is_super_admin
                                || mutation_busy
                                || !can_manage_roles,
                        )
                        .tooltip(role_tooltip)
                        .on_click(move |_, window, cx| {
                            _ = role_page.update(cx, |page, cx| {
                                page.manage_roles(role_user_id.clone(), window, cx);
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
                            is_service_account
                                || user.is_super_admin
                                || mutation_busy
                                || !can_change_status,
                        )
                        .tooltip(status_tooltip)
                        .on_click(move |_, _, cx| {
                            _ = status_page.update(cx, |page, cx| {
                                page.set_user_status(status_user_id.clone(), target_status, cx);
                            });
                        }),
                ),
        )
        .center()
    }
}
