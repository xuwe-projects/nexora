//! Account 桌面客户端自带的用户、角色与权限管理 Feature。

use gpui::{Context, IntoElement, Render, Task, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _,
    alert::Alert,
    button::Button,
    h_flex,
    spinner::Spinner,
    table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow},
    tag::Tag,
    v_flex,
};

use crate::{
    __private::FeatureRegistration,
    Feature, FeatureElement, FeatureInstance, FeatureMetadata, FeatureRuntimeError, NoPath,
    NoQuery, RouteMatch,
    account::client::contract::{PermissionResponse, RoleResponse, UserResponse},
};

const USERS_METADATA: FeatureMetadata = FeatureMetadata::new(
    "users",
    "用户管理",
    "/users",
    Some("访问控制"),
    Some("user"),
    None,
    900,
    true,
);
const ROLES_METADATA: FeatureMetadata = FeatureMetadata::new(
    "roles",
    "角色与权限",
    "/roles",
    Some("访问控制"),
    Some("asterisk"),
    None,
    910,
    true,
);

/// 返回 Account 默认管理页面的回退注册记录。
///
/// 应用只要声明相同稳定 ID 或路径的普通 `Feature`，注册表就会保留应用实现并跳过对应
/// 默认页面，因此不需要再引入专用派生宏。
pub(crate) const fn default_account_feature_registrations() -> [FeatureRegistration; 2] {
    [
        FeatureRegistration::new(USERS_METADATA, create_users_feature),
        FeatureRegistration::new(ROLES_METADATA, create_roles_feature),
    ]
}

#[derive(Default)]
struct DefaultUsersFeature {
    users: Vec<UserResponse>,
    page: u32,
    total: i64,
    loaded: bool,
    loading: bool,
    error: Option<String>,
    _task: Option<Task<()>>,
}

impl DefaultUsersFeature {
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(session) = crate::account::client::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        let background = cx.background_spawn(async move { session.list_users(1, 50) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(response) => {
                        this.users = response.items;
                        this.page = response.page.number;
                        this.total = response.page.total;
                        this.loaded = true;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Feature for DefaultUsersFeature {
    type Path = NoPath;
    type Query = NoQuery;

    const METADATA: FeatureMetadata = USERS_METADATA;
}

impl Render for DefaultUsersFeature {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        <Self as FeatureElement>::render(self, window, cx)
    }
}

impl FeatureElement for DefaultUsersFeature {
    fn activated(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            self.load(cx);
        }
    }

    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.users.iter().map(|user| {
            let name = if user.is_super_admin {
                format!("{} · 超级管理员", user.display_name)
            } else {
                user.display_name.clone()
            };
            let status = match user.status {
                crate::account::client::contract::UserStatus::Active => "已启用",
                crate::account::client::contract::UserStatus::Suspended => "已停用",
            };
            TableRow::new()
                .child(
                    TableCell::new().child(
                        v_flex().gap_1().child(name).child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(user.id.clone()),
                        ),
                    ),
                )
                .child(TableCell::new().child(user.email.clone().unwrap_or_else(|| "—".to_owned())))
                .child(TableCell::new().child(status))
                .child(TableCell::new().child(user.identity_id.clone()))
        });
        let table = Table::new()
            .child(
                TableHeader::new().child(
                    TableRow::new()
                        .child(TableHead::new().child("用户"))
                        .child(TableHead::new().child("邮箱"))
                        .child(TableHead::new().w(px(96.)).child("状态"))
                        .child(TableHead::new().child("Identity ID")),
                ),
            )
            .child(TableBody::new().children(rows));

        v_flex()
            .w_full()
            .gap_4()
            .p_5()
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_lg().font_semibold().child("用户管理"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "共 {} 个用户，第 {} 页",
                                        self.total, self.page
                                    )),
                            ),
                    )
                    .child(
                        Button::new("reload-default-account-users")
                            .outline()
                            .small()
                            .label("刷新")
                            .loading(self.loading)
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-account-users-error", error))
            })
            .when(self.loading && self.users.is_empty(), |this| {
                this.child(h_flex().justify_center().py_8().child(Spinner::new()))
            })
            .when(!self.users.is_empty(), |this| this.child(table))
    }
}

#[derive(Default)]
struct DefaultRolesFeature {
    roles: Vec<RoleResponse>,
    permissions: Vec<PermissionResponse>,
    loaded: bool,
    loading: bool,
    error: Option<String>,
    _task: Option<Task<()>>,
}

impl DefaultRolesFeature {
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(session) = crate::account::client::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        let background = cx.background_spawn(async move {
            let roles = session.list_roles()?;
            let permissions = session.list_permissions()?;
            Ok::<_, crate::account::client::AccountClientError>((roles, permissions))
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok((roles, permissions)) => {
                        this.roles = roles;
                        this.permissions = permissions;
                        this.loaded = true;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Feature for DefaultRolesFeature {
    type Path = NoPath;
    type Query = NoQuery;

    const METADATA: FeatureMetadata = ROLES_METADATA;
}

impl Render for DefaultRolesFeature {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        <Self as FeatureElement>::render(self, window, cx)
    }
}

impl FeatureElement for DefaultRolesFeature {
    fn activated(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            self.load(cx);
        }
    }

    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let roles = self.roles.iter().map(|role| {
            v_flex()
                .gap_3()
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .justify_between()
                        .child(div().font_semibold().child(role.name.clone()))
                        .child(Tag::new().child(role.key.clone())),
                )
                .when_some(role.description.clone(), |this, description| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    )
                })
                .child(
                    h_flex().gap_2().flex_wrap().children(
                        role.permissions
                            .iter()
                            .map(|permission| Tag::new().child(permission.key.clone())),
                    ),
                )
        });

        v_flex()
            .w_full()
            .gap_4()
            .p_5()
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_lg().font_semibold().child("角色与权限"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} 个角色，{} 项可分配权限",
                                        self.roles.len(),
                                        self.permissions.len()
                                    )),
                            ),
                    )
                    .child(
                        Button::new("reload-default-account-roles")
                            .outline()
                            .small()
                            .label("刷新")
                            .loading(self.loading)
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-account-roles-error", error))
            })
            .when(self.loading && self.roles.is_empty(), |this| {
                this.child(h_flex().justify_center().py_8().child(Spinner::new()))
            })
            .child(v_flex().gap_3().children(roles))
    }
}

fn create_users_feature(
    route: RouteMatch,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Result<FeatureInstance, FeatureRuntimeError> {
    crate::__private::create_feature::<DefaultUsersFeature>(route, window, cx, |_, _| {
        DefaultUsersFeature::default()
    })
}

fn create_roles_feature(
    route: RouteMatch,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Result<FeatureInstance, FeatureRuntimeError> {
    crate::__private::create_feature::<DefaultRolesFeature>(route, window, cx, |_, _| {
        DefaultRolesFeature::default()
    })
}
