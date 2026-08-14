//! 默认用户列表数据表。

use gpui::{App, Context, Entity, WeakEntity, Window, div, prelude::*, px};
use gpui_component::{
    Disableable as _, Sizable as _, avatar::Avatar, button::Button, clipboard::Clipboard, h_flex,
    table::Column, tag::Tag,
};
use serde::{Deserialize, Serialize};

use crate::{
    defaults::account::has_permission,
    desktop::{
        CrudListState, CrudLoadError, CrudPage, api_session,
        contract::{UserListQuery, UserResponse, UserStatus, UserType},
    },
};
use ui::{CrudTableDelegate, TableCell};

use super::UsersPage;

pub(in crate::defaults::account::users) struct UsersTable {
    state: Entity<CrudListState<UserTableRow, UserQuery>>,
}

impl UsersTable {
    pub(super) fn new(
        page: WeakEntity<UsersPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let action_page = page.clone();
        let delegate = CrudTableDelegate::new(Vec::new())
            .empty_title("暂无用户")
            .empty_description("点击右上角按钮添加第一个人员或服务账号")
            .action_column(
                Column::new("actions", "操作")
                    .width(px(284.))
                    .min_width(px(180.))
                    .max_width(px(320.))
                    .selectable(false),
                move |row: &UserTableRow, window, cx| {
                    UserTableRow::render_actions(row, action_page.clone(), window, cx)
                },
            );
        let state = CrudListState::create_with_delegate(
            UserQuery::default(),
            |query, cx| {
                let session = api_session(cx);
                let task = session.map(|session| {
                    cx.background_spawn(async move { session.list_users_filtered(query.to_api()) })
                });
                async move {
                    let Some(task) = task else {
                        return Err(CrudLoadError::terminal("当前登录会话不可用，请重新登录"));
                    };
                    let response = task
                        .await
                        .map_err(|error| CrudLoadError::retryable(error.user_message()))?;
                    let items = response
                        .items
                        .into_iter()
                        .map(UserTableRow::from)
                        .collect::<Vec<_>>();
                    Ok(CrudPage::new(
                        items,
                        response.page.number,
                        response.page.size,
                        usize::try_from(response.page.total.max(0)).unwrap_or(usize::MAX),
                    ))
                }
            },
            move |_| delegate,
            false,
            window,
            cx,
        )
        .expect("默认用户 CRUD 列表状态应当合法");
        Self { state }
    }

    pub(super) fn loaded_len(&self, cx: &App) -> usize {
        self.state.read(cx).current_rows().len()
    }

    pub(super) fn visible_len(&self, cx: &App) -> usize {
        self.state.read(cx).current_rows().len()
    }

    pub(super) fn total(&self, cx: &App) -> usize {
        self.state.read(cx).total()
    }

    pub(super) fn is_loading(&self, cx: &App) -> bool {
        self.state.read(cx).is_loading()
    }

    pub(super) fn load_if_needed(&self, cx: &mut Context<Self>) {
        if !self.state.read(cx).loaded_once() {
            self.state.update(cx, CrudListState::load_current);
        }
    }

    pub(super) fn refresh(&self, cx: &mut Context<Self>) {
        self.state.update(cx, CrudListState::refresh_current);
    }

    pub(super) fn set_query(&self, query: UserQuery, cx: &mut Context<Self>) {
        _ = self
            .state
            .update(cx, |state, cx| state.set_query(query, cx));
        self.state.update(cx, CrudListState::load_current);
    }

    pub(super) fn query(&self, cx: &App) -> UserQuery {
        self.state.read(cx).query().clone()
    }

    pub(super) fn state(&self) -> Entity<CrudListState<UserTableRow, UserQuery>> {
        self.state.clone()
    }

