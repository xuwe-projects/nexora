//! Console 角色与权限管理功能。

use std::collections::BTreeSet;

use contracts::{
    account::{
        CreateRoleRequest, PermissionResponse, ReplaceRolePermissionsRequest, RoleResponse,
        UpdateRoleRequest,
    },
    patch::PatchField,
};
use gpui::{
    AnyElement, Context, Entity, IntoElement as _, Render, Task, WeakEntity, Window, div,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    alert::Alert,
    button::{Button, ButtonVariant, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    form::{field, v_form},
    h_flex,
    input::{Input, InputState},
    spinner::Spinner,
    table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow},
    v_flex,
};

use crate::{
    account_api::{AccountApi, AccountApiError},
    auth,
};

/// 角色管理页面状态与异步生命周期。
pub struct RolesFeature {
    roles: Vec<RoleResponse>,
    permissions: Vec<PermissionResponse>,
    selected_role_id: Option<i64>,
    selected_permission_ids: BTreeSet<i64>,
    create_key: Entity<InputState>,
    create_name: Entity<InputState>,
    create_description: Entity<InputState>,
    edit_name: Entity<InputState>,
    edit_description: Entity<InputState>,
    loaded: bool,
    loading: bool,
    saving: bool,
    error: Option<String>,
    notice: Option<String>,
    _load_task: Option<Task<()>>,
    _mutation_task: Option<Task<()>>,
}

