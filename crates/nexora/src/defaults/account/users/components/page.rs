//! 默认用户管理页面状态。

use gpui::{Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    h_flex,
    pagination::Pagination,
    spinner::Spinner,
    v_flex,
};

use crate::desktop::{
    AccountClientError, api_session,
    contract::{RoleResponse, UpdateUserStatusRequest, UserResponse, UserStatus},
};

use super::{ProvisionUserDialog, UserRoleEditor, UsersTable};
use crate::defaults::account::has_permission;

const PAGE_SIZE: u32 = 25;

pub(in crate::defaults::account::users) struct UsersPage {
    users: Vec<UserResponse>,
    roles: Vec<RoleResponse>,
    page: u32,
    total: i64,
    loaded: bool,
    loading: bool,
    busy_user_id: Option<String>,
    error: Option<String>,
    notice: Option<String>,
    role_editor: Entity<UserRoleEditor>,
    provision_dialog: Option<WeakEntity<ProvisionUserDialog>>,
    _role_editor_subscription: Subscription,
    _load_task: Option<Task<()>>,
    _mutation_task: Option<Task<()>>,
}

impl UsersPage {
    pub(in crate::defaults::account::users) fn new(cx: &mut Context<Self>) -> Self {
        let role_editor = cx.new(|_| UserRoleEditor::default());
        let role_editor_subscription = cx.observe(&role_editor, |_, _, cx| cx.notify());
        Self {
            users: Vec::new(),
            roles: Vec::new(),
            page: 1,
            total: 0,
            loaded: false,
            loading: false,
            busy_user_id: None,
            error: None,
            notice: None,
            role_editor,
            provision_dialog: None,
            _role_editor_subscription: role_editor_subscription,
            _load_task: None,
            _mutation_task: None,
        }
    }

    pub(in crate::defaults::account::users) fn set_provision_dialog(
        &mut self,
        dialog: WeakEntity<ProvisionUserDialog>,
        cx: &mut Context<Self>,
    ) {
        self.provision_dialog = Some(dialog);
        cx.notify();
    }

    pub(in crate::defaults::account::users) fn load_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            self.load_page(1, cx);
        }
    }

    pub(super) fn user_provisioned(&mut self, display_name: String, cx: &mut Context<Self>) {
        self.notice = Some(format!("用户“{display_name}”已开通"));
        self.load_page(1, cx);
    }

    fn load_page(&mut self, page: u32, cx: &mut Context<Self>) {
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        let can_read_roles = has_permission(cx, "roles:read");
        let background = cx.background_spawn(async move {
            let users = session.list_users(page, PAGE_SIZE)?;
            let roles = if can_read_roles {
                session.list_roles()?
            } else {
                Vec::new()
            };
            Ok::<_, AccountClientError>((users, roles))
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

    pub(super) fn open_provision_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.provision_dialog {
            let roles = self.roles.clone();
            _ = dialog.update(cx, |dialog, cx| dialog.open(roles, window, cx));
        }
    }

    pub(super) fn manage_roles(&mut self, user_id: String, cx: &mut Context<Self>) {
        if self.loading || self.role_editor.read(cx).is_busy() {
            return;
        }
        let roles = self.roles.clone();
        self.role_editor.update(cx, |editor, cx| {
            editor.open(user_id, roles, cx);
        });
    }

    pub(super) fn set_user_status(
        &mut self,
        user_id: String,
        status: UserStatus,
        cx: &mut Context<Self>,
    ) {
        if self.loading || self.busy_user_id.is_some() {
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.busy_user_id = Some(user_id.clone());
        self.error = None;
        self.notice = None;
        let background = cx.background_spawn(async move {
            session.update_user_status(user_id.as_str(), &UpdateUserStatusRequest { status })
        });
        self._mutation_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.busy_user_id = None;
                match result {
                    Ok(updated) => {
                        if let Some(user) = this.users.iter_mut().find(|user| user.id == updated.id)
                        {
                            *user = updated.clone();
                        }
                        let action = match updated.status {
                            UserStatus::Active => "启用",
                            UserStatus::Suspended => "停用",
                        };
                        this.notice = Some(format!("用户“{}”已{action}", updated.display_name));
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn total_pages(&self) -> usize {
        usize::try_from(
            self.total.max(0).saturating_add(i64::from(PAGE_SIZE) - 1) / i64::from(PAGE_SIZE),
        )
        .unwrap_or(1)
        .max(1)
    }
}

impl Render for UsersPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current_page = self.page;
        let can_provision = has_permission(cx, "users:provision");
        let can_change_status = has_permission(cx, "users:status.write");
        let can_manage_roles =
            has_permission(cx, "users:roles.write") && has_permission(cx, "roles:read");
        let role_editor_busy = self.role_editor.read(cx).is_busy();
        let table = UsersTable::new(
            self.users.clone(),
            self.busy_user_id.clone(),
            self.loading || role_editor_busy,
            can_change_status,
            can_manage_roles,
            cx.entity().downgrade(),
        );

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
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("open-default-account-user-dialog")
                                    .debug_selector(|| "open-default-account-user-dialog".into())
                                    .primary()
                                    .icon(IconName::Plus)
                                    .label("开通已确认身份")
                                    .disabled(self.loading || !can_provision)
                                    .tooltip(if can_provision {
                                        "把已由管理员确认的 OIDC 身份开通为本地用户"
                                    } else {
                                        "需要 users:provision 权限"
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_provision_dialog(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("refresh-default-account-users")
                                    .outline()
                                    .icon(IconName::Loader)
                                    .label("刷新")
                                    .loading(self.loading)
                                    .disabled(self.loading)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.load_page(current_page, cx);
                                    })),
                            ),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-account-users-error", error).title("用户操作失败"))
            })
            .when_some(self.notice.clone(), |this, notice| {
                this.child(Alert::success("default-account-users-notice", notice))
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
                this.child(table).child(
                    h_flex().justify_end().child(
                        Pagination::new("default-account-users-pagination")
                            .current_page(self.page as usize)
                            .total_pages(self.total_pages())
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
                |this| {
                    this.child(Alert::info(
                        "default-account-users-empty",
                        "当前系统还没有本地用户，可通过右上角开通第一个用户。",
                    ))
                },
            )
            .child(self.role_editor.clone())
    }
}
