//! 使用公共 FormDialog 创建角色。

use std::collections::BTreeSet;

use gpui::{Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*};
use gpui_component::{
    Disableable as _, StyledExt as _,
    alert::Alert,
    checkbox::Checkbox,
    form::{field, v_form},
    input::{Input, InputEvent, InputState},
    v_flex,
};
use ui::{FormDialog, FormDialogState};

use crate::{
    defaults::account::has_permission,
    desktop::{
        api_session,
        contract::{CreateRoleRequest, PermissionResponse},
    },
};

use super::RolesPage;

pub(in crate::defaults::account::roles) struct RoleCreateDialog {
    page: WeakEntity<RolesPage>,
    form: Entity<FormDialogState>,
    role_key: Entity<InputState>,
    role_name: Entity<InputState>,
    description: Entity<InputState>,
    permissions: Vec<PermissionResponse>,
    selected_permission_ids: BTreeSet<i64>,
    saving: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
    _task: Option<Task<()>>,
}

impl RoleCreateDialog {
    pub(in crate::defaults::account::roles) fn new(
        page: WeakEntity<RolesPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(FormDialogState::new);
        let role_key =
            cx.new(|cx| InputState::new(window, cx).placeholder("例如：quality_manager"));
        let role_name = cx.new(|cx| InputState::new(window, cx).placeholder("角色名称"));
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("可选角色说明"));
        let subscriptions = vec![
            track_input(cx, &form, &role_key, "key", "角色键"),
            track_input(cx, &form, &role_name, "name", "角色名称"),
            track_input(cx, &form, &description, "description", "说明"),
        ];
        Self {
            page,
            form,
            role_key,
            role_name,
            description,
            permissions: Vec::new(),
            selected_permission_ids: BTreeSet::new(),
            saving: false,
            error: None,
            _subscriptions: subscriptions,
            _task: None,
        }
    }

    pub(super) fn open(
        &mut self,
        permissions: Vec<PermissionResponse>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.form.read(cx).is_open() {
            return;
        }
        self.reset(window, cx);
        self.permissions = permissions;
        self.error = None;
        self.form.update(cx, |form, cx| form.open(window, cx));
        cx.notify();
    }

    fn toggle_permission(&mut self, permission_id: i64, checked: bool, cx: &mut Context<Self>) {
        if checked {
            self.selected_permission_ids.insert(permission_id);
        } else {
            self.selected_permission_ids.remove(&permission_id);
        }
        let draft = permission_draft(&self.selected_permission_ids);
        self.form.update(cx, |form, cx| {
            form.set_field_draft("permission_ids", "初始权限", "", draft, cx);
        });
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let key = input_text(&self.role_key, cx);
        let name = input_text(&self.role_name, cx);
        if key.is_empty() || name.is_empty() {
            self.error = Some("角色键和角色名称不能为空".to_owned());
            cx.notify();
            return;
        }
        let request = CreateRoleRequest {
            key,
            name,
            description: optional_text(self.description.read(cx).value().as_ref()),
            permission_ids: self.selected_permission_ids.iter().copied().collect(),
        };
        self.saving = true;
        self.error = None;
        self.form
            .update(cx, |form, cx| form.set_submitting(true, cx));
        let page = self.page.clone();
        let form = self.form.clone();
        let background = cx.background_spawn(async move { session.create_role(&request) });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                form.update(cx, |form, cx| form.set_submitting(false, cx));
                match result {
                    Ok(role) => {
                        _ = page.update(cx, |page, cx| page.role_created(role, cx));
                        form.update(cx, |form, cx| {
                            form.mark_saved(cx);
                            form.close(window, cx);
                        });
                        this.reset(window, cx);
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [&self.role_key, &self.role_name, &self.description] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.selected_permission_ids.clear();
        self.form.update(cx, FormDialogState::reset_fields);
    }
}

impl Render for RoleCreateDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_read_permissions = has_permission(cx, "permissions:read");
        let permission_options = self.permissions.iter().map(|permission| {
            let permission_id = permission.id;
            Checkbox::new(format!("default-create-role-permission-{permission_id}"))
                .label(format!("{}（{}）", permission.name, permission.key))
                .checked(self.selected_permission_ids.contains(&permission_id))
                .disabled(self.saving || !can_read_permissions)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_permission(permission_id, *checked, cx);
                }))
        });
        let content = v_flex()
            .w_full()
            .gap_4()
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-create-role-error", error).title("角色创建失败"))
            })
            .child(
                v_form()
                    .columns(1)
                    .child(
                        field()
                            .label("角色键")
                            .description(
                                "使用 2 至 64 位小写字母、数字、点、下划线或连字符，并以字母开头。",
                            )
                            .required(true)
                            .child(Input::new(&self.role_key).disabled(self.saving)),
                    )
                    .child(
                        field()
                            .label("角色名称")
                            .required(true)
                            .child(Input::new(&self.role_name).disabled(self.saving)),
                    )
                    .child(
                        field()
                            .label("说明")
                            .child(Input::new(&self.description).disabled(self.saving)),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().font_semibold().child("初始权限"))
                    .when(!can_read_permissions, |this| {
                        this.child(Alert::info(
                            "default-create-role-permissions-unavailable",
                            "当前账号没有 permissions:read 权限，角色将以空权限集合创建。",
                        ))
                    })
                    .when(
                        can_read_permissions && self.permissions.is_empty(),
                        |this| {
                            this.child(Alert::info(
                                "default-create-role-permissions-empty",
                                "当前系统没有可分配权限。",
                            ))
                        },
                    )
                    .when(can_read_permissions, |this| {
                        this.children(permission_options)
                    }),
            );
        let dialog = cx.entity().downgrade();
        FormDialog::new(
            "default-create-role-form-dialog",
            self.form.clone(),
            "创建角色",
            content,
            move |_, window, cx| {
                _ = dialog.update(cx, |dialog, cx| dialog.submit(window, cx));
            },
        )
        .description("创建自定义角色并设置初始权限；后续可在角色管理中调整。")
        .submit_label("创建角色")
    }
}

fn track_input(
    cx: &mut Context<RoleCreateDialog>,
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

fn permission_draft(permission_ids: &BTreeSet<i64>) -> String {
    permission_ids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
