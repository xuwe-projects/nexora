//! 用户开通对话框。

use std::collections::BTreeSet;

use gpui::{
    Context, Entity, FocusHandle, Render, Task, WeakEntity, WeakFocusHandle, Window, div,
    prelude::*,
};
use gpui_component::{
    Disableable as _, StyledExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    form::{field, v_form},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use ui::PanelDialog;

use crate::{
    defaults::account::has_permission,
    desktop::{
        api_session,
        contract::{ProvisionUserRequest, RoleResponse},
    },
};

use super::UsersPage;

pub(in crate::defaults::account::users) struct ProvisionUserDialog {
    page: WeakEntity<UsersPage>,
    identity_id: Entity<InputState>,
    display_name: Entity<InputState>,
    email: Entity<InputState>,
    avatar_url: Entity<InputState>,
    roles: Vec<RoleResponse>,
    selected_role_ids: BTreeSet<i64>,
    focus_handle: FocusHandle,
    previous_focus: Option<WeakFocusHandle>,
    open: bool,
    saving: bool,
    error: Option<String>,
    _task: Option<Task<()>>,
}

impl ProvisionUserDialog {
    pub(in crate::defaults::account::users) fn new(
        page: WeakEntity<UsersPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            page,
            identity_id: cx
                .new(|cx| InputState::new(window, cx).placeholder("OIDC subject / Identity ID")),
            display_name: cx.new(|cx| InputState::new(window, cx).placeholder("用户显示名称")),
            email: cx.new(|cx| InputState::new(window, cx).placeholder("可选邮箱")),
            avatar_url: cx.new(|cx| InputState::new(window, cx).placeholder("可选头像 URL")),
            roles: Vec::new(),
            selected_role_ids: BTreeSet::new(),
            focus_handle: cx.focus_handle(),
            previous_focus: None,
            open: false,
            saving: false,
            error: None,
            _task: None,
        }
    }

    pub(super) fn open(
        &mut self,
        roles: Vec<RoleResponse>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open {
            return;
        }
        self.previous_focus = window.focused(cx).map(|handle| handle.downgrade());
        self.roles = roles;
        self.selected_role_ids.clear();
        self.open = true;
        self.error = None;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn toggle_role(&mut self, role_id: i64, checked: bool, cx: &mut Context<Self>) {
        if !can_assign_initial_roles(cx) {
            return;
        }
        if checked {
            self.selected_role_ids.insert(role_id);
        } else {
            self.selected_role_ids.remove(&role_id);
        }
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.open = false;
        if let Some(handle) = self
            .previous_focus
            .take()
            .and_then(|handle| handle.upgrade())
        {
            handle.focus(window, cx);
        }
        cx.notify();
    }

    fn provision(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        if !has_permission(cx, "users:provision") {
            self.error = Some("需要 users:provision 权限才能开通用户".to_owned());
            cx.notify();
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let identity_id = self.identity_id.read(cx).value().trim().to_owned();
        let display_name = self.display_name.read(cx).value().trim().to_owned();
        if identity_id.is_empty() || display_name.is_empty() {
            self.error = Some("Identity ID 和显示名称不能为空".to_owned());
            cx.notify();
            return;
        }
        let request = ProvisionUserRequest {
            identity_id,
            email: optional_text(self.email.read(cx).value().as_ref()),
            display_name,
            avatar_url: optional_text(self.avatar_url.read(cx).value().as_ref()),
            role_ids: if can_assign_initial_roles(cx) {
                self.selected_role_ids.iter().copied().collect()
            } else {
                Vec::new()
            },
        };
        self.saving = true;
        self.error = None;
        let page = self.page.clone();
        let background = cx.background_spawn(async move { session.provision_user(&request) });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(user) => {
                        _ = page.update(cx, |page, cx| {
                            page.user_provisioned(user.display_name.clone(), cx);
                        });
                        this.reset_inputs(window, cx);
                        this.close(window, cx);
                    }
                    Err(error) => {
                        this.error = Some(error.user_message());
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    fn reset_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [
            &self.identity_id,
            &self.display_name,
            &self.email,
            &self.avatar_url,
        ] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.selected_role_ids.clear();
    }
}

impl Render for ProvisionUserDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }

        let dialog = cx.entity().downgrade();
        let can_close = !self.saving;
        let can_assign_roles = can_assign_initial_roles(cx);
        let role_options = self.roles.iter().map(|role| {
            let role_id = role.id;
            Checkbox::new(format!("default-provision-user-role-{role_id}"))
                .label(format!("{}（{}）", role.name, role.key))
                .checked(self.selected_role_ids.contains(&role_id))
                .disabled(self.saving || !can_assign_roles)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_role(role_id, *checked, cx);
                }))
        });
        PanelDialog::new(
            "default-account-provision-user-dialog",
            self.focus_handle.clone(),
        )
        .title("开通已确认的 OIDC 身份")
        .overlay_closable(can_close)
        .on_close(move |_, window, cx| {
            if can_close {
                _ = dialog.update(cx, |dialog, cx| dialog.close(window, cx));
            }
        })
        .when_some(self.error.clone(), |this, error| {
            this.child(Alert::error("default-provision-user-error", error).title("用户开通失败"))
        })
        .child(
            v_form()
                .columns(1)
                .child(
                    field()
                        .label("Identity ID")
                        .description(
                            "必须来自当前 OIDC Provider 中已存在且由管理员确认的稳定 subject；此操作不会创建 Provider 用户。",
                        )
                        .required(true)
                        .child(Input::new(&self.identity_id).disabled(self.saving)),
                )
                .child(
                    field()
                        .label("显示名称")
                        .required(true)
                        .child(Input::new(&self.display_name).disabled(self.saving)),
                )
                .child(
                    field()
                        .label("邮箱")
                        .child(Input::new(&self.email).disabled(self.saving)),
                )
                .child(
                    field()
                        .label("头像 URL")
                        .child(Input::new(&self.avatar_url).disabled(self.saving)),
                ),
        )
        .child(
            v_flex()
                .gap_2()
                .child(div().text_sm().font_semibold().child("初始角色"))
                .child(
                    div()
                        .text_xs()
                        .child("开通用户与角色关联会在同一请求中原子完成。"),
                )
                .when(!can_assign_roles, |this| {
                    this.child(Alert::info(
                        "default-provision-user-roles-forbidden",
                        "当前只能开通空角色用户；选择初始角色还需要 users:roles.write 与 roles:read 权限。",
                    ))
                })
                .when(can_assign_roles && self.roles.is_empty(), |this| {
                    this.child(Alert::info(
                        "default-provision-user-roles-empty",
                        "当前没有可分配角色。",
                    ))
                })
                .children(role_options),
        )
        .footer(
            h_flex()
                .gap_2()
                .child(
                    Button::new("cancel-default-provision-user")
                        .outline()
                        .label("取消")
                        .disabled(self.saving)
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                )
                .child(
                    Button::new("submit-default-provision-user")
                        .primary()
                        .label("确认开通本地用户")
                        .loading(self.saving)
                        .disabled(self.saving)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.provision(window, cx);
                        })),
                ),
        )
        .into_any_element()
    }
}

fn optional_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn can_assign_initial_roles(cx: &gpui::App) -> bool {
    has_permission(cx, "users:provision")
        && has_permission(cx, "users:roles.write")
        && has_permission(cx, "roles:read")
}
