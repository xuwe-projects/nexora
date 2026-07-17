//! 角色元数据、权限与删除操作组件。

use std::collections::BTreeSet;

use gpui::{Context, Entity, Render, Task, WeakEntity, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, StyledExt as _, WindowExt as _,
    alert::Alert,
    button::{Button, ButtonVariant, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    form::{field, v_form},
    h_flex,
    input::{Input, InputState},
    v_flex,
};

use contracts::patch::PatchField;

use crate::{
    defaults::account::has_permission,
    desktop::{
        AccountClientError, AccountSession, api_session,
        contract::{
            PermissionResponse, ReplaceRolePermissionsRequest, RoleResponse, UpdateRoleRequest,
        },
    },
};

use super::RolesPage;

pub(in crate::defaults::account::roles) struct RoleEditor {
    page: WeakEntity<RolesPage>,
    role: Option<RoleResponse>,
    permissions: Vec<PermissionResponse>,
    selected_permission_ids: BTreeSet<i64>,
    edit_name: Entity<InputState>,
    edit_description: Entity<InputState>,
    saving: bool,
    error: Option<String>,
    notice: Option<String>,
    _task: Option<Task<()>>,
}

impl RoleEditor {
    pub(super) const fn is_busy(&self) -> bool {
        self.saving
    }

    pub(in crate::defaults::account::roles) fn new(
        page: WeakEntity<RolesPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            page,
            role: None,
            permissions: Vec::new(),
            selected_permission_ids: BTreeSet::new(),
            edit_name: cx.new(|cx| InputState::new(window, cx).placeholder("角色名称")),
            edit_description: cx.new(|cx| InputState::new(window, cx).placeholder("可选角色说明")),
            saving: false,
            error: None,
            notice: None,
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
        if self.is_busy() {
            return;
        }
        self.selected_permission_ids = role
            .permissions
            .iter()
            .map(|permission| permission.id)
            .collect();
        self.edit_name.update(cx, |input, cx| {
            input.set_value(role.name.clone(), window, cx);
        });
        self.edit_description.update(cx, |input, cx| {
            input.set_value(role.description.clone().unwrap_or_default(), window, cx);
        });
        self.role = Some(role);
        self.permissions = permissions;
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.role = None;
        self.permissions.clear();
        self.selected_permission_ids.clear();
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    fn toggle_permission(&mut self, permission_id: i64, checked: bool, cx: &mut Context<Self>) {
        if checked {
            self.selected_permission_ids.insert(permission_id);
        } else {
            self.selected_permission_ids.remove(&permission_id);
        }
        cx.notify();
    }

    fn save_metadata(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let Some(role) = &self.role else {
            return;
        };
        let name = self.edit_name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.error = Some("角色名称不能为空".to_owned());
            cx.notify();
            return;
        }
        let role_id = role.id;
        let description = self.edit_description.read(cx).value();
        let request = UpdateRoleRequest {
            name: Some(name),
            description: optional_text(description.as_ref())
                .map(PatchField::Value)
                .unwrap_or(PatchField::Null),
        };
        self.start_update("角色基本信息已保存", cx, move |session| {
            session.update_role(role_id, &request)
        });
    }

    fn save_permissions(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let Some(role) = &self.role else {
            return;
        };
        let role_id = role.id;
        let request = ReplaceRolePermissionsRequest {
            permission_ids: self.selected_permission_ids.iter().copied().collect(),
        };
        self.start_update("角色权限已保存", cx, move |session| {
            session.replace_role_permissions(role_id, &request)
        });
    }

    fn start_update(
        &mut self,
        success_message: &'static str,
        cx: &mut Context<Self>,
        operation: impl FnOnce(&AccountSession) -> Result<RoleResponse, AccountClientError>
        + Send
        + 'static,
    ) {
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.saving = true;
        self.error = None;
        self.notice = None;
        let page = self.page.clone();
        let background = cx.background_spawn(async move { operation(&session) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(updated) => {
                        this.selected_permission_ids = updated
                            .permissions
                            .iter()
                            .map(|permission| permission.id)
                            .collect();
                        this.role = Some(updated.clone());
                        this.notice = Some(success_message.to_owned());
                        this.error = None;
                        _ = page.update(cx, |page, cx| page.role_updated(updated, cx));
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn delete_role(&mut self, role_id: i64, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.saving = true;
        self.error = None;
        self.notice = None;
        let page = self.page.clone();
        let background = cx.background_spawn(async move { session.delete_role(role_id) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => {
                        this.role = None;
                        this.permissions.clear();
                        this.selected_permission_ids.clear();
                        this.error = None;
                        _ = page.update(cx, |page, cx| page.role_deleted(role_id, cx));
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

        v_flex()
            .gap_4()
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
                            .child(
                                div()
                                    .text_lg()
                                    .font_semibold()
                                    .child(format!("管理 {}", role.name)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(role.key),
                            ),
                    )
                    .child(
                        Button::new("close-default-role-editor")
                            .ghost()
                            .label("关闭")
                            .disabled(self.saving)
                            .on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
                    ),
            )
            .when(immutable, |this| {
                this.child(Alert::info(
                    format!("default-system-role-immutable-{role_id}"),
                    "内置角色不能修改、删除或重新配置权限。",
                ))
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-role-editor-error", error))
            })
            .when_some(self.notice.clone(), |this, notice| {
                this.child(Alert::success("default-role-editor-notice", notice))
            })
            .child(
                v_form()
                    .columns(2)
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
                            "需要 permissions:read 权限才能查看或替换权限集合。",
                        ))
                    })
                    .when(
                        can_read_permissions && self.permissions.is_empty(),
                        |this| {
                            this.child(Alert::info(
                                "default-role-editor-permissions-empty",
                                "当前系统没有可分配权限。",
                            ))
                        },
                    )
                    .when(can_read_permissions, |this| {
                        this.children(permission_options)
                    }),
            )
            .when(!immutable, |this| {
                let editor: WeakEntity<Self> = cx.entity().downgrade();
                this.child(
                    h_flex()
                        .justify_between()
                        .child(
                            Button::new(format!("delete-default-role-{role_id}"))
                                .danger()
                                .outline()
                                .label("删除角色")
                                .disabled(self.saving || !can_write)
                                .on_click(move |_, window, cx| {
                                    let editor = editor.clone();
                                    window.open_alert_dialog(cx, move |dialog, _, _| {
                                        let editor = editor.clone();
                                        dialog
                                            .title("删除角色")
                                            .description(
                                                "若角色仍被用户引用，数据库将拒绝删除。请先解除所有用户—角色关联后重试。",
                                            )
                                            .button_props(
                                                DialogButtonProps::default()
                                                    .ok_text("确认删除")
                                                    .ok_variant(ButtonVariant::Danger)
                                                    .cancel_text("取消")
                                                    .show_cancel(true),
                                            )
                                            .on_ok(move |_, _, cx| {
                                                _ = editor.update(cx, |this, cx| {
                                                    this.delete_role(role_id, cx);
                                                });
                                                true
                                            })
                                    });
                                }),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new(format!("save-default-role-metadata-{role_id}"))
                                        .outline()
                                        .label("保存基本信息")
                                        .loading(self.saving)
                                        .disabled(self.saving || !can_write)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.save_metadata(cx);
                                        })),
                                )
                                .child(
                                    Button::new(format!("save-default-role-permissions-{role_id}"))
                                        .primary()
                                        .label("保存权限")
                                        .loading(self.saving)
                                        .disabled(
                                            self.saving || !can_write || !can_read_permissions,
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.save_permissions(cx);
                                        })),
                                ),
                        ),
                )
            })
            .into_any_element()
    }
}

fn optional_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