    pub(super) fn refresh_actions(&self, cx: &mut Context<Self>) {
        self.state.update(cx, |_, cx| cx.notify());
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, nexora::CrudQuery)]
#[nexora(page_size(default = 25, min = 15, max = 100, options = [15, 25, 50, 100]))]
pub(super) struct UserQuery {
    #[nexora(pagination)]
    #[serde(flatten)]
    pub(super) page: contracts::pagination::PageQuery,
    #[nexora(filter(label = "关键词", control = "input", trigger = "manual"))]
    pub(super) keyword: Option<String>,
    #[nexora(filter(label = "状态", control = "select", trigger = "immediate"))]
    pub(super) status: Option<UserStatus>,
    #[nexora(filter(label = "类型", control = "select", trigger = "immediate"))]
    pub(super) user_type: Option<UserType>,
}

impl UserQuery {
    fn to_api(&self) -> UserListQuery {
        UserListQuery {
            page: self.page,
            keyword: self.keyword.clone(),
            status: self.status,
            user_type: self.user_type,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum UserStatusFilter {
    #[default]
    All,
    Active,
    Suspended,
}

impl UserStatusFilter {
    pub(super) const ALL: [Self; 3] = [Self::All, Self::Active, Self::Suspended];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::All => "全部状态",
            Self::Active => "已启用",
            Self::Suspended => "已停用",
        }
    }

    pub(super) const fn value(self) -> Option<UserStatus> {
        match self {
            Self::All => None,
            Self::Active => Some(UserStatus::Active),
            Self::Suspended => Some(UserStatus::Suspended),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum UserTypeFilter {
    #[default]
    All,
    Human,
    ServiceAccount,
}

impl UserTypeFilter {
    pub(super) const ALL: [Self; 3] = [Self::All, Self::Human, Self::ServiceAccount];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::All => "全部类型",
            Self::Human => "人员",
            Self::ServiceAccount => "服务账号",
        }
    }

    pub(super) const fn value(self) -> Option<UserType> {
        match self {
            Self::All => None,
            Self::Human => Some(UserType::Human),
            Self::ServiceAccount => Some(UserType::ServiceAccount),
        }
    }
}

#[derive(Clone, nexora::CrudTableRow)]
pub(super) struct UserTableRow {
    #[nexora(column(
        key = "avatar",
        name = "头像",
        width = 64.,
        min_width = 64.,
        max_width = 64.,
        align = "center",
        render = Self::render_avatar
    ))]
    avatar_name: String,
    #[nexora(
        row_id,
        column(
            key = "id",
            name = "用户 ID",
            width = 220.,
            min_width = 180.,
            max_width = 300.,
            render = Self::render_id
        )
    )]
    id: String,
    #[nexora(skip)]
    source: UserResponse,
    #[nexora(column(
        key = "display_name",
            name = "名称",
        width = 180.,
        min_width = 140.,
        max_width = 260.,
        render = Self::render_display_name,
        text = Self::display_name_text
    ))]
    display_name: String,
    #[nexora(column(
        key = "identity",
        name = "身份",
        width = 112.,
        min_width = 96.,
        max_width = 140.,
        align = "center",
        render = Self::render_identity,
        text = Self::identity_text
    ))]
    is_super_admin: bool,
    #[nexora(column(
        key = "type",
        name = "类型",
        width = 96.,
        min_width = 84.,
        max_width = 120.,
        align = "center",
        render = Self::render_user_type,
        text = Self::user_type_text
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
        status,
        render = Self::render_status,
        text = Self::status_text
    ))]
    status: UserStatus,
}

impl From<UserResponse> for UserTableRow {
    fn from(user: UserResponse) -> Self {
        Self {
            avatar_name: user.display_name.clone(),
            id: user.id.clone(),
            display_name: user.display_name.clone(),
            is_super_admin: user.is_super_admin,
            user_type: user.user_type,
            username: user.username.clone().unwrap_or_else(|| "未绑定".to_owned()),
            email: user.email.clone().unwrap_or_else(|| "—".to_owned()),
            status: user.status,
            source: user,
        }
    }
}

impl UserTableRow {
    fn identity_text(row: &Self, _cx: &App) -> String {
        if row.is_super_admin {
            "超级管理员".to_owned()
        } else {
            "普通用户".to_owned()
        }
    }

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

    fn render_avatar(row: &Self, _window: &mut Window, _cx: &mut App) -> TableCell {
        TableCell::new(Avatar::new().name(row.avatar_name.clone()).small()).center()
    }

