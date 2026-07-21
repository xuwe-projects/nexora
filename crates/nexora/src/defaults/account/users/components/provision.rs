//! 使用公共 FormDialog 创建用户。

use std::collections::BTreeSet;

use gpui::{Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*};
use gpui_component::{
    Disableable as _, Sizable as _, StyledExt as _,
    alert::Alert,
    checkbox::Checkbox,
    h_flex,
    input::{InputEvent, InputState},
    spinner::Spinner,
    v_flex,
};
use ui::{FormDialog, FormDialogState, FormItem};

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
    avatar_url: Entity<InputState>,
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
        let avatar_url = cx.new(|cx| {
            InputState::new(window, cx).placeholder("https://cdn.example.com/avatar.png")
        });
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("user@example.com"));
        let subscriptions = vec![
            track_input(cx, &form, &username, "username", "登录用户名"),
            track_input(cx, &form, &given_name, "given_name", "名字"),
            track_input(cx, &form, &family_name, "family_name", "姓氏"),
            track_input(cx, &form, &display_name, "display_name", "展示名称"),
            track_input(cx, &form, &avatar_url, "avatar_url", "头像 URL"),
            track_input(cx, &form, &email, "email", "邮箱"),
        ];
        Self {
            page,
            form,
            username,
            given_name,
            family_name,
            display_name,
            avatar_url,
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
            self.error = Some("当前账号不能创建用户".to_owned());
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
        let initial_password = format!("nexora{}.", username);
        let request = ProvisionUserRequest {
            username,
            given_name,
            family_name,
            email,
            display_name: optional_text(self.display_name.read(cx).value().as_ref()),
            avatar_url: optional_text(self.avatar_url.read(cx).value().as_ref()),
            initial_password,
            require_password_change: true,
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
            &self.avatar_url,
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
        let component_size = theme::component_size(cx);
        let can_assign_roles = can_assign_initial_roles(cx);
        let role_options = self.roles.iter().map(|role| {
            let role_id = role.id;
            Checkbox::new(format!("default-provision-user-role-{role_id}"))
                .with_size(component_size)
                .label(format!("{}（{}）", role.name, role.key))
                .checked(self.selected_role_ids.contains(&role_id))
                .disabled(self.saving || !can_assign_roles)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_role(role_id, *checked, cx);
                }))
        });

        let status_section = v_flex()
            .w_full()
            .gap_3()
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
            });
        let roles_section = v_flex()
            .gap_2()
            .child(div().text_sm().font_semibold().child("初始角色"))
            .child(div().text_xs().child("可选；创建后也可以继续调整。"))
            .when(!can_assign_roles, |this| {
                this.child(Alert::info(
                    "default-provision-user-roles-forbidden",
                    "当前账号不能选择初始角色。",
                ))
            })
            .when(can_assign_roles && self.roles.is_empty(), |this| {
                this.child(Alert::info(
                    "default-provision-user-roles-empty",
                    "当前没有可分配角色。",
                ))
            })
            .children(role_options);
        let dialog = cx.entity().downgrade();
        FormDialog::new("default-provision-user-form-dialog", self.form.clone())
            .title("创建用户")
            .description("填写信息后创建用户。")
            .columns(2)
            .section(status_section)
            .child(
                FormItem::new("登录用户名")
                    .description("用于登录系统的唯一用户名。")
                    .required()
                    .input(&self.username)
                    .disabled(self.saving),
            )
            .child(
                FormItem::new("邮箱")
                    .description("用于接收账号相关通知。")
                    .required()
                    .input(&self.email)
                    .disabled(self.saving),
            )
            .child(
                FormItem::new("名字")
                    .required()
                    .input(&self.given_name)
                    .disabled(self.saving),
            )
            .child(
                FormItem::new("姓氏")
                    .required()
                    .input(&self.family_name)
                    .disabled(self.saving),
            )
            .child(
                FormItem::new("展示名称")
                    .description("可选；省略时使用名字与姓氏。")
                    .input(&self.display_name)
                    .disabled(self.saving),
            )
            .child(
                FormItem::new("头像 URL")
                    .description("可选；创建后会同步到身份目录")
                    .input(&self.avatar_url)
                    .disabled(self.saving),
            )
            .section(roles_section)
            .submit_label("创建用户")
            .with_size(component_size)
            .on_submit(move |_, window, cx| {
                _ = dialog.update(cx, |dialog, cx| dialog.provision(window, cx));
            })
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
