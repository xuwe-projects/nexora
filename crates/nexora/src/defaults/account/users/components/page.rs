//! 默认用户管理页面状态。

use gpui::{
    Anchor, App, Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*,
    px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};

use crate::{
    defaults::account::has_permission,
    desktop::{
        AccountClientError, CrudPanel, api_session,
        contract::{RoleResponse, UpdateUserStatusRequest, UserStatus},
    },
};

use super::table::{UserFilters, UserStatusFilter, UserTypeFilter};
use super::{ProvisionUserDialog, UserRoleEditor, UsersTable};

const PAGE_SIZE: u32 = 25;

#[derive(Clone, Copy, PartialEq, Eq)]
enum UserLoadMode {
    Replace,
    Append,
}

pub(in crate::defaults::account::users) struct UsersPage {
    roles: Vec<RoleResponse>,
    page: u32,
    total: i64,
    loaded: bool,
    loading: bool,
    busy_user_id: Option<String>,
    error: Option<String>,
    notice: Option<String>,
    keyword_input: Entity<InputState>,
    status_filter: UserStatusFilter,
    type_filter: UserTypeFilter,
    applied_filters: UserFilters,
    users_table: Entity<UsersTable>,
    role_editor: Entity<UserRoleEditor>,
    provision_dialog: Option<WeakEntity<ProvisionUserDialog>>,
    _role_editor_subscription: Subscription,
    _load_task: Option<Task<()>>,
    _mutation_task: Option<Task<()>>,
}

