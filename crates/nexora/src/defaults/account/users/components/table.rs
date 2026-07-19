//! 默认用户列表数据表。

use std::collections::BTreeSet;

use gpui::{
    AnyElement, App, Context, Div, Entity, IntoElement, Render, Stateful, WeakEntity, Window, div,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    avatar::Avatar,
    button::Button,
    h_flex,
    table::{Column, DataTable, TableDelegate, TableState},
    tag::Tag,
    v_flex,
};

use crate::{
    defaults::account::has_permission,
    desktop::contract::{UserResponse, UserStatus},
};
use ui::{Card, TableHeaderCell};

use super::UsersPage;

const USER_TABLE_ROW_HEIGHT: f32 = 52.0;

pub(in crate::defaults::account::users) struct UsersTable {
    state: Entity<TableState<UsersTableDelegate>>,
}

impl UsersTable {
    pub(super) fn new(
        page: WeakEntity<UsersPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let delegate = UsersTableDelegate::new(page);
        let state = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .sortable(false)
                .col_movable(true)
                .col_resizable(true)
                .col_selectable(false)
                .row_selectable(false)
        });
        Self { state }
    }

    pub(super) fn replace_rows(
        &mut self,
        users: Vec<UserResponse>,
        total: i64,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| {
            state.delegate_mut().users = users;
            state.delegate_mut().total = usize::try_from(total.max(0)).unwrap_or(usize::MAX);
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
                .users
                .iter()
                .map(|user| user.id.clone())
                .collect::<BTreeSet<_>>();
            delegate.users.extend(
                users
                    .into_iter()
                    .filter(|user| !existing_ids.contains(&user.id)),
            );
            delegate.total = usize::try_from(total.max(0)).unwrap_or(usize::MAX);
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn update_user(&mut self, updated: UserResponse, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            if let Some(user) = state
                .delegate_mut()
                .users
                .iter_mut()
                .find(|user| user.id == updated.id)
            {
                *user = updated;
                cx.notify();
            }
        });
    }

    pub(super) fn len(&self, cx: &App) -> usize {
        self.state.read(cx).delegate().users.len()
    }

    pub(super) fn refresh(&self, cx: &mut Context<Self>) {
        self.state.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
}

impl Render for UsersTable {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w_full().flex_1().min_h_0().child(
            Card::new().size_full().overflow_hidden().child(
                DataTable::new(&self.state)
                    .stripe(true)
                    .bordered(false)
                    .with_size(px(USER_TABLE_ROW_HEIGHT)),
            ),
        )
    }
}

struct UsersTableDelegate {
    columns: Vec<Column>,
    users: Vec<UserResponse>,
    total: usize,
    page: WeakEntity<UsersPage>,
}

impl UsersTableDelegate {
    fn new(page: WeakEntity<UsersPage>) -> Self {
        Self {
            columns: vec![
                Column::new("user", "用户")
                    .width(px(360.))
                    .min_width(px(280.))
                    .max_width(px(560.)),
                Column::new("username", "登录用户名")
                    .width(px(160.))
                    .min_width(px(120.))
                    .max_width(px(240.)),
                Column::new("email", "邮箱")
                    .width(px(260.))
                    .min_width(px(180.))
                    .max_width(px(360.)),
                Column::new("status", "状态")
                    .width(px(76.))
                    .min_width(px(76.))
                    .max_width(px(76.))
                    .resizable(false),
                Column::new("actions", "操作")
                    .width(px(184.))
                    .min_width(px(180.))
                    .max_width(px(220.))
                    .selectable(false),
            ],
            users: Vec::new(),
            total: 0,
            page,
        }
    }

    fn render_user(&self, user: &UserResponse, cx: &App) -> AnyElement {
        let avatar = Avatar::new().name(user.display_name.clone()).small();
        let avatar = if let Some(avatar_url) = user.avatar_url.clone() {
            avatar.src(avatar_url)
        } else {
            avatar
        };
        h_flex()
            .h_full()
            .min_w_0()
            .gap_2()
            .child(avatar)
            .child(
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
            )
            .into_any_element()
    }

