//! 使用公共 FormDialog 完整替换用户直接角色。

use std::collections::BTreeSet;

use gpui::{Context, Entity, Render, Task, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, alert::Alert, checkbox::Checkbox, h_flex,
    spinner::Spinner, v_flex,
};
use ui::{FormDialog, FormDialogState};

use crate::defaults::account::has_permission;
use crate::desktop::{
    api_session,
    contract::{AccessProfileResponse, ReplaceUserRolesRequest, RoleResponse, SYSTEM_ROLE_OWNER},
};

pub(in crate::defaults::account::users) struct UserRoleEditor {
    form: Entity<FormDialogState>,
    loading: bool,
    saving: bool,
    roles: Vec<RoleResponse>,
    profile: Option<AccessProfileResponse>,
    selected_role_ids: BTreeSet<i64>,
    original_role_ids: String,
    error: Option<String>,
    _task: Option<Task<()>>,
}

impl UserRoleEditor {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            form: cx.new(FormDialogState::new),
            loading: false,
            saving: false,
            roles: Vec::new(),
            profile: None,
            selected_role_ids: BTreeSet::new(),
            original_role_ids: String::new(),
            error: None,
            _task: None,
        }
    }

    pub(super) const fn is_busy(&self) -> bool {
        self.loading || self.saving
    }

    pub(super) fn open(
        &mut self,
        user_id: String,
        roles: Vec<RoleResponse>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() {
            return;
        }
        self.form.update(cx, |form, cx| {
            form.reset_fields(cx);
            form.open(window, cx);
        });
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.loading = true;
        self.roles = roles;
        self.profile = None;
        self.selected_role_ids.clear();
        self.original_role_ids.clear();
        self.error = None;
        let form = self.form.clone();
        let background = cx.background_spawn(async move { session.get_user(user_id.as_str()) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(profile) => {
                        this.selected_role_ids = profile.roles.iter().map(|role| role.id).collect();
                        this.original_role_ids = role_draft(&this.selected_role_ids);
                        form.update(cx, |form, cx| {
                            form.set_field_draft(
                                "role_ids",
                                "用户角色",
                                this.original_role_ids.clone(),
                                this.original_role_ids.clone(),
                                cx,
                            );
                        });
                        this.profile = Some(profile);
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn toggle_role(&mut self, role_id: i64, checked: bool, cx: &mut Context<Self>) {
        if checked {
            self.selected_role_ids.insert(role_id);
        } else {
            self.selected_role_ids.remove(&role_id);
        }
        let draft = role_draft(&self.selected_role_ids);
        self.form.update(cx, |form, cx| {
            form.set_field_draft(
                "role_ids",
                "用户角色",
                self.original_role_ids.clone(),
                draft,
                cx,
            );
        });
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let Some(profile) = &self.profile else {
            return;
        };
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let user_id = profile.user.id.clone();
        let request = ReplaceUserRolesRequest {
            owner: SYSTEM_ROLE_OWNER.to_owned(),
            role_ids: self.selected_role_ids.iter().copied().collect(),
        };
        self.saving = true;
        self.error = None;
        self.form
            .update(cx, |form, cx| form.set_submitting(true, cx));
        let form = self.form.clone();
        let background =
            cx.background_spawn(
                async move { session.replace_user_roles(user_id.as_str(), &request) },
            );
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                form.update(cx, |form, cx| form.set_submitting(false, cx));
                match result {
                    Ok(profile) => {
                        this.selected_role_ids = profile.roles.iter().map(|role| role.id).collect();
                        this.original_role_ids = role_draft(&this.selected_role_ids);
                        this.profile = Some(profile);
                        this.error = None;
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
}

impl Render for UserRoleEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let role_options = self.roles.iter().map(|role| {
            let role_id = role.id;
            Checkbox::new(format!("default-assign-role-{role_id}"))
                .with_size(component_size)
                .label(format!("{}（{}）", role.name, role.key))
                .checked(self.selected_role_ids.contains(&role_id))
                .disabled(self.loading || self.saving)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_role(role_id, *checked, cx);
                }))
        });
        let display_name = self
            .profile
            .as_ref()
            .map(|profile| profile.user.display_name.as_str())
            .unwrap_or("用户");
        let can_save = has_permission(cx, "users:roles.write") && has_permission(cx, "roles:read");
        let content = v_flex()
            .gap_3()
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-user-role-error", error))
            })
            .when(self.loading, |this| {
                this.child(
                    h_flex()
                        .justify_center()
                        .gap_2()
                        .py_6()
                        .child(Spinner::new().small())
                        .child("正在读取用户角色…"),
                )
            })
            .when(!self.loading && self.roles.is_empty(), |this| {
                this.child(Alert::info(
                    "default-user-role-empty",
                    "当前系统没有可分配角色。",
                ))
            })
            .when(!self.loading && !self.roles.is_empty(), |this| {
                this.child(v_flex().gap_2().children(role_options))
            })
            .when(!can_save, |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("当前账号不能修改用户角色。"),
                )
            });
        let editor = cx.entity().downgrade();
        FormDialog::new("default-user-role-form-dialog", self.form.clone())
            .title(format!("管理 {display_name} 的角色"))
            .description("保存该用户的角色设置。")
            .section(content)
            .submit_label("保存角色")
            .submit_disabled(self.loading || self.profile.is_none() || !can_save)
            .with_size(component_size)
            .on_submit(move |_, window, cx| {
                _ = editor.update(cx, |editor, cx| editor.save(window, cx));
            })
    }
}

fn role_draft(role_ids: &BTreeSet<i64>) -> String {
    role_ids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
