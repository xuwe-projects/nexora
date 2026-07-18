//! 使用公共 FormDialog 创建 ZITADEL 用户并绑定本地账号。

use std::collections::BTreeSet;

use gpui::{Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*};
use gpui_component::{
    Disableable as _, Sizable as _, StyledExt as _,
    alert::Alert,
    checkbox::Checkbox,
    form::{field, v_form},
    h_flex,
    input::{Input, InputEvent, InputState},
    spinner::Spinner,
    v_flex,
};
use ui::{FormDialog, FormDialogState};

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
    form: Entity<FormDialogState>,
    username: Entity<InputState>,
    given_name: Entity<InputState>,
    family_name: Entity<InputState>,
    display_name: Entity<InputState>,
    email: Entity<InputState>,
    roles: Vec<RoleResponse>,
    selected_role_ids: BTreeSet<i64>,
    saving: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
    _task: Option<Task<()>>,
}

impl ProvisionUserDialog {
    pub(in crate::defaults::account::users) fn new(
        page: WeakEntity<UsersPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(FormDialogState::new);
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("登录用户名"));
        let given_name = cx.new(|cx| InputState::new(window, cx).placeholder("名字"));
        let family_name = cx.new(|cx| InputState::new(window, cx).placeholder("姓氏"));
        let display_name = cx.new(|cx| InputState::new(window, cx).placeholder("可选展示名称"));
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("user@example.com"));
        let subscriptions = vec![
            track_input(cx, &form, &username, "username", "登录用户名"),
            track_input(cx, &form, &given_name, "given_name", "名字"),
            track_input(cx, &form, &family_name, "family_name", "姓氏"),
            track_input(cx, &form, &display_name, "display_name", "展示名称"),
            track_input(cx, &form, &email, "email", "邮箱"),
        ];
        Self {
            page,
            form,
            username,
            given_name,
            family_name,
            display_name,
            email,
            roles: Vec::new(),
            selected_role_ids: BTreeSet::new(),
            saving: false,
            error: None,
            _subscriptions: subscriptions,
            _task: None,
        }
    }

    pub(super) fn open(
        &mut self,
        roles: Vec<RoleResponse>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving {
            return;
        }
        self.reset_inputs(window, cx);
        self.roles = roles;
        self.error = None;
        self.form.update(cx, |form, cx| form.open(window, cx));
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
        let draft = role_draft(&self.selected_role_ids);
        self.form.update(cx, |form, cx| {
            form.set_field_draft("role_ids", "初始角色", "", draft, cx);
        });
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
        let username = input_text(&self.username, cx);
        let given_name = input_text(&self.given_name, cx);
        let family_name = input_text(&self.family_name, cx);
        let email = input_text(&self.email, cx);
        if username.is_empty()
            || given_name.is_empty()
            || family_name.is_empty()
            || email.is_empty()
        {
            self.error = Some("登录用户名、名字、姓氏和邮箱不能为空".to_owned());
            cx.notify();
            return;
        }
        let request = ProvisionUserRequest {
            username,
            given_name,
            family_name,
            email,
            display_name: optional_text(self.display_name.read(cx).value().as_ref()),
            role_ids: if can_assign_initial_roles(cx) {
                self.selected_role_ids.iter().copied().collect()
            } else {
                Vec::new()
            },
        };
        self.saving = true;
        self.error = None;
        self.form
            .update(cx, |form, cx| form.set_submitting(true, cx));
        let page = self.page.clone();
        let form = self.form.clone();
        let background = cx.background_spawn(async move { session.provision_user(&request) });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                form.update(cx, |form, cx| form.set_submitting(false, cx));
                match result {
                    Ok(user) => {
                        _ = page.update(cx, |page, cx| {
                            page.user_provisioned(user.display_name.clone(), cx);
                        });
                        form.update(cx, |form, cx| {
                            form.mark_saved(cx);
                            form.close(window, cx);
                        });
                        this.reset_inputs(window, cx);
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
            &self.username,
            &self.given_name,
            &self.family_name,
            &self.display_name,
            &self.email,
        ] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.selected_role_ids.clear();
        self.form.update(cx, FormDialogState::reset_fields);
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

        let content = v_flex()
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
                        .child("正在 ZITADEL 创建并绑定用户…"),
                )
            })
            .child(
                v_form()
                    .columns(2)
                    .child(
                        field()
                            .label("登录用户名")
                            .description("在 ZITADEL Organization 中唯一，并可用于登录。")
                            .required(true)
                            .child(Input::new(&self.username).disabled(self.saving)),
                    )
                    .child(
                        field()
                            .label("邮箱")
                            .description("创建后由 ZITADEL 发送默认验证邮件。")
                            .required(true)
                            .child(Input::new(&self.email).disabled(self.saving)),
                    )
                    .child(
                        field()
                            .label("名字")
                            .required(true)
                            .child(Input::new(&self.given_name).disabled(self.saving)),
                    )
                    .child(
                        field()
                            .label("姓氏")
                            .required(true)
                            .child(Input::new(&self.family_name).disabled(self.saving)),
                    )
                    .child(
                        field()
                            .label("展示名称")
                            .description("可选；省略时使用名字与姓氏。")
                            .child(Input::new(&self.display_name).disabled(self.saving)),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().font_semibold().child("初始角色"))
                    .child(
                        div()
                            .text_xs()
                            .child("ZITADEL 创建成功后，本地用户与角色会在同一事务中写入。"),
                    )
                    .when(!can_assign_roles, |this| {
                        this.child(Alert::info(
                            "default-provision-user-roles-forbidden",
                            "选择初始角色还需要 users:roles.write 与 roles:read 权限。",
                        ))
                    })
                    .when(can_assign_roles && self.roles.is_empty(), |this| {
                        this.child(Alert::info(
                            "default-provision-user-roles-empty",
                            "当前没有可分配角色。",
                        ))
                    })
                    .children(role_options),
            );
        let dialog = cx.entity().downgrade();
        FormDialog::new(
            "default-provision-user-form-dialog",
            self.form.clone(),
            "创建用户",
            content,
            move |_, window, cx| {
                _ = dialog.update(cx, |dialog, cx| dialog.provision(window, cx));
            },
        )
        .description("在 ZITADEL 创建人类用户，并自动关联到 Nexora Account。")
        .submit_label("创建用户")
    }
}

fn track_input(
    cx: &mut Context<ProvisionUserDialog>,
    form: &Entity<FormDialogState>,
    input: &Entity<InputState>,
    key: &'static str,
    label: &'static str,
) -> Subscription {
    let form = form.clone();
    cx.subscribe(input, move |_, input, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            let draft = input.read(cx).value().to_string();
            form.update(cx, |form, cx| {
                form.set_field_draft(key, label, "", draft, cx);
            });
        }
    })
}

fn input_text(input: &Entity<InputState>, cx: &gpui::App) -> String {
    input.read(cx).value().trim().to_owned()
}

fn optional_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn role_draft(role_ids: &BTreeSet<i64>) -> String {
    role_ids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn can_assign_initial_roles(cx: &gpui::App) -> bool {
    has_permission(cx, "users:provision")
        && has_permission(cx, "users:roles.write")
        && has_permission(cx, "roles:read")
}