    fn render_actions(&self, user: &UserResponse, cx: &mut App) -> AnyElement {
        let role_user_id = user.id.clone();
        let status_user_id = user.id.clone();
        let role_page = self.page.clone();
        let status_page = self.page.clone();
        let mutation_busy = self
            .page
            .upgrade()
            .is_some_and(|page| page.read(cx).has_active_mutation(cx));
        let current_user_busy = self
            .page
            .upgrade()
            .is_some_and(|page| page.read(cx).is_user_busy(user.id.as_str()));
        let can_manage_roles =
            has_permission(cx, "users:roles.write") && has_permission(cx, "roles:read");
        let can_change_status = has_permission(cx, "users:status.write");
        let is_active = user.status == UserStatus::Active;
        let status_action = if is_active { "停用" } else { "启用" };
        let target_status = if is_active {
            UserStatus::Suspended
        } else {
            UserStatus::Active
        };

        h_flex()
            .h_full()
            .gap_2()
            .child(
                Button::new(format!("default-user-roles-{role_user_id}"))
                    .small()
                    .label("管理角色")
                    .disabled(user.is_super_admin || mutation_busy || !can_manage_roles)
                    .tooltip(if can_manage_roles {
                        "完整替换用户的直接角色集合"
                    } else {
                        "需要 users:roles.write 与 roles:read 权限"
                    })
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
                    .disabled(user.is_super_admin || mutation_busy || !can_change_status)
                    .tooltip(if can_change_status {
                        status_action
                    } else {
                        "需要 users:status.write 权限"
                    })
                    .on_click(move |_, _, cx| {
                        _ = status_page.update(cx, |page, cx| {
                            page.set_user_status(status_user_id.clone(), target_status, cx);
                        });
                    }),
            )
            .into_any_element()
    }
}

impl TableDelegate for UsersTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.users.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        TableHeaderCell::new(self.column(col_ix, cx).name)
    }

    fn move_column(
        &mut self,
        col_ix: usize,
        to_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        if col_ix >= self.columns.len() || to_ix >= self.columns.len() || col_ix == to_ix {
            return;
        }
        let column = self.columns.remove(col_ix);
        self.columns.insert(to_ix, column);
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let id = self
            .users
            .get(row_ix)
            .map(|user| format!("default-user-row-{}", user.id))
            .unwrap_or_else(|| format!("default-user-row-missing-{row_ix}"));
        div().id(id)
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(user) = self.users.get(row_ix).cloned() else {
            return div().into_any_element();
        };
        match self.columns[col_ix].key.as_ref() {
            "user" => self.render_user(&user, cx),
            "username" => user
                .username
                .clone()
                .unwrap_or_else(|| "未绑定".to_owned())
                .into_any_element(),
            "email" => user
                .email
                .clone()
                .unwrap_or_else(|| "—".to_owned())
                .into_any_element(),
            "status" => match user.status {
                UserStatus::Active => Tag::success()
                    .small()
                    .rounded_full()
                    .child("已启用")
                    .into_any_element(),
                UserStatus::Suspended => Tag::warning()
                    .small()
                    .rounded_full()
                    .child("已停用")
                    .into_any_element(),
            },
            "actions" => self.render_actions(&user, cx),
            _ => div().into_any_element(),
        }
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_1()
            .text_color(cx.theme().muted_foreground)
            .child("暂无用户")
            .child(div().text_xs().child("点击右上角“创建用户”添加第一个用户"))
    }

    fn loading(&self, cx: &App) -> bool {
        self.users.is_empty()
            && self
                .page
                .upgrade()
                .is_some_and(|page| page.read(cx).is_loading())
    }

    fn has_more(&self, cx: &App) -> bool {
        self.users.len() < self.total
            && self
                .page
                .upgrade()
                .is_some_and(|page| !page.read(cx).is_loading())
    }

    fn load_more(&mut self, _window: &mut Window, cx: &mut Context<TableState<Self>>) {
        _ = self.page.update(cx, UsersPage::load_next_page);
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _cx: &App) -> String {
        let Some(user) = self.users.get(row_ix) else {
            return String::new();
        };
        match col_ix {
            0 => user.display_name.clone(),
            1 => user.username.clone().unwrap_or_default(),
            2 => user.email.clone().unwrap_or_default(),
            3 => match user.status {
                UserStatus::Active => "已启用".to_owned(),
                UserStatus::Suspended => "已停用".to_owned(),
            },
            _ => String::new(),
        }
    }
}
