//! 创建用户对话框内容。

use std::collections::BTreeSet;

use gpui::{Context, Entity, Render, Task, WeakEntity, Window, div, prelude::*, px};
use gpui_component::{
    Disableable as _, Sizable as _, StyledExt as _, WindowExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogFooter,
    form::{field, v_form},
    h_flex,
    input::{Input, InputState},
    spinner::Spinner,
    v_flex,
};

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
    username: Entity<InputState>,
    display_name: Entity<InputState>,
    email: Entity<InputState>,
    avatar_url: Entity<InputState>,
    roles: Vec<RoleResponse>,
    selected_role_ids: BTreeSet<i64>,
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
            username: cx.new(|cx| InputState::new(window, cx).placeholder("登录用户名")),
            display_name: cx.new(|cx| InputState::new(window, cx).placeholder("用户显示名称")),
            email: cx.new(|cx| InputState::new(window, cx).placeholder("可选邮箱")),
            avatar_url: cx.new(|cx| InputState::new(window, cx).placeholder("可选头像 URL")),
            roles: Vec::new(),
            selected_role_ids: BTreeSet::new(),
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
        if self.saving || window.has_active_dialog(cx) {
            return;
        }
        self.reset_inputs(window, cx);
        self.roles = roles;
        self.error = None;
        let content = cx.entity();
        let cancel_dialog = content.downgrade();
        let submit_dialog = content.downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let cancel_dialog = cancel_dialog.clone();
            let submit_dialog = submit_dialog.clone();
            dialog
                .title("创建用户")
                .width(px(520.))
                .max_h(px(640.))
                .close_button(false)
                .keyboard(false)
                .overlay_closable(false)
                .child(content.clone())
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("cancel-default-provision-user")
                                .outline()
                                .label("取消")
                                .on_click(move |_, window, cx| {
                                    _ = cancel_dialog.update(cx, |dialog, cx| {
                                        dialog.cancel(window, cx);
                                    });
                                }),
                        )
                        .child(
                            Button::new("submit-default-provision-user")
                                .primary()
                                .label("创建用户")
                                .on_click(move |_, window, cx| {
                                    _ = submit_dialog.update(cx, |dialog, cx| {
                                        dialog.provision(window, cx);
                                    });
                                }),
                        ),
                )
        });
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

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.reset_inputs(window, cx);
        self.error = None;
        window.close_dialog(cx);
        cx.notify();
    }

    fn provision(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        if !has_permission(cx, "users:provision") {
            self.error = Some("需要 users:provision 权限才能创建用户".to_owned());
            cx.notify();
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let identity_id = self.identity_id.read(cx).value().trim().to_owned();
        let username = self.username.read(cx).value().trim().to_owned();
        let display_name = self.display_name.read(cx).value().trim().to_owned();
        if identity_id.is_empty() || username.is_empty() || display_name.is_empty() {
            self.error = Some("登录用户名、Identity ID 和显示名称不能为空".to_owned());
            cx.notify();
            return;
        }
        let request = ProvisionUserRequest {
            identity_id,
            username: Some(username),
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
                        window.close_dialog(cx);
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn reset_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [
            &self.identity_id,
            &self.username,
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

        v_flex()
            .w_full()
            .gap_4()
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-provision-user-error", error).title("创建失败"))
            })
            .when(self.saving, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child("正在创建用户…"),
                )
            })
            .child(
                v_form()
                    .columns(1)
                    .child(
                        field()
                            .label("登录用户名")
                            .description("绑定身份提供方中的登录用户名，用于管理界面识别用户。")
                            .required(true)
                            .child(Input::new(&self.username).disabled(self.saving)),
                    )
                    .child(
                        field()
                            .label("Identity ID")
                            .description(
                                "填写当前 OIDC Provider 中稳定且唯一的 subject；实际登录绑定仍以此字段为准。",
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
                            .child("创建用户与角色关联会在同一请求中原子完成。"),
                    )
                    .when(!can_assign_roles, |this| {
                        this.child(Alert::info(
                            "default-provision-user-roles-forbidden",
                            "当前只能创建空角色用户；选择初始角色还需要 users:roles.write 与 roles:read 权限。",
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
