//! 默认角色管理页面状态。

use gpui::{
    Anchor, Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
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

pub(in crate::defaults::account::roles) struct RolesPage {
    roles: Vec<RoleResponse>,
    permissions: Vec<PermissionResponse>,
    selected_role_id: Option<i64>,
    editor: Option<Entity<RoleEditor>>,
    create_dialog: Option<WeakEntity<RoleCreateDialog>>,
    _editor_subscription: Option<Subscription>,
    keyword_input: Entity<InputState>,
    kind_filter: RoleKindFilter,
    applied_filters: RoleFilters,
    loaded: bool,
    loading: bool,
    error: Option<String>,
    notice: Option<String>,
    _load_task: Option<Task<()>>,
}

impl RolesPage {
    pub(in crate::defaults::account::roles) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            roles: Vec::new(),
            permissions: Vec::new(),
            selected_role_id: None,
            editor: None,
            create_dialog: None,
            _editor_subscription: None,
            keyword_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("搜索角色名称、键或说明")),
            kind_filter: RoleKindFilter::default(),
            applied_filters: RoleFilters::default(),
            loaded: false,
            loading: false,
            error: None,
            notice: None,
            _load_task: None,
        }
    }

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

    fn set_kind_filter(&mut self, filter: RoleKindFilter, cx: &mut Context<Self>) {
        self.kind_filter = filter;
        cx.notify();
    }

    fn apply_filters(&mut self, cx: &mut Context<Self>) {
        let keyword = self
            .keyword_input
            .read(cx)
            .value()
            .trim()
            .to_ascii_lowercase();
        self.applied_filters = RoleFilters::new(keyword, self.kind_filter);
        cx.notify();
    }
}

impl Render for RolesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
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
        let filtered_roles = self
            .roles
            .iter()
            .filter(|role| self.applied_filters.matches(role))
            .cloned()
            .collect::<Vec<_>>();
        let list = RolesList::new(
            filtered_roles.clone(),
            selected_role_id,
            self.loading || editor_busy,
            cx.entity().downgrade(),
        );
        let query_action = Button::new("query-default-account-roles")
            .with_size(component_size)
            .outline()
            .icon(IconName::Search)
            .label("查询")
            .disabled(self.loading || editor_busy)
            .on_click(cx.listener(|this, _, _, cx| this.apply_filters(cx)));
        let create_role_action = Button::new("open-default-account-role-dialog")
            .debug_selector(|| "open-default-account-role-dialog".into())
            .with_size(component_size)
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
        let page = cx.entity().downgrade();
        let kind_filter = filter_dropdown(
            "default-account-role-kind-filter",
            self.kind_filter.label(),
            RoleKindFilter::ALL.map(|filter| (filter.label(), filter)),
            self.kind_filter,
            page,
            |page, filter, cx| page.set_kind_filter(filter, cx),
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
                    .disabled(self.loading || editor_busy),
            );
        let kind_filter = v_flex()
            .gap_1()
            .w(px(160.0))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("类型"),
            )
            .child(kind_filter);

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
            .when(!filtered_roles.is_empty(), |this| this.child(list))
            .when(
                self.loaded && !self.loading && self.roles.is_empty(),
                |this| {
                    this.child(Alert::info(
                        "default-account-roles-empty",
                        "当前系统没有角色。",
                    ))
                },
            )
            .when(
                self.loaded && !self.loading && !self.roles.is_empty() && filtered_roles.is_empty(),
                |this| {
                    this.child(Alert::info(
                        "default-account-roles-filter-empty",
                        "没有匹配当前筛选条件的角色。",
                    ))
                },
            );

        CrudPanel::new("角色与权限", content)
            .description(format!(
                "{} 个角色 · 当前显示 {} 个 · {} 项可分配权限",
                self.roles.len(),
                filtered_roles.len(),
                self.permissions.len()
            ))
            .refresh(
                "refresh-default-account-roles",
                self.loading,
                self.loading || editor_busy,
                cx.listener(|this, _, _, cx| this.load(cx)),
            )
            .filter(keyword_filter)
            .filter(kind_filter)
            .action(query_action)
            .action(create_role_action)
            .with_size(component_size)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RoleFilters {
    keyword: String,
    kind: RoleKindFilter,
}

impl RoleFilters {
    fn new(keyword: impl Into<String>, kind: RoleKindFilter) -> Self {
        Self {
            keyword: keyword.into(),
            kind,
        }
    }

    fn matches(&self, role: &RoleResponse) -> bool {
        if !self.kind.matches(role) {
            return false;
        }
        if self.keyword.is_empty() {
            return true;
        }

        let keyword = self.keyword.as_str();
        role.key.to_ascii_lowercase().contains(keyword)
            || role.name.to_ascii_lowercase().contains(keyword)
            || role
                .description
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains(keyword)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RoleKindFilter {
    #[default]
    All,
    System,
    Custom,
}

impl RoleKindFilter {
    const ALL: [Self; 3] = [Self::All, Self::System, Self::Custom];

    fn label(self) -> &'static str {
        match self {
            Self::All => "全部类型",
            Self::System => "内置角色",
            Self::Custom => "自定义角色",
        }
    }

    fn matches(self, role: &RoleResponse) -> bool {
        match self {
            Self::All => true,
            Self::System => role.is_system,
            Self::Custom => !role.is_system,
        }
    }
}

fn filter_dropdown<T>(
    id: &'static str,
    label: &'static str,
    options: impl IntoIterator<Item = (&'static str, T)>,
    selected: T,
    page: WeakEntity<RolesPage>,
    on_select: impl Fn(&mut RolesPage, T, &mut Context<RolesPage>) + Clone + 'static,
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
