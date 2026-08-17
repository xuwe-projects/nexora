//! 使用公共 FormDialog 创建或同步服务账号。

use std::collections::BTreeSet;

use gpui::{Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*};
use gpui_component::{
    Disableable as _, Sizable as _, StyledExt as _, WindowExt as _,
    alert::Alert,
    button::ButtonVariant,
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    form::field,
    h_flex,
    input::{Input, InputEvent, InputState},
    spinner::Spinner,
    v_flex,
};
use ui::{FormDialog, FormDialogState};

use crate::{
    defaults::account::has_permission,
    desktop::{
        AccountClientError, api_session,
        contract::{CreateServiceAccountRequest, RoleResponse},
    },
};

use super::UsersPage;

pub(in crate::defaults::account::users) struct CreateServiceAccountDialog {
    page: WeakEntity<UsersPage>,
    form: Entity<FormDialogState>,
    username: Entity<InputState>,
    display_name: Entity<InputState>,
    description: Entity<InputState>,
    roles: Vec<RoleResponse>,
    selected_role_ids: BTreeSet<i64>,
    saving: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
    _task: Option<Task<()>>,
}

impl CreateServiceAccountDialog {
    pub(in crate::defaults::account::users) fn new(
        page: WeakEntity<UsersPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(FormDialogState::new);
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("dispenser-line-a"));
        let display_name = cx.new(|cx| InputState::new(window, cx).placeholder("A 线点料机"));
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("可选用途说明"));
        let subscriptions = vec![
            track_input(cx, &form, &username, "username", "稳定 username"),
            track_input(cx, &form, &display_name, "display_name", "展示名称"),
            track_input(cx, &form, &description, "description", "说明"),
        ];
        Self {
            page,
            form,
            username,
            display_name,
            description,
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
        self.reset(window, cx);
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
        self.form.update(cx, |form, cx| {
            form.set_field_draft(
                "role_ids",
                "初始角色",
                "",
                self.selected_role_ids
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
                cx,
            );
        });
        cx.notify();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || !has_permission(cx, "service_accounts:provision") {
            return;
        }
        if text(&self.username, cx).is_empty() || text(&self.display_name, cx).is_empty() {
            self.error = Some("稳定 username 和展示名称不能为空".to_owned());
            cx.notify();
            return;
        }
        self.start_create(false, window, cx);
    }

    fn start_create(&mut self, use_existing: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let request = CreateServiceAccountRequest {
            username: text(&self.username, cx),
            display_name: text(&self.display_name, cx),
            description: optional_text(&self.description, cx),
            role_ids: if can_assign_initial_roles(cx) {
                self.selected_role_ids.iter().copied().collect()
            } else {
                Vec::new()
            },
            use_existing,
        };
        self.saving = true;
        self.error = None;
        self.form
            .update(cx, |form, cx| form.set_submitting(true, cx));
        let page = self.page.clone();
        let form = self.form.clone();
        let background =
            cx.background_spawn(async move { session.create_service_account(&request) });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                form.update(cx, |form, cx| form.set_submitting(false, cx));
                match result {
                    Ok(user) => {
                        _ = page.update(cx, |page, cx| {
                            page.service_account_created(user.display_name.clone(), cx);
                        });
                        form.update(cx, |form, cx| {
                            form.mark_saved(cx);
                            form.close(window, cx);
                        });
                        this.reset(window, cx);
                    }
                    Err(AccountClientError::Rejected { code, .. })
                        if code == "service_account_reuse_confirmation_required" =>
                    {
                        let dialog = cx.entity().downgrade();
                        window.open_alert_dialog(cx, move |alert, _, _| {
                            let dialog = dialog.clone();
                            alert
                                .title("服务账号已经存在")
                                .description(
                                    "检测到同名服务账号。是否直接同步并使用该账号？确认后会按当前选择替换它的角色。",
                                )
                                .button_props(
                                    DialogButtonProps::default()
                                        .ok_text("直接使用")
                                        .ok_variant(ButtonVariant::Primary)
                                        .cancel_text("取消")
                                        .show_cancel(true),
                                )
                                .on_ok(move |_, window, cx| {
                                    _ = dialog.update(cx, |dialog, cx| {
                                        dialog.start_create(true, window, cx);
                                    });
                                    true
                                })
                        });
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [&self.username, &self.display_name, &self.description] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.selected_role_ids.clear();
        self.form.update(cx, FormDialogState::reset_fields);
    }
}

impl Render for CreateServiceAccountDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let can_assign_roles = can_assign_initial_roles(cx);
        let roles = self.roles.iter().map(|role| {
            let role_id = role.id;
            Checkbox::new(format!("default-service-account-role-{role_id}"))
                .with_size(component_size)
                .label(format!("{}（{}）", role.name, role.key))
                .checked(self.selected_role_ids.contains(&role_id))
                .disabled(self.saving || !can_assign_roles)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_role(role_id, *checked, cx);
                }))
        });
        let status = v_flex()
            .gap_3()
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("create-service-account-error", error).title("创建失败"))
            })
            .when(self.saving, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child("正在创建或同步服务账号…"),
                )
            });
        let role_section = v_flex()
            .gap_2()
            .child(div().text_sm().font_semibold().child("初始角色"))
            .child(div().text_xs().child("可选；不会自动附加 member 角色。"))
            .when(!can_assign_roles, |this| {
                this.child(Alert::info(
                    "service-account-roles-forbidden",
                    "当前账号不能选择初始角色。",
                ))
            })
            .children(roles);
        let dialog = cx.entity().downgrade();
        FormDialog::new("create-service-account-dialog", self.form.clone())
            .title("创建服务账号")
            .description("创建新的 ZITADEL machine user；同名账号存在时可确认后直接同步使用。")
            .section(status)
            .child(
                field()
                    .label("稳定 username")
                    .description("创建后不可修改。")
                    .required(true)
                    .child(Input::new(&self.username).disabled(self.saving)),
            )
            .child(
                field()
                    .label("展示名称")
                    .required(true)
                    .child(Input::new(&self.display_name).disabled(self.saving)),
            )
            .child(
                field()
                    .label("说明")
                    .description("可选；最多 500 个字符。")
                    .child(Input::new(&self.description).disabled(self.saving)),
            )
            .section(role_section)
            .submit_label("创建服务账号")
            .with_size(component_size)
            .on_submit(move |_, window, cx| {
                _ = dialog.update(cx, |dialog, cx| dialog.create(window, cx));
            })
    }
}

fn track_input(
    cx: &mut Context<CreateServiceAccountDialog>,
    form: &Entity<FormDialogState>,
    input: &Entity<InputState>,
    key: &'static str,
    label: &'static str,
) -> Subscription {
    let form = form.clone();
    cx.subscribe(input, move |_, input, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            form.update(cx, |form, cx| {
                form.set_field_draft(key, label, "", input.read(cx).value().to_string(), cx);
            });
        }
    })
}

fn text(input: &Entity<InputState>, cx: &gpui::App) -> String {
    input.read(cx).value().trim().to_owned()
}

fn optional_text(input: &Entity<InputState>, cx: &gpui::App) -> Option<String> {
    let value = text(input, cx);
    (!value.is_empty()).then_some(value)
}

fn can_assign_initial_roles(cx: &gpui::App) -> bool {
    has_permission(cx, "service_accounts:provision")
        && has_permission(cx, "users:roles.write")
        && has_permission(cx, "roles:read")
}
