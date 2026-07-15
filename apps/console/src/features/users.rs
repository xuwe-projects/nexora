//! Console 用户管理功能。

use std::collections::BTreeSet;

use contracts::account::{
    AccessProfileResponse, ReplaceUserRolesRequest, RoleResponse, UpdateUserStatusRequest,
    UserResponse, UserStatus,
};
use gpui::{AnyElement, Context, IntoElement as _, Render, Task, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    pagination::Pagination,
    spinner::Spinner,
    table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow},
    v_flex,
};

use crate::{
    account_api::{AccountApi, AccountApiError},
    auth,
};

const PAGE_SIZE: u32 = 25;

struct UserRoleEditor {
    profile: AccessProfileResponse,
    selected_role_ids: BTreeSet<i64>,
}

/// 用户管理页面状态与异步生命周期。
pub struct UsersFeature {
    users: Vec<UserResponse>,
    roles: Vec<RoleResponse>,
    page: u32,
    total: i64,
    loaded: bool,
    loading: bool,
    busy_user_id: Option<String>,
    role_editor: Option<UserRoleEditor>,
    error: Option<String>,
    _load_task: Option<Task<()>>,
    _mutation_task: Option<Task<()>>,
}

impl Default for UsersFeature {
    fn default() -> Self {
        Self {
            users: Vec::new(),
            roles: Vec::new(),
            page: 1,
            total: 0,
            loaded: false,
            loading: false,
            busy_user_id: None,
            role_editor: None,
            error: None,
            _load_task: None,
            _mutation_task: None,
        }
    }
}

impl UsersFeature {
    /// 创建尚未发起网络请求的用户管理页面。
    pub fn new() -> Self {
        Self::default()
    }

