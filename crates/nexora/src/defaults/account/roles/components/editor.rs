//! 使用公共 FormDialog 编辑角色元数据与权限。

use std::collections::BTreeSet;

use gpui::{Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, StyledExt as _, WindowExt as _,
    alert::Alert,
    button::{Button, ButtonVariant, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    form::{field, v_form},
    input::{Input, InputEvent, InputState},
    v_flex,
};
use ui::{FormDialog, FormDialogState};

use contracts::patch::PatchField;

use crate::{
    defaults::account::has_permission,
    desktop::{
        api_session,
        contract::{
            PermissionResponse, ReplaceRolePermissionsRequest, RoleResponse, UpdateRoleRequest,
        },
    },
};

use super::RolesPage;

pub(in crate::defaults::account::roles) struct RoleEditor {
    page: WeakEntity<RolesPage>,
    form: Entity<FormDialogState>,
    role: Option<RoleResponse>,
    permissions: Vec<PermissionResponse>,
    selected_permission_ids: BTreeSet<i64>,
    original_permission_ids: String,
    edit_name: Entity<InputState>,
    edit_description: Entity<InputState>,
    saving: bool,
    error: Option<String>,
    _form_subscription: Subscription,
    _subscriptions: Vec<Subscription>,
    _task: Option<Task<()>>,
}

impl RoleEditor {
    pub(super) fn is_open(&self, cx: &gpui::App) -> bool {
        self.form.read(cx).is_open()
    }

    pub(super) fn is_busy(&self, cx: &gpui::App) -> bool {
        self.saving || self.is_open(cx)
    }

    pub(in crate::defaults::account::roles) fn new(
        page: WeakEntity<RolesPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(FormDialogState::new);
        let edit_name = cx.new(|cx| InputState::new(window, cx).placeholder("角色名称"));
        let edit_description = cx.new(|cx| InputState::new(window, cx).placeholder("可选角色说明"));
        let subscriptions = vec![
            track_input(cx, &form, &edit_name, "name", "角色名称"),
            track_input(cx, &form, &edit_description, "description", "说明"),
        ];
        let form_subscription = cx.observe(&form, |_, _, cx| cx.notify());
        Self {
            page,
            form,
            role: None,
            permissions: Vec::new(),
            selected_permission_ids: BTreeSet::new(),
            original_permission_ids: String::new(),
            edit_name,
            edit_description,
            saving: false,
            error: None,
            _form_subscription: form_subscription,
            _subscriptions: subscriptions,
            _task: None,
        }
    }

    pub(super) fn open(
        &mut self,
        role: RoleResponse,
        permissions: Vec<PermissionResponse>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy(cx) {
            return;
        }
        self.selected_permission_ids = role
            .permissions
            .iter()
            .map(|permission| permission.id)
            .collect();
        self.original_permission_ids = permission_draft(&self.selected_permission_ids);
        self.edit_name.update(cx, |input, cx| {
            input.set_value(role.name.clone(), window, cx);
        });
        self.edit_description.update(cx, |input, cx| {
            input.set_value(role.description.clone().unwrap_or_default(), window, cx);
        });
        self.permissions = permissions;
        self.error = None;
        self.form.update(cx, |form, cx| {
            form.reset_fields(cx);
            form.set_field_draft("name", "角色名称", role.name.clone(), role.name.clone(), cx);
            let description = role.description.clone().unwrap_or_default();
            form.set_field_draft("description", "说明", description.clone(), description, cx);
            form.set_field_draft(
                "permission_ids",
                "绑定权限",
                self.original_permission_ids.clone(),
                self.original_permission_ids.clone(),
                cx,
            );
            form.open(window, cx);
        });
        self.role = Some(role);
        cx.notify();
    }

    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.form.read(cx).is_open() {
            return;
        }
        self.role = None;
        self.permissions.clear();
        self.selected_permission_ids.clear();
        self.original_permission_ids.clear();
        self.error = None;
        self.form.update(cx, FormDialogState::reset_fields);
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
            form.set_field_draft(
                "permission_ids",
                "绑定权限",
                self.original_permission_ids.clone(),
                draft,
                cx,
            );
        });
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(role) = &self.role else {
            return;
        };
        if role.is_system {
            self.error = Some("内置系统角色不可修改".to_owned());
            cx.notify();
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let name = input_text(&self.edit_name, cx);
        if name.is_empty() {
            self.error = Some("角色名称不能为空".to_owned());
            cx.notify();
            return;
        }
        let role_id = role.id;
        let metadata = UpdateRoleRequest {
            name: Some(name),
            description: optional_text(self.edit_description.read(cx).value().as_ref())
                .map(PatchField::Value)
                .unwrap_or(PatchField::Null),
        };
        let permissions = ReplaceRolePermissionsRequest {
            permission_ids: self.selected_permission_ids.iter().copied().collect(),
        };
        self.saving = true;
        self.error = None;
        self.form
            .update(cx, |form, cx| form.set_submitting(true, cx));
        let page = self.page.clone();
        let form = self.form.clone();
        let background = cx.background_spawn(async move {
            session.update_role(role_id, &metadata)?;
            session.replace_role_permissions(role_id, &permissions)
        });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                form.update(cx, |form, cx| form.set_submitting(false, cx));
                match result {
                    Ok(updated) => {
                        this.selected_permission_ids = updated
                            .permissions
                            .iter()
                            .map(|permission| permission.id)
                            .collect();
                        this.original_permission_ids =
                            permission_draft(&this.selected_permission_ids);
                        this.role = Some(updated.clone());
                        this.error = None;
                        _ = page.update(cx, |page, cx| page.role_updated(updated, cx));
                        form.update(cx, |form, cx| {
                            form.mark_saved(cx);
                            form.close(window, cx);
                        });
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn delete_role(&mut self, role_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.saving = true;
        self.error = None;
        self.form
            .update(cx, |form, cx| form.set_submitting(true, cx));
        let page = self.page.clone();
        let form = self.form.clone();
        let background = cx.background_spawn(async move { session.delete_role(role_id) });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                form.update(cx, |form, cx| form.set_submitting(false, cx));
                match result {
                    Ok(()) => {
                        this.role = None;
                        this.permissions.clear();
                        this.selected_permission_ids.clear();
                        this.error = None;
                        _ = page.update(cx, |page, cx| page.role_deleted(role_id, cx));
                        form.update(cx, |form, cx| {
                            form.reset_fields(cx);
                            form.close(window, cx);
                        });
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for RoleEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(role) = self.role.clone() else {
            return div().into_any_element();
        };
        let role_id = role.id;
        let immutable = role.is_system;
        let can_write = has_permission(cx, "roles:write");
        let can_read_permissions = has_permission(cx, "permissions:read");
        let permission_options = self.permissions.iter().map(|permission| {
            let permission_id = permission.id;
            Checkbox::new(format!("default-role-permission-{role_id}-{permission_id}"))
                .label(format!("{}（{}）", permission.name, permission.key))
                .checked(self.selected_permission_ids.contains(&permission_id))
                .disabled(immutable || self.saving || !can_write || !can_read_permissions)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_permission(permission_id, *checked, cx);
                }))
        });
        let editor: WeakEntity<Self> = cx.entity().downgrade();
        let delete_button = editor.clone();
        let content = v_flex()
            .w_full()
            .gap_4()
            .when(immutable, |this| {
                this.child(Alert::info(
                    format!("default-system-role-immutable-{role_id}"),
                    if role.key == "admin" {
                        "系统管理员角色由框架维护：新注册权限会自动加入，不能手动修改或删除。"
                    } else {
                        "内置角色不能修改、删除或重新配置权限。"
                    },
                ))
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-role-editor-error", error))
            })
            .child(
                v_form()
                    .columns(1)
                    .child(
                        field().label("角色键").child(
                            div()
                                .p_2()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().tokens.group_box)
                                .child(role.key.clone()),
                        ),
                    )
                    .child(
                        field().label("角色名称").child(
                            Input::new(&self.edit_name)
                                .disabled(immutable || self.saving || !can_write),
                        ),
                    )
                    .child(
                        field().label("说明").child(
                            Input::new(&self.edit_description)
                                .disabled(immutable || self.saving || !can_write),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().font_semibold().child("绑定权限"))
                    .when(!can_read_permissions, |this| {
                        this.child(Alert::info(
                            "default-role-editor-permissions-unavailable",
                            "当前账号不能查看或替换权限集合。",
                        ))
                    })
                    .when(can_read_permissions && self.permissions.is_empty(), |this| {
                        this.child(Alert::info(
                            "default-role-editor-permissions-empty",
                            "当前系统没有可分配权限。",
                        ))
                    })
                    .when(can_read_permissions, |this| this.children(permission_options)),
            )
            .when(!immutable, |this| {
                this.child(
                    Button::new(format!("delete-default-role-{role_id}"))
                        .danger()
                        .outline()
                        .label("删除角色")
                        .disabled(self.saving || !can_write)
                        .on_click(move |_, window, cx| {
                            let editor = delete_button.clone();
                            window.open_alert_dialog(cx, move |dialog, _, _| {
                                let editor = editor.clone();
                                dialog
                                    .title("删除角色")
                                    .description(
                                        "若角色仍被用户引用，数据库将拒绝删除。请先解除用户—角色关联后重试。",
                                    )
                                    .button_props(
                                        DialogButtonProps::default()
                                            .ok_text("确认删除")
                                            .ok_variant(ButtonVariant::Danger)
                                            .cancel_text("取消")
                                            .show_cancel(true),
                                    )
                                    .on_ok(move |_, window, cx| {
                                        _ = editor.update(cx, |this, cx| {
                                            this.delete_role(role_id, window, cx);
                                        });
                                        true
                                    })
                            });
                        }),
                )
            });
        FormDialog::new(
            "default-role-editor-form-dialog",
            self.form.clone(),
            format!("管理 {}", role.name),
            content,
            move |_, window, cx| {
                _ = editor.update(cx, |editor, cx| editor.submit(window, cx));
            },
        )
        .description("保存角色信息与权限设置。")
        .submit_label("保存角色")
        .submit_disabled(immutable || !can_write || !can_read_permissions)
        .into_any_element()
    }
}

fn track_input(
    cx: &mut Context<RoleEditor>,
    form: &Entity<FormDialogState>,
    input: &Entity<InputState>,
    key: &'static str,
    label: &'static str,
) -> Subscription {
    let form = form.clone();
    cx.subscribe(input, move |this, input, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            let draft = input.read(cx).value().to_string();
            let original = this
                .role
                .as_ref()
                .map_or_else(String::new, |role| match key {
                    "name" => role.name.clone(),
                    "description" => role.description.clone().unwrap_or_default(),
                    _ => String::new(),
                });
            form.update(cx, |form, cx| {
                form.set_field_draft(key, label, original, draft, cx);
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
