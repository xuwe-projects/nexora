//! 用户直接角色完整替换组件。

use std::collections::BTreeSet;

use gpui::{Context, Render, Task, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    spinner::Spinner,
    v_flex,
};

use crate::defaults::account::has_permission;
use crate::desktop::{
    api_session,
    contract::{AccessProfileResponse, ReplaceUserRolesRequest, RoleResponse},
};

#[derive(Default)]
pub(in crate::defaults::account::users) struct UserRoleEditor {
    open: bool,
    loading: bool,
    saving: bool,
    roles: Vec<RoleResponse>,
    profile: Option<AccessProfileResponse>,
    selected_role_ids: BTreeSet<i64>,
    error: Option<String>,
    notice: Option<String>,
    _task: Option<Task<()>>,
}

impl UserRoleEditor {
    pub(super) const fn is_busy(&self) -> bool {
        self.loading || self.saving
    }

    pub(super) fn open(
        &mut self,
        user_id: String,
        roles: Vec<RoleResponse>,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() {
            return;
        }
        let Some(session) = api_session(cx) else {
            self.open = true;
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.open = true;
        self.loading = true;
        self.roles = roles;
        self.profile = None;
        self.selected_role_ids.clear();
        self.error = None;
        self.notice = None;
        let background = cx.background_spawn(async move { session.get_user(user_id.as_str()) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(profile) => {
                        this.selected_role_ids = profile.roles.iter().map(|role| role.id).collect();
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

    fn close(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.saving {
            return;
        }
        self.open = false;
        self.profile = None;
        self.selected_role_ids.clear();
        self.error = None;
        self.notice = None;
        cx.notify();
    }

    fn toggle_role(&mut self, role_id: i64, checked: bool, cx: &mut Context<Self>) {
        if checked {
            self.selected_role_ids.insert(role_id);
        } else {
            self.selected_role_ids.remove(&role_id);
        }
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
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
            role_ids: self.selected_role_ids.iter().copied().collect(),
        };
        self.saving = true;
        self.error = None;
        self.notice = None;
        let background =
            cx.background_spawn(
                async move { session.replace_user_roles(user_id.as_str(), &request) },
            );
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(profile) => {
                        this.selected_role_ids = profile.roles.iter().map(|role| role.id).collect();
                        this.profile = Some(profile);
                        this.notice = Some("用户角色已保存".to_owned());
                        this.error = None;
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
        if !self.open {
            return div().into_any_element();
        }

        let role_options = self.roles.iter().map(|role| {
            let role_id = role.id;
            Checkbox::new(format!("default-assign-role-{role_id}"))
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

        v_flex()
            .gap_3()
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
                                    .font_semibold()
                                    .child(format!("为 {display_name} 分配角色")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("保存时会完整替换该用户的直接角色集合。"),
                            ),
                    )
                    .child(
                        Button::new("close-default-user-role-editor")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .tooltip("关闭角色分配")
                            .disabled(self.loading || self.saving)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("default-user-role-error", error))
            })
            .when_some(self.notice.clone(), |this, notice| {
                this.child(Alert::success("default-user-role-notice", notice))
            })
            .when(self.loading, |this| {
                this.child(
                    h_flex()
                        .justify_center()
                        .gap_2()
                        .py_6()
                        .child(Spinner::new().small())
                        .child("正在读取用户角色..."),
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
            .child(
                h_flex().justify_end().child(
                    Button::new("save-default-user-roles")
                        .primary()
                        .label("保存角色")
                        .loading(self.saving)
                        .disabled(
                            self.loading || self.saving || self.profile.is_none() || !can_save,
                        )
                        .tooltip(if can_save {
                            "完整替换该用户的直接角色集合"
                        } else {
                            "需要 users:roles.write 与 roles:read 权限"
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                ),
            )
            .into_any_element()
    }
}