    /// 页面首次进入时加载用户页和可分配角色目录。
    pub fn load_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            self.load_page(1, cx);
        }
    }

    fn load_page(&mut self, page: u32, cx: &mut Context<Self>) {
        let Some(session) = auth::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        let background = cx.background_spawn(async move {
            let api = AccountApi::new(session)?;
            let users = api.list_users(page, PAGE_SIZE)?;
            let roles = api.list_roles()?;
            Ok::<_, AccountApiError>((users, roles))
        });
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok((users, roles)) => {
                        this.page = users.page.number;
                        this.total = users.page.total;
                        this.users = users.items;
                        this.roles = roles;
                        this.loaded = true;
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn load_user_roles(&mut self, user_id: String, cx: &mut Context<Self>) {
        let Some(session) = auth::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.busy_user_id = Some(user_id.clone());
        self.error = None;
        let background = cx
            .background_spawn(async move { AccountApi::new(session)?.get_user(user_id.as_str()) });
        self._mutation_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.busy_user_id = None;
                match result {
                    Ok(profile) => {
                        let selected_role_ids = profile.roles.iter().map(|role| role.id).collect();
                        this.role_editor = Some(UserRoleEditor {
                            profile,
                            selected_role_ids,
                        });
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn set_user_status(&mut self, user_id: String, status: UserStatus, cx: &mut Context<Self>) {
        let Some(session) = auth::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.busy_user_id = Some(user_id.clone());
        self.error = None;
        let background = cx.background_spawn(async move {
            AccountApi::new(session)?
                .update_user_status(user_id.as_str(), &UpdateUserStatusRequest { status })
        });
        self._mutation_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.busy_user_id = None;
                match result {
                    Ok(updated) => {
                        if let Some(user) = this.users.iter_mut().find(|user| user.id == updated.id)
                        {
                            *user = updated;
                        }
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn save_user_roles(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.role_editor else {
            return;
        };
        let Some(session) = auth::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let user_id = editor.profile.user.id.clone();
        let request = ReplaceUserRolesRequest {
            role_ids: editor.selected_role_ids.iter().copied().collect(),
        };
        self.busy_user_id = Some(user_id.clone());
        self.error = None;
        let background = cx.background_spawn(async move {
            AccountApi::new(session)?.replace_user_roles(user_id.as_str(), &request)
        });
        self._mutation_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.busy_user_id = None;
                match result {
                    Ok(profile) => {
                        let selected_role_ids = profile.roles.iter().map(|role| role.id).collect();
                        this.role_editor = Some(UserRoleEditor {
                            profile,
                            selected_role_ids,
                        });
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn toggle_role(&mut self, role_id: i64, checked: bool, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.role_editor else {
            return;
        };
        if checked {
            editor.selected_role_ids.insert(role_id);
        } else {
            editor.selected_role_ids.remove(&role_id);
        }
        cx.notify();
    }

    fn render_user_table(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.users.iter().map(|user| {
            let user_id = user.id.clone();
            let status_user_id = user.id.clone();
            let busy = self.busy_user_id.is_some();
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
                                .child(user.id.clone()),
                        ),
                    ),
                )
                .child(TableCell::new().child(user.email.clone().unwrap_or_else(|| "—".to_owned())))
                .child(TableCell::new().w(px(88.)).child(status_label))
                .child(
                    TableCell::new().w(px(210.)).child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new(format!("user-roles-{user_id}"))
                                    .small()
                                    .label("管理角色")
                                    .disabled(user.is_super_admin || busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.load_user_roles(user_id.clone(), cx);
                                    })),
                            )
                            .child(
                                Button::new(format!("user-status-{status_user_id}"))
                                    .small()
                                    .outline()
                                    .label(status_action)
                                    .disabled(user.is_super_admin || busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_user_status(
                                            status_user_id.clone(),
                                            target_status,
                                            cx,
                                        );
                                    })),
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
                        .child(TableHead::new().w(px(210.)).child("操作")),
                ),
            )
            .child(TableBody::new().children(rows))
            .into_any_element()
    }

    fn render_role_editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.role_editor.as_ref()?;
        let busy = self.busy_user_id.is_some();
        let role_options = self.roles.iter().map(|role| {
            let role_id = role.id;
            Checkbox::new(format!("assign-role-{role_id}"))
                .label(format!("{}（{}）", role.name, role.key))
                .checked(editor.selected_role_ids.contains(&role_id))
                .disabled(busy)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_role(role_id, *checked, cx);
                }))
        });

        Some(
            v_flex()
                .gap_3()
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().tokens.group_box)
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            v_flex()
                                .gap_1()
                                .child(div().font_semibold().child(format!(
                                    "为 {} 分配角色",
                                    editor.profile.user.display_name
                                )))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("保存时会完整替换该用户的直接角色集合。"),
                                ),
                        )
                        .child(
                            Button::new("close-user-role-editor")
                                .ghost()
                                .small()
                                .icon(IconName::Close)
                                .tooltip("关闭角色分配")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.role_editor = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(v_flex().gap_2().children(role_options))
                .child(
                    h_flex().justify_end().child(
                        Button::new("save-user-roles")
                            .primary()
                            .label("保存角色")
                            .loading(busy)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_user_roles(cx);
                            })),
                    ),
                )
                .into_any_element(),
        )
    }
}

impl Render for UsersFeature {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let total_pages = usize::try_from(
            self.total.max(0).saturating_add(i64::from(PAGE_SIZE) - 1) / i64::from(PAGE_SIZE),
        )
        .unwrap_or(1)
        .max(1);
        let page = self.page;

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
                            .child(div().text_xl().font_bold().child("用户管理"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("共 {} 个本地用户", self.total.max(0))),
                            ),
                    )
                    .child(
                        Button::new("refresh-users")
                            .outline()
                            .icon(IconName::Loader)
                            .label("刷新")
                            .disabled(self.loading)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.load_page(page, cx);
                            })),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("users-error", error).title("用户操作失败"))
            })
            .when(self.loading && self.users.is_empty(), |this| {
                this.child(
                    h_flex()
                        .justify_center()
                        .gap_2()
                        .py_8()
                        .child(Spinner::new().small())
                        .child("正在加载用户..."),
                )
            })
            .when(!self.users.is_empty(), |this| {
                this.child(self.render_user_table(cx)).child(
                    h_flex().justify_end().child(
                        Pagination::new("users-pagination")
                            .current_page(self.page as usize)
                            .total_pages(total_pages)
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, page: &usize, _, cx| {
                                if let Ok(page) = u32::try_from(*page) {
                                    this.load_page(page, cx);
                                }
                            })),
                    ),
                )
            })
            .when(
                self.loaded && !self.loading && self.users.is_empty(),
                |this| this.child(Alert::info("users-empty", "当前系统还没有本地用户")),
            )
            .when_some(self.render_role_editor(cx), |this, editor| {
                this.child(editor)
            })
    }
}
