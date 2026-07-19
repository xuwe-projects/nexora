//! 默认角色管理页面状态。

use gpui::{Context, Entity, Render, Subscription, Task, WeakEntity, Window, prelude::*};
use gpui_component::{
    Disableable as _, IconName, Sizable as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    h_flex,
    spinner::Spinner,
    v_flex,
};

use crate::{
    defaults::account::has_permission,
    desktop::{
        AccountClientError, CrudPanel, api_session,
        contract::{PermissionResponse, RoleResponse},
    },
};

use super::{RoleCreateDialog, RoleEditor, RolesList};

#[derive(Default)]
pub(in crate::defaults::account::roles) struct RolesPage {
    roles: Vec<RoleResponse>,
    permissions: Vec<PermissionResponse>,
    selected_role_id: Option<i64>,
    editor: Option<Entity<RoleEditor>>,
    create_dialog: Option<WeakEntity<RoleCreateDialog>>,
    _editor_subscription: Option<Subscription>,
    loaded: bool,
    loading: bool,
    error: Option<String>,
    notice: Option<String>,
    _load_task: Option<Task<()>>,
}

impl RolesPage {
    pub(in crate::defaults::account::roles) fn set_components(
        &mut self,
        editor: Entity<RoleEditor>,
        create_dialog: WeakEntity<RoleCreateDialog>,
        cx: &mut Context<Self>,
    ) {
        self._editor_subscription = Some(cx.observe(&editor, |_, _, cx| cx.notify()));
        self.editor = Some(editor);
        self.create_dialog = Some(create_dialog);
        cx.notify();
    }

    pub(in crate::defaults::account::roles) fn load_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            self.load(cx);
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        self.notice = None;
        let can_read_permissions = has_permission(cx, "permissions:read");
        let background = cx.background_spawn(async move {
            let roles = session.list_roles()?;
            let permissions = if can_read_permissions {
                session.list_permissions()?
            } else {
                Vec::new()
            };
            Ok::<_, AccountClientError>((roles, permissions))
        });
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok((roles, permissions)) => {
                        this.roles = roles;
                        this.permissions = permissions;
                        this.selected_role_id = None;
                        if let Some(editor) = &this.editor {
                            editor.update(cx, RoleEditor::clear);
                        }
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

    fn open_create_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.create_dialog {
            let permissions = self.permissions.clone();
            _ = dialog.update(cx, |dialog, cx| {
                dialog.open(permissions, window, cx);
            });
        }
    }

    pub(super) fn select_role(
        &mut self,
        role_id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading
            || self
                .editor
                .as_ref()
                .is_some_and(|editor| editor.read(cx).is_busy(cx))
        {
            return;
        }
        let Some(role) = self.roles.iter().find(|role| role.id == role_id).cloned() else {
            return;
        };
        self.selected_role_id = Some(role_id);
        if let Some(editor) = &self.editor {
            let permissions = self.permissions.clone();
            editor.update(cx, |editor, cx| {
                editor.open(role, permissions, window, cx);
            });
        }
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    pub(super) fn role_created(&mut self, role: RoleResponse, cx: &mut Context<Self>) {
        let role_name = role.name.clone();
        self.roles.push(role);
        self.roles.sort_by_key(|role| role.id);
        self.notice = Some(format!("角色“{role_name}”已创建"));
        self.error = None;
        cx.notify();
    }

    pub(super) fn role_updated(&mut self, role: RoleResponse, cx: &mut Context<Self>) {
        if let Some(current) = self.roles.iter_mut().find(|current| current.id == role.id) {
            *current = role;
        }
        cx.notify();
    }

    pub(super) fn role_deleted(&mut self, role_id: i64, cx: &mut Context<Self>) {
        self.roles.retain(|role| role.id != role_id);
        self.selected_role_id = None;
        self.notice = Some("角色已删除".to_owned());
        self.error = None;
        cx.notify();
    }
}

impl Render for RolesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_write = has_permission(cx, "roles:write");
        let can_read_permissions = has_permission(cx, "permissions:read");
        let editor_busy = self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.read(cx).is_busy(cx));
        let selected_role_id = self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.read(cx).is_open(cx))
            .then_some(self.selected_role_id)
            .flatten();
        let list = RolesList::new(
            self.roles.clone(),
            selected_role_id,
            self.loading || editor_busy,
            cx.entity().downgrade(),
        );
        let create_role_action = Button::new("open-default-account-role-dialog")
            .debug_selector(|| "open-default-account-role-dialog".into())
            .primary()
            .icon(IconName::Plus)
            .label("创建角色")
            .disabled(self.loading || !can_write)
            .tooltip(if can_write {
                "创建角色"
            } else {
                "当前账号不能创建角色"
            })
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_create_dialog(window, cx);
            }));

        let content = v_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .gap_4()
            .when(!can_read_permissions, |this| {
                this.child(Alert::info(
                    "default-account-permissions-unavailable",
                    "当前账号不能查看或替换权限集合。",
                ))
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-account-roles-error", error).title("角色操作失败"))
            })
            .when_some(self.notice.clone(), |this, notice| {
                this.child(Alert::success("default-account-roles-notice", notice))
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
            .when(!self.roles.is_empty(), |this| this.child(list))
            .when(
                self.loaded && !self.loading && self.roles.is_empty(),
                |this| {
                    this.child(Alert::info(
                        "default-account-roles-empty",
                        "当前系统没有角色。",
                    ))
                },
            );

        CrudPanel::new("角色与权限", content)
            .description(format!(
                "{} 个角色 · {} 项可分配权限",
                self.roles.len(),
                self.permissions.len()
            ))
            .refresh(
                "refresh-default-account-roles",
                self.loading,
                self.loading || editor_busy,
                cx.listener(|this, _, _, cx| this.load(cx)),
            )
            .action(create_role_action)
    }
}