    fn render_id(row: &Self, _window: &mut Window, _cx: &mut App) -> TableCell {
        TableCell::new(
            h_flex()
                .min_w_0()
                .gap_1()
                .child(div().min_w_0().truncate().child(row.id.clone()))
                .child(
                    Clipboard::new(format!("copy-default-user-id-{}", row.id))
                        .value(row.id.clone())
                        .tooltip("复制用户 ID"),
                ),
        )
    }

    fn render_display_name(row: &Self, _window: &mut Window, _cx: &mut App) -> TableCell {
        TableCell::new(div().min_w_0().truncate().child(row.display_name.clone()))
    }

    fn render_identity(row: &Self, _window: &mut Window, _cx: &mut App) -> TableCell {
        let tag = if row.is_super_admin {
            Tag::info().small().rounded_full().child("超级管理员")
        } else {
            Tag::secondary().small().rounded_full().child("普通用户")
        };
        TableCell::new(tag).center()
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
        let credentials_user = user.clone();
        let role_page = page.clone();
        let status_page = page.clone();
        let credentials_page = page.clone();
        let mutation_busy = page
            .upgrade()
            .is_some_and(|page| page.read(cx).has_active_mutation(cx));
        let current_user_busy = page
            .upgrade()
            .is_some_and(|page| page.read(cx).is_user_busy(user.id.as_str()));
        let can_manage_roles =
            has_permission(cx, "users:roles.write") && has_permission(cx, "roles:read");
        let can_change_status = has_permission(cx, "users:status.write");
        let can_manage_credentials = has_permission(cx, "service_accounts:credentials.read")
            || has_permission(cx, "service_accounts:profile.write");
        let is_service_account = Self::user_is_service_account(user);
        let is_active = user.status == UserStatus::Active;
        let status_action = if is_active { "停用" } else { "启用" };
        let target_status = if is_active {
            UserStatus::Suspended
        } else {
            UserStatus::Active
        };

        let role_tooltip = if user.is_super_admin {
            "超级管理员不能修改角色"
        } else if can_manage_roles {
            "管理账号角色"
        } else {
            "当前账号不能管理角色"
        };
        let status_tooltip = if user.is_super_admin {
            "超级管理员不能修改状态"
        } else if can_change_status {
            status_action
        } else {
            "当前账号不能修改状态"
        };
        let component_size = theme::component_size(cx);

        TableCell::new(
            h_flex()
                .gap_2()
                .child(
                    Button::new(format!("default-user-roles-{role_user_id}"))
                        .with_size(component_size)
                        .label("管理角色")
                        .disabled(user.is_super_admin || mutation_busy || !can_manage_roles)
                        .tooltip(role_tooltip)
                        .on_click(move |_, window, cx| {
                            _ = role_page.update(cx, |page, cx| {
                                page.manage_roles(role_user_id.clone(), window, cx);
                            });
                        }),
                )
                .child(
                    Button::new(format!("default-user-status-{status_user_id}"))
                        .with_size(component_size)
                        .outline()
                        .label(status_action)
                        .loading(current_user_busy)
                        .disabled(user.is_super_admin || mutation_busy || !can_change_status)
                        .tooltip(status_tooltip)
                        .on_click(move |_, _, cx| {
                            _ = status_page.update(cx, |page, cx| {
                                page.set_user_status(status_user_id.clone(), target_status, cx);
                            });
                        }),
                )
                .when(is_service_account, |this| {
                    this.child(
                        Button::new(format!("default-service-account-manage-{}", user.id))
                            .with_size(component_size)
                            .outline()
                            .label("账号与凭据")
                            .disabled(mutation_busy || !can_manage_credentials)
                            .tooltip(if can_manage_credentials {
                                "管理服务账号资料与凭据"
                            } else {
                                "当前账号不能查看服务账号凭据"
                            })
                            .on_click(move |_, window, cx| {
                                _ = credentials_page.update(cx, |page, cx| {
                                    page.manage_service_account(
                                        credentials_user.clone(),
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    )
                }),
        )
        .center()
    }
}
