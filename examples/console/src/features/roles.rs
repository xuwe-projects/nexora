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
    AnyElement, Context, Entity, FocusHandle, IntoElement as _, Render, Task, WeakEntity,
    WeakFocusHandle, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    accordion::Accordion,
    alert::Alert,
    button::{Button, ButtonVariant, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    form::{field, v_form},
    h_flex,
    input::{Input, InputState},
    spinner::Spinner,
    tag::Tag,
    v_flex,
};
use ui::PanelDialog;

use crate::{
    account_api::{AccountApi, AccountApiError},
    auth,
};

struct RoleCreateDialog {
    roles_feature: WeakEntity<RolesFeature>,
    role_key: Entity<InputState>,
    role_name: Entity<InputState>,
    description: Entity<InputState>,
    focus_handle: FocusHandle,
    previous_focus: Option<WeakFocusHandle>,
    open: bool,
    saving: bool,
    error: Option<String>,
    _create_task: Option<Task<()>>,
}

impl RoleCreateDialog {
    fn new(
        roles_feature: WeakEntity<RolesFeature>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            roles_feature,
            role_key: cx.new(|cx| InputState::new(window, cx).placeholder("例如：project_manager")),
            role_name: cx.new(|cx| InputState::new(window, cx).placeholder("角色名称")),
            description: cx.new(|cx| InputState::new(window, cx).placeholder("可选角色说明")),
            focus_handle: cx.focus_handle(),
            previous_focus: None,
            open: false,
            saving: false,
            error: None,
            _create_task: None,
        }
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            return;
        }

        self.previous_focus = window.focused(cx).map(|handle| handle.downgrade());
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

    fn create_role(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = auth::api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let request = CreateRoleRequest {
            key: self.role_key.read(cx).value().trim().to_owned(),
            name: self.role_name.read(cx).value().trim().to_owned(),
            description: optional_text(self.description.read(cx).value().as_ref()),
            permission_ids: Vec::new(),
        };
        self.saving = true;
        self.error = None;
        let roles_feature = self.roles_feature.clone();
        let background =
            cx.background_spawn(async move { AccountApi::new(session)?.create_role(&request) });
        self._create_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(role) => {
                        let role_name = role.name.clone();
                        _ = roles_feature.update(cx, |roles, cx| {
                            roles.roles.push(role);
                            roles.roles.sort_by_key(|role| role.id);
                            roles.notice =
                                Some(format!("角色“{role_name}”已创建，可继续展开配置权限"));
                            roles.error = None;
                            cx.notify();
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
        for input in [&self.role_key, &self.role_name, &self.description] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }
}

impl Render for RoleCreateDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }

        let dialog = cx.entity().downgrade();
        let can_close = !self.saving;
        PanelDialog::new("create-role-panel-dialog", self.focus_handle.clone())
            .title("创建自定义角色")
            .overlay_closable(can_close)
            .on_close(move |_, window, cx| {
                if can_close {
                    _ = dialog.update(cx, |dialog, cx| dialog.close(window, cx));
                }
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("create-role-error", error).title("角色创建失败"))
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
            .footer(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("cancel-create-role")
                            .outline()
                            .label("取消")
                            .disabled(self.saving)
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    )
                    .child(
                        Button::new("submit-create-role")
                            .primary()
                            .label("创建角色")
                            .loading(self.saving)
                            .disabled(self.saving)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.create_role(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// 角色管理页面状态与异步生命周期。
#[derive(nexora::Feature)]
#[nexora(
    title = "角色管理",
    path = "/roles",
    section = "访问控制",
    icon = "asterisk",
    order = 40,
    factory = RolesFeature::new
)]
pub struct RolesFeature {
    roles: Vec<RoleResponse>,
    permissions: Vec<PermissionResponse>,
    selected_role_id: Option<i64>,
    selected_permission_ids: BTreeSet<i64>,
    create_dialog: Entity<RoleCreateDialog>,
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
        let roles_feature = cx.entity().downgrade();
        Self {
            roles: Vec::new(),
            permissions: Vec::new(),
            selected_role_id: None,
            selected_permission_ids: BTreeSet::new(),
            create_dialog: cx.new(|cx| RoleCreateDialog::new(roles_feature, window, cx)),
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
        if self.selected_role_id == Some(role_id) {
            return;
        }
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

    fn sync_open_role(
        &mut self,
        open_indices: &[usize],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let role_id = open_indices
            .first()
            .and_then(|index| self.roles.get(*index))
            .map(|role| role.id);
        if role_id == self.selected_role_id {
            return;
        }

        if let Some(role_id) = role_id {
            self.select_role(role_id, window, cx);
        } else {
            self.selected_role_id = None;
            self.selected_permission_ids.clear();
            cx.notify();
        }
    }

    fn open_create_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.create_dialog
            .update(cx, |dialog, cx| dialog.open(window, cx));
    }

    /// 返回应挂载到工作区右侧 Panel 的角色创建对话框层。
    ///
    /// 对话框 Entity 始终由角色 Feature 持有，因此切换到其他标签只会暂时停止渲染，
    /// 已打开状态和未提交输入不会被清理。
    pub(crate) fn panel_dialog(&self) -> AnyElement {
        self.create_dialog.clone().into_any_element()
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

    fn render_roles_accordion(&self, cx: &mut Context<Self>) -> AnyElement {
        self.roles
            .iter()
            .fold(Accordion::new("roles-accordion"), |accordion, role| {
                let open = self.selected_role_id == Some(role.id);
                let role_header = self.render_role_header(role, cx);

                accordion.item(|item| {
                    item.open(open)
                        .title(role_header)
                        .when(open, |item| item.child(self.render_role_editor(role, cx)))
                })
            })
            .on_toggle_click(cx.listener(|this, open_indices: &[usize], window, cx| {
                this.sync_open_role(open_indices, window, cx);
            }))
            .into_any_element()
    }

    fn render_role_header(&self, role: &RoleResponse, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .flex_1()
            .min_w_0()
            .justify_between()
            .gap_3()
            .child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(div().font_semibold().child(role.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(role.key.clone()),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(if role.is_system {
                        Tag::secondary().small().child("内置").into_any_element()
                    } else {
                        Tag::info().small().child("自定义").into_any_element()
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} 项权限", role.permissions.len())),
                    ),
            )
            .into_any_element()
    }

    fn render_role_editor(&self, role: &RoleResponse, cx: &mut Context<Self>) -> AnyElement {
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

        v_flex()
            .gap_4()
            .when_some(role.description.clone(), |this, description| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
            })
            .when(immutable, |this| {
                this.child(Alert::info(
                    format!("system-role-immutable-{role_id}"),
                    "内置角色不能修改、删除或重新配置权限。管理员仍按该角色的权限正常参与校验。",
                ))
            })
            .child(
                v_form()
                    .columns(2)
                    .child(
                        field()
                            .label("角色名称")
                            .child(Input::new(&self.edit_name).disabled(immutable || self.saving)),
                    )
                    .child(field().label("说明").child(
                        Input::new(&self.edit_description).disabled(immutable || self.saving),
                    )),
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
                            Button::new(format!("delete-role-{role_id}"))
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
                                            .description(
                                                "删除后，所有用户与该角色的关联都会同时移除。",
                                            )
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
                                    Button::new(format!("save-role-metadata-{role_id}"))
                                        .outline()
                                        .label("保存基本信息")
                                        .disabled(self.saving)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.save_role_metadata(cx);
                                        })),
                                )
                                .child(
                                    Button::new(format!("save-role-permissions-{role_id}"))
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
            .into_any_element()
    }
}

impl nexora::FeatureElement for RolesFeature {
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
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("open-create-role-dialog")
                                    .debug_selector(|| "open-create-role-dialog".into())
                                    .primary()
                                    .icon(IconName::Plus)
                                    .label("创建角色")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_create_dialog(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("refresh-roles")
                                    .outline()
                                    .icon(IconName::Loader)
                                    .label("刷新")
                                    .disabled(self.loading)
                                    .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                            ),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("roles-error", error).title("角色操作失败"))
            })
            .when_some(self.notice.clone(), |this, notice| {
                this.child(Alert::success("roles-notice", notice))
            })
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
                this.child(self.render_roles_accordion(cx))
            })
            .when(
                self.loaded && !self.loading && self.roles.is_empty(),
                |this| this.child(Alert::info("roles-empty", "当前系统没有角色")),
            )
    }

    fn activated(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.load_if_needed(cx);
    }
}

fn optional_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