impl UsersPage {
    pub(in crate::defaults::account::users) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let page = cx.entity().downgrade();
        let keyword_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索用户、登录名或邮箱"));
        let users_table = cx.new(|cx| UsersTable::new(page, window, cx));
        let role_editor = cx.new(UserRoleEditor::new);
        let table = users_table.downgrade();
        let role_editor_subscription = cx.observe(&role_editor, move |_, _, cx| {
            _ = table.update(cx, |table, cx| table.refresh_actions(cx));
            cx.notify();
        });
        Self {
            roles: Vec::new(),
            page: 0,
            total: 0,
            loaded: false,
            loading: false,
            busy_user_id: None,
            error: None,
            notice: None,
            keyword_input,
            status_filter: UserStatusFilter::default(),
            type_filter: UserTypeFilter::default(),
            applied_filters: UserFilters::default(),
            users_table,
            role_editor,
            provision_dialog: None,
            _role_editor_subscription: role_editor_subscription,
            _load_task: None,
            _mutation_task: None,
        }
    }

    pub(in crate::defaults::account::users) fn set_provision_dialog(
        &mut self,
        dialog: WeakEntity<ProvisionUserDialog>,
        cx: &mut Context<Self>,
    ) {
        self.provision_dialog = Some(dialog);
        cx.notify();
    }

    pub(in crate::defaults::account::users) fn role_editor(&self) -> Entity<UserRoleEditor> {
        self.role_editor.clone()
    }

    pub(in crate::defaults::account::users) fn load_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            self.load_page(1, UserLoadMode::Replace, cx);
        }
    }

    pub(super) fn load_next_page(&mut self, cx: &mut Context<Self>) {
        let loaded_count = self.users_table.read(cx).loaded_len();
        let total = usize::try_from(self.total.max(0)).unwrap_or(usize::MAX);
        if self.loaded && !self.loading && self.applied_filters.is_empty() && loaded_count < total {
            self.load_page(self.page.saturating_add(1), UserLoadMode::Append, cx);
        }
    }

    pub(super) fn user_provisioned(&mut self, display_name: String, cx: &mut Context<Self>) {
        self.notice = Some(format!("用户“{display_name}”已创建"));
        self.load_page(1, UserLoadMode::Replace, cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load_page(1, UserLoadMode::Replace, cx);
    }

    fn load_page(&mut self, page: u32, mode: UserLoadMode, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        self.refresh_table(cx);
        let load_roles = mode == UserLoadMode::Replace && has_permission(cx, "roles:read");
        let background = cx.background_spawn(async move {
            let users = session.list_users(page, PAGE_SIZE)?;
            let roles = if load_roles {
                Some(session.list_roles()?)
            } else {
                None
            };
            Ok::<_, AccountClientError>((users, roles))
        });
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok((users, roles)) => {
                        this.page = users.page.number;
                        this.total = users.page.total;
                        if let Some(roles) = roles {
                            this.roles = roles;
                        }
                        let filters = this.applied_filters.clone();
                        this.users_table.update(cx, |table, cx| match mode {
                            UserLoadMode::Replace => {
                                table.replace_rows(users.items, users.page.total, &filters, cx)
                            }
                            UserLoadMode::Append => {
                                table.append_rows(users.items, users.page.total, &filters, cx)
                            }
                        });
                        this.loaded = true;
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                this.refresh_table(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn open_provision_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.provision_dialog {
            let roles = self.roles.clone();
            _ = dialog.update(cx, |dialog, cx| dialog.open(roles, window, cx));
        }
    }

    pub(super) fn manage_roles(
        &mut self,
        user_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading || self.role_editor.read(cx).is_busy() {
            return;
        }
        let roles = self.roles.clone();
        self.role_editor.update(cx, |editor, cx| {
            editor.open(user_id, roles, window, cx);
        });
        self.refresh_table(cx);
    }

    pub(super) fn set_user_status(
        &mut self,
        user_id: String,
        status: UserStatus,
        cx: &mut Context<Self>,
    ) {
        if self.loading || self.busy_user_id.is_some() {
            return;
        }
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.busy_user_id = Some(user_id.clone());
        self.error = None;
        self.notice = None;
        self.refresh_table(cx);
        let background = cx.background_spawn(async move {
            session.update_user_status(user_id.as_str(), &UpdateUserStatusRequest { status })
        });
        self._mutation_task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.busy_user_id = None;
                match result {
                    Ok(updated) => {
                        let filters = this.applied_filters.clone();
                        this.users_table.update(cx, |table, cx| {
                            table.update_user(updated.clone(), &filters, cx);
                        });
                        let action = match updated.status {
                            UserStatus::Active => "启用",
                            UserStatus::Suspended => "停用",
                        };
                        this.notice = Some(format!("用户“{}”已{action}", updated.display_name));
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                this.refresh_table(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn is_user_busy(&self, user_id: &str) -> bool {
        self.busy_user_id.as_deref() == Some(user_id)
    }

    pub(super) fn has_active_mutation(&self, cx: &App) -> bool {
        self.loading || self.busy_user_id.is_some() || self.role_editor.read(cx).is_busy()
    }

    fn refresh_table(&self, cx: &mut Context<Self>) {
        let loading = self.loading;
        self.users_table
            .update(cx, |table, cx| table.refresh(loading, cx));
    }

    fn set_status_filter(&mut self, filter: UserStatusFilter, cx: &mut Context<Self>) {
        self.status_filter = filter;
        cx.notify();
    }

    fn set_type_filter(&mut self, filter: UserTypeFilter, cx: &mut Context<Self>) {
        self.type_filter = filter;
        cx.notify();
    }

    fn apply_filters(&mut self, cx: &mut Context<Self>) {
        let keyword = self
            .keyword_input
            .read(cx)
            .value()
            .trim()
            .to_ascii_lowercase();
        self.applied_filters = UserFilters::new(keyword, self.status_filter, self.type_filter);
        let filters = self.applied_filters.clone();
        self.users_table
            .update(cx, |table, cx| table.apply_filters(&filters, cx));
        cx.notify();
    }
}

impl Render for UsersPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let can_provision = has_permission(cx, "users:provision");
        let loaded_count = self.users_table.read(cx).loaded_len();
        let visible_count = self.users_table.read(cx).visible_len(cx);
        let query_action = Button::new("query-default-account-users")
            .with_size(component_size)
            .outline()
            .icon(IconName::Search)
            .label("查询")
            .disabled(self.loading)
            .on_click(cx.listener(|this, _, _, cx| this.apply_filters(cx)));
        let create_user_action = Button::new("open-default-account-user-dialog")
            .debug_selector(|| "open-default-account-user-dialog".into())
            .with_size(component_size)
            .primary()
            .icon(IconName::Plus)
            .label("创建用户")
            .disabled(self.loading || !can_provision)
            .tooltip(if can_provision {
                "创建用户"
            } else {
                "当前账号不能创建用户"
            })
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_provision_dialog(window, cx);
            }));
        let page = cx.entity().downgrade();
        let status_filter = filter_dropdown(
            "default-account-user-status-filter",
            self.status_filter.label(),
            UserStatusFilter::ALL.map(|filter| (filter.label(), filter)),
            self.status_filter,
            page.clone(),
            |page, filter, cx| page.set_status_filter(filter, cx),
            component_size,
        );
        let type_filter = filter_dropdown(
            "default-account-user-type-filter",
            self.type_filter.label(),
            UserTypeFilter::ALL.map(|filter| (filter.label(), filter)),
            self.type_filter,
            page,
            |page, filter, cx| page.set_type_filter(filter, cx),
            component_size,
        );
        let keyword_filter = v_flex()
            .gap_1()
            .w(px(280.0))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("关键词"),
            )
            .child(
                Input::new(&self.keyword_input)
                    .with_size(component_size)
                    .cleanable(true)
                    .disabled(self.loading),
            );
        let status_filter = v_flex()
            .gap_1()
            .w(px(160.0))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("状态"),
            )
            .child(status_filter);
        let type_filter = v_flex()
            .gap_1()
            .w(px(160.0))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("类型"),
            )
            .child(type_filter);

        let content = v_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .gap_4()
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    Alert::error("default-account-users-error", error)
                        .title("用户操作失败")
                        .flex_shrink_0(),
                )
            })
            .when_some(self.notice.clone(), |this, notice| {
                this.child(Alert::success("default-account-users-notice", notice).flex_shrink_0())
            })
            .child(self.users_table.clone());

        CrudPanel::new("用户管理", content)
            .description(format!(
                "已加载 {loaded_count} / {} 个本地用户 · 当前显示 {visible_count} 个",
                self.total.max(0),
            ))
            .refresh(
                "refresh-default-account-users",
                self.loading,
                self.loading,
                cx.listener(|this, _, _, cx| this.refresh(cx)),
            )
            .filter(keyword_filter)
            .filter(status_filter)
            .filter(type_filter)
            .action(query_action)
            .action(create_user_action)
            .with_size(component_size)
    }
}

fn filter_dropdown<T>(
    id: &'static str,
    label: &'static str,
    options: impl IntoIterator<Item = (&'static str, T)>,
    selected: T,
    page: WeakEntity<UsersPage>,
    on_select: impl Fn(&mut UsersPage, T, &mut Context<UsersPage>) + Clone + 'static,
    size: gpui_component::Size,
) -> impl IntoElement
where
    T: Copy + PartialEq + 'static,
{
    let options = options.into_iter().collect::<Vec<_>>();
    Button::new(id)
        .with_size(size)
        .outline()
        .dropdown_caret(true)
        .label(label)
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
            options.iter().fold(menu, |menu, (label, filter)| {
                let page = page.clone();
                let on_select = on_select.clone();
                let filter = *filter;
                menu.item(
                    PopupMenuItem::new(*label)
                        .checked(filter == selected)
                        .on_click(move |_, _, cx| {
                            _ = page.update(cx, |page, cx| on_select(page, filter, cx));
                        }),
                )
            })
        })
}