impl RolesFeature {
    /// 创建角色管理页面及其输入框状态，但不在构造阶段发起网络请求。
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            roles: Vec::new(),
            permissions: Vec::new(),
            selected_role_id: None,
            selected_permission_ids: BTreeSet::new(),
            create_key: cx
                .new(|cx| InputState::new(window, cx).placeholder("例如：project_manager")),
            create_name: cx.new(|cx| InputState::new(window, cx).placeholder("角色名称")),
            create_description: cx
                .new(|cx| InputState::new(window, cx).placeholder("可选角色说明")),
            edit_name: cx.new(|cx| InputState::new(window, cx).placeholder("角色名称")),
            edit_description: cx.new(|cx| InputState::new(window, cx).placeholder("可选角色说明")),
            loaded: false,
            loading: false,
            saving: false,
            error: None,
            notice: None,
            _load_task: None,
            _mutation_task: None,
        }
    }

    /// 页面首次进入时加载角色与权限目录。
    pub fn load_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            self.load(cx);
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(session) = auth::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        self.notice = None;
        let background = cx.background_spawn(async move {
            let api = AccountApi::new(session)?;
            let roles = api.list_roles()?;
            let permissions = api.list_permissions()?;
            Ok::<_, AccountApiError>((roles, permissions))
        });
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok((roles, permissions)) => {
                        this.roles = roles;
                        this.permissions = permissions;
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

    fn select_role(&mut self, role_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(role) = self.roles.iter().find(|role| role.id == role_id) else {
            return;
        };
        self.selected_role_id = Some(role_id);
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
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    fn create_role(&mut self, cx: &mut Context<Self>) {
        let Some(session) = auth::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let request = CreateRoleRequest {
            key: self.create_key.read(cx).value().trim().to_owned(),
            name: self.create_name.read(cx).value().trim().to_owned(),
            description: optional_text(self.create_description.read(cx).value().as_ref()),
            permission_ids: Vec::new(),
        };
        self.saving = true;
        self.error = None;
        self.notice = None;
        let background =
            cx.background_spawn(async move { AccountApi::new(session)?.create_role(&request) });
        self._mutation_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(role) => {
                        let role_name = role.name.clone();
                        this.roles.push(role);
                        this.roles.sort_by_key(|role| role.id);
                        this.notice = Some(format!("角色“{role_name}”已创建，可继续配置权限"));
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn save_role_metadata(&mut self, cx: &mut Context<Self>) {
        let Some(role) = self.selected_role().cloned() else {
            return;
        };
        if role.is_system {
            return;
        }
        let description = self.edit_description.read(cx).value();
        let request = UpdateRoleRequest {
            name: Some(self.edit_name.read(cx).value().trim().to_owned()),
            description: optional_text(description.as_ref())
                .map(PatchField::Value)
                .unwrap_or(PatchField::Null),
        };
        self.start_role_update(role.id, "角色基本信息已保存", cx, move |api| {
            api.update_role(role.id, &request)
        });
    }

    fn save_role_permissions(&mut self, cx: &mut Context<Self>) {
        let Some(role) = self.selected_role().cloned() else {
            return;
        };
        if role.is_system {
            return;
        }
        let request = ReplaceRolePermissionsRequest {
            permission_ids: self.selected_permission_ids.iter().copied().collect(),
        };
        self.start_role_update(role.id, "角色权限已保存", cx, move |api| {
            api.replace_role_permissions(role.id, &request)
        });
    }

    fn start_role_update(
        &mut self,
        role_id: i64,
        success_message: &'static str,
        cx: &mut Context<Self>,
        operation: impl FnOnce(&AccountApi) -> Result<RoleResponse, AccountApiError> + Send + 'static,
    ) {
        let Some(session) = auth::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.saving = true;
        self.error = None;
        self.notice = None;
        let background = cx.background_spawn(async move {
            let api = AccountApi::new(session)?;
            operation(&api)
        });
        self._mutation_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(updated) => {
                        if let Some(role) = this.roles.iter_mut().find(|role| role.id == role_id) {
                            *role = updated.clone();
                        }
                        this.selected_permission_ids = updated
                            .permissions
                            .iter()
                            .map(|permission| permission.id)
                            .collect();
                        this.notice = Some(success_message.to_owned());
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn delete_role(&mut self, role_id: i64, cx: &mut Context<Self>) {
        let Some(session) = auth::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.saving = true;
        self.error = None;
        self.notice = None;
        let background =
            cx.background_spawn(async move { AccountApi::new(session)?.delete_role(role_id) });
        self._mutation_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => {
                        this.roles.retain(|role| role.id != role_id);
                        this.selected_role_id = None;
                        this.selected_permission_ids.clear();
                        this.notice = Some("角色已删除".to_owned());
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
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

    fn selected_role(&self) -> Option<&RoleResponse> {
        let selected_role_id = self.selected_role_id?;
        self.roles.iter().find(|role| role.id == selected_role_id)
    }

    fn render_create_form(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tokens.group_box)
            .child(div().font_semibold().child("创建自定义角色"))
            .child(
                v_form()
                    .columns(3)
                    .child(
                        field()
                            .label("角色键")
                            .required(true)
                            .child(Input::new(&self.create_key)),
                    )
                    .child(
                        field()
                            .label("角色名称")
                            .required(true)
                            .child(Input::new(&self.create_name)),
                    )
                    .child(
                        field()
                            .label("说明")
                            .child(Input::new(&self.create_description)),
                    ),
            )
            .child(
                h_flex().justify_end().child(
                    Button::new("create-role")
                        .primary()
                        .icon(IconName::Plus)
                        .label("创建角色")
                        .loading(self.saving)
                        .disabled(self.saving)
                        .on_click(cx.listener(|this, _, _, cx| this.create_role(cx))),
                ),
            )
            .into_any_element()
    }

    fn render_roles_table(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.roles.iter().map(|role| {
            let role_id = role.id;
            let permissions = if role.permissions.is_empty() {
                "未绑定权限".to_owned()
            } else {
                role.permissions
                    .iter()
                    .map(|permission| permission.name.as_str())
                    .collect::<Vec<_>>()
                    .join("、")
            };
            TableRow::new()
                .child(
                    TableCell::new().child(
                        v_flex()
                            .gap_1()
                            .child(if role.is_system {
                                format!("{} · 内置", role.name)
                            } else {
                                role.name.clone()
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(role.key.clone()),
                            ),
                    ),
                )
                .child(TableCell::new().child(permissions))
                .child(
                    TableCell::new().w(px(100.)).child(
                        Button::new(format!("edit-role-{role_id}"))
                            .small()
                            .label(if role.is_system { "查看" } else { "编辑" })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_role(role_id, window, cx);
                            })),
                    ),
                )
        });

        Table::new()
            .child(
                TableHeader::new().child(
                    TableRow::new()
                        .child(TableHead::new().child("角色"))
                        .child(TableHead::new().child("直接权限"))
                        .child(TableHead::new().w(px(100.)).child("操作")),
                ),
            )
            .child(TableBody::new().children(rows))
            .into_any_element()
    }

    fn render_role_editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let role = self.selected_role()?.clone();
        let role_id = role.id;
        let immutable = role.is_system;
        let permission_options = self.permissions.iter().map(|permission| {
            let permission_id = permission.id;
            Checkbox::new(format!("role-permission-{role_id}-{permission_id}"))
                .label(format!("{}（{}）", permission.name, permission.key))
                .checked(self.selected_permission_ids.contains(&permission_id))
                .disabled(immutable || self.saving)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_permission(permission_id, *checked, cx);
                }))
        });

        Some(
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
                                .child(div().font_semibold().child(format!(
                                    "{}角色：{}",
                                    if immutable { "查看内置" } else { "编辑" },
                                    role.name
                                )))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("稳定键：{}", role.key)),
                                ),
                        )
                        .child(
                            Button::new("close-role-editor")
                                .ghost()
                                .small()
                                .icon(IconName::Close)
                                .tooltip("关闭角色编辑")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.selected_role_id = None;
                                    this.selected_permission_ids.clear();
                                    cx.notify();
                                })),
                        ),
                )
                .when(immutable, |this| {
                    this.child(Alert::info(
                        "system-role-immutable",
                        "内置角色不能修改、删除或重新配置权限。管理员仍按该角色的权限正常参与校验。",
                    ))
                })
                .child(
                    v_form()
                        .columns(2)
                        .child(
                            field().label("角色名称").child(
                                Input::new(&self.edit_name)
                                    .disabled(immutable || self.saving),
                            ),
                        )
                        .child(
                            field().label("说明").child(
                                Input::new(&self.edit_description)
                                    .disabled(immutable || self.saving),
                            ),
                        ),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child("绑定权限"))
                        .children(permission_options),
                )
                .when(!immutable, |this| {
                    let feature: WeakEntity<Self> = cx.entity().downgrade();
                    this.child(
                        h_flex()
                            .justify_between()
                            .child(
                                Button::new("delete-role")
                                    .danger()
                                    .outline()
                                    .label("删除角色")
                                    .disabled(self.saving)
                                    .on_click(move |_, window, cx| {
                                        let feature = feature.clone();
                                        window.open_alert_dialog(cx, move |dialog, _, _| {
                                            let feature = feature.clone();
                                            dialog
                                                .title("删除角色")
                                                .description("删除后，所有用户与该角色的关联都会同时移除。")
                                                .button_props(
                                                    DialogButtonProps::default()
                                                        .ok_text("确认删除")
                                                        .ok_variant(ButtonVariant::Danger)
                                                        .cancel_text("取消")
                                                        .show_cancel(true),
                                                )
                                                .on_ok(move |_, _, cx| {
                                                    _ = feature.update(cx, |this, cx| {
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
                                        Button::new("save-role-metadata")
                                            .outline()
                                            .label("保存基本信息")
                                            .disabled(self.saving)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.save_role_metadata(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("save-role-permissions")
                                            .primary()
                                            .label("保存权限")
                                            .loading(self.saving)
                                            .disabled(self.saving)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.save_role_permissions(cx);
                                            })),
                                    ),
                            ),
                    )
                })
                .into_any_element(),
        )
    }
}

impl Render for RolesFeature {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .child(div().text_xl().font_bold().child("角色管理"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} 个角色 · {} 项权限",
                                        self.roles.len(),
                                        self.permissions.len()
                                    )),
                            ),
                    )
                    .child(
                        Button::new("refresh-roles")
                            .outline()
                            .icon(IconName::Loader)
                            .label("刷新")
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("roles-error", error).title("角色操作失败"))
            })
            .when_some(self.notice.clone(), |this, notice| {
                this.child(Alert::success("roles-notice", notice))
            })
            .child(self.render_create_form(cx))
            .when(self.loading && self.roles.is_empty(), |this| {
                this.child(
                    h_flex()
                        .justify_center()
                        .gap_2()
                        .py_8()
                        .child(Spinner::new().small())
                        .child("正在加载角色与权限..."),
                )
            })
            .when(!self.roles.is_empty(), |this| {
                this.child(self.render_roles_table(cx))
            })
            .when(
                self.loaded && !self.loading && self.roles.is_empty(),
                |this| this.child(Alert::info("roles-empty", "当前系统没有角色")),
            )
            .when_some(self.render_role_editor(cx), |this, editor| {
                this.child(editor)
            })
    }
}

fn optional_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
