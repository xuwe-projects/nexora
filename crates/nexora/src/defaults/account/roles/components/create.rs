//! 角色创建对话框。

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
        contract::{CreateRoleRequest, PermissionResponse},
    },
};

use super::RolesPage;

pub(in crate::defaults::account::roles) struct RoleCreateDialog {
    page: WeakEntity<RolesPage>,
    role_key: Entity<InputState>,
    role_name: Entity<InputState>,
    description: Entity<InputState>,
    permissions: Vec<PermissionResponse>,
    selected_permission_ids: BTreeSet<i64>,
    focus_handle: FocusHandle,
    previous_focus: Option<WeakFocusHandle>,
    open: bool,
    saving: bool,
    error: Option<String>,
    _task: Option<Task<()>>,
}

impl RoleCreateDialog {
    pub(in crate::defaults::account::roles) fn new(
        page: WeakEntity<RolesPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            page,
            role_key: cx.new(|cx| InputState::new(window, cx).placeholder("例如：quality_manager")),
            role_name: cx.new(|cx| InputState::new(window, cx).placeholder("角色名称")),
            description: cx.new(|cx| InputState::new(window, cx).placeholder("可选角色说明")),
            permissions: Vec::new(),
            selected_permission_ids: BTreeSet::new(),
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
        permissions: Vec<PermissionResponse>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open {
            return;
        }
        self.previous_focus = window.focused(cx).map(|handle| handle.downgrade());
        self.permissions = permissions;
        self.selected_permission_ids.clear();
        self.open = true;
        self.error = None;
        self.focus_handle.focus(window, cx);
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

    fn toggle_permission(&mut self, permission_id: i64, checked: bool, cx: &mut Context<Self>) {
        if checked {
            self.selected_permission_ids.insert(permission_id);
        } else {
            self.selected_permission_ids.remove(&permission_id);
        }
        cx.notify();
    }

    fn create_role(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let key = self.role_key.read(cx).value().trim().to_owned();
        let name = self.role_name.read(cx).value().trim().to_owned();
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
        let page = self.page.clone();
        let background = cx.background_spawn(async move { session.create_role(&request) });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(role) => {
                        _ = page.update(cx, |page, cx| page.role_created(role, cx));
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
        for input in [&self.role_key, &self.role_name, &self.description] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.selected_permission_ids.clear();
    }
}

impl Render for RoleCreateDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }

        let can_read_permissions = has_permission(cx, "permissions:read");
        let permission_options = self.permissions.iter().map(|permission| {
            let permission_id = permission.id;
            Checkbox::new(format!("default-create-role-permission-{permission_id}"))
                .label(format!("{}（{}）", permission.name, permission.key))
                .checked(self.selected_permission_ids.contains(&permission_id))
                .disabled(self.saving)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_permission(permission_id, *checked, cx);
                }))
        });
        let dialog = cx.entity().downgrade();
        let can_close = !self.saving;

        PanelDialog::new(
            "default-account-create-role-dialog",
            self.focus_handle.clone(),
        )
        .title("创建自定义角色")
        .overlay_closable(can_close)
        .on_close(move |_, window, cx| {
            if can_close {
                _ = dialog.update(cx, |dialog, cx| dialog.close(window, cx));
            }
        })
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
        )
        .footer(
            h_flex()
                .gap_2()
                .child(
                    Button::new("cancel-default-create-role")
                        .outline()
                        .label("取消")
                        .disabled(self.saving)
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                )
                .child(
                    Button::new("submit-default-create-role")
                        .primary()
                        .label("创建角色")
                        .loading(self.saving)
                        .disabled(self.saving)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.create_role(window, cx);
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
