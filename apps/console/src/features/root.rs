//! Console 桌面应用的根视图。
//!
//! 该模块定义主窗口中最外层的业务视图，运行器会将其嵌入 `gpui_component::Root`。

use crate::{
    auth,
    features::{
        FeatureChildItem, FeatureId, FeatureItem, feature_catalog, home::HomeFeature,
        login::LoginFeature, projects::ProjectsFeature, tasks::TasksFeature,
        virtual_scroll::VirtualScrollFeature,
    },
};
use actions::account::{
    self as account_actions, AccountActionKind, OpenAccountProfile, SignInAccount, SignOutAccount,
};
use actions::settings::OpenSettings;
use gpui::{
    Anchor, AnyElement, Context, IntoElement, MouseButton, Render, ScrollHandle, WeakEntity,
    Window, div, prelude::*, px, rems,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
    avatar::Avatar,
    breadcrumb::{Breadcrumb, BreadcrumbItem},
    button::{Button, ButtonVariants as _, Toggle},
    h_flex,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem,
    },
    tab::{Tab, TabBar},
};
use ui::{PanelHeader, layout::WorkspaceLayout};

/// 控制台主窗口的业务根视图。
///
/// 该视图持有当前功能区、标签页和导航历史等控制台业务状态，并向共享 `WorkspaceLayout`
/// 提供侧边导航、标题栏内容和当前 feature 面板；窗口级结构与平台适配由共享布局统一处理。
pub struct RootView {
    /// 当前在主内容区展示的功能区。
    active_feature: FeatureId,
    /// 顶部标签栏中已经打开过的功能区。
    opened_tabs: Vec<FeatureId>,
    /// 顶部标签栏中被置顶的功能区。
    pinned_tabs: Vec<FeatureId>,
    /// 最近一次右键点击的标签页，用于构建标签页上下文菜单。
    tab_context_feature: Option<FeatureId>,
    /// 置顶标签区域的横向滚动句柄，用于从更多菜单选择置顶标签时自动滚动到目标标签。
    pinned_tab_scroll_handle: ScrollHandle,
    /// 普通标签区域的横向滚动句柄，用于在从更多菜单选择标签时自动滚动到目标标签。
    regular_tab_scroll_handle: ScrollHandle,
    /// 当前窗口访问过的功能区历史，用于支持顶部栏前进和后退。
    navigation_history: Vec<FeatureId>,
    /// 当前所在的历史游标位置，指向 `navigation_history` 中正在展示的功能区。
    navigation_history_index: usize,
    home_feature: HomeFeature,
    virtual_scroll_feature: VirtualScrollFeature,
}

impl Default for RootView {
    /// 创建处于首页初始状态的控制台根视图。
    ///
    /// 该实现委托给 `RootView::new`，确保默认标签页和导航历史都包含首页。
    fn default() -> Self {
        Self::new()
    }
}

impl RootView {
    /// 创建一个新的根视图。
    ///
    /// 默认会选中首页功能区，后续用户可以通过侧边栏导航切换到其他 feature。
    pub fn new() -> Self {
        Self {
            active_feature: FeatureId::default(),
            opened_tabs: vec![FeatureId::default()],
            pinned_tabs: Vec::new(),
            tab_context_feature: None,
            pinned_tab_scroll_handle: ScrollHandle::new(),
            regular_tab_scroll_handle: ScrollHandle::new(),
            navigation_history: vec![FeatureId::default()],
            navigation_history_index: 0,
            home_feature: HomeFeature::default(),
            virtual_scroll_feature: VirtualScrollFeature::default(),
        }
    }

    /// 返回当前选中的功能区。
    ///
    /// 该方法主要用于测试和后续 action 处理，避免外部直接访问内部状态字段。
    pub fn active_feature(&self) -> FeatureId {
        self.active_feature
    }

    /// 返回顶部标签栏中已经打开的功能区。
    ///
    /// 该列表按用户首次打开页面的顺序保存；重复选择同一个 feature 不会插入重复标签。
    pub fn opened_tabs(&self) -> &[FeatureId] {
        &self.opened_tabs
    }

    /// 返回顶部标签栏中被置顶的功能区。
    ///
    /// 返回顺序就是它们在标签栏左侧展示的顺序；置顶标签会排在普通标签之前。
    pub fn pinned_tabs(&self) -> &[FeatureId] {
        &self.pinned_tabs
    }

    /// 返回顶部标签栏中未置顶的普通功能区。
    ///
    /// 普通标签会进入右侧可横向滚动区域；置顶标签则固定展示在左侧，避免被滚动条隐藏。
    pub fn regular_tabs(&self) -> Vec<FeatureId> {
        self.opened_tabs()
            .iter()
            .copied()
            .filter(|feature| !self.is_tab_pinned(*feature))
            .collect()
    }

    /// 返回当前窗口的功能区访问历史。
    ///
    /// 访问历史用于驱动顶部栏前进和后退按钮；同一个连续功能区不会重复写入历史。
    pub fn navigation_history(&self) -> &[FeatureId] {
        &self.navigation_history
    }

    /// 判断顶部栏后退按钮当前是否可用。
    ///
    /// 返回 `true` 表示历史游标左侧还有更早访问过的功能区，可以调用 `navigate_back` 返回。
    pub fn can_navigate_back(&self) -> bool {
        self.navigation_history_index > 0
    }

    /// 判断顶部栏前进按钮当前是否可用。
    ///
    /// 返回 `true` 表示用户刚执行过后退，并且历史游标右侧还有可恢复的功能区。
    pub fn can_navigate_forward(&self) -> bool {
        self.navigation_history_index + 1 < self.navigation_history().len()
    }

    /// 判断指定功能区对应的标签页是否已经置顶。
    ///
    /// 置顶状态只影响标签栏排序和批量关闭行为，不改变 feature 自身的业务状态。
    pub fn is_tab_pinned(&self, feature: FeatureId) -> bool {
        self.pinned_tabs.contains(&feature)
    }

    /// 切换当前选中的功能区。
    ///
    /// RootView 只保存导航状态，各个 feature 的业务状态仍应由对应模块自行管理。
    pub fn select_feature(&mut self, feature: FeatureId) {
        self.navigate_to_feature(feature, true);
    }

    /// 按访问历史后退到上一个功能区。
    ///
    /// 后退只移动历史游标，不会追加新的历史记录；如果目标功能区对应标签已被关闭，会自动重新打开。
    pub fn navigate_back(&mut self) {
        if !self.can_navigate_back() {
            return;
        }

        self.navigation_history_index -= 1;
        if let Some(feature) = self
            .navigation_history
            .get(self.navigation_history_index)
            .copied()
        {
            self.navigate_to_feature(feature, false);
        }
    }

    /// 按访问历史前进到下一个功能区。
    ///
    /// 前进只移动历史游标，不会追加新的历史记录；如果目标功能区对应标签已被关闭，会自动重新打开。
    pub fn navigate_forward(&mut self) {
        if !self.can_navigate_forward() {
            return;
        }

        self.navigation_history_index += 1;
        if let Some(feature) = self
            .navigation_history
            .get(self.navigation_history_index)
            .copied()
        {
            self.navigate_to_feature(feature, false);
        }
    }

    /// 关闭指定功能区对应的标签页。
    ///
    /// 如果关闭的是当前激活标签，会优先激活原位置右侧的标签；没有右侧标签时回退到左侧标签。
    /// 如果所有标签都被关闭，会重新打开首页，保证应用始终有一个可展示页面。
    pub fn close_tab(&mut self, feature: FeatureId) {
        let removed_index = self
            .opened_tabs
            .iter()
            .position(|opened| *opened == feature);
        let Some(index) = removed_index else {
            return;
        };

        let closing_active = self.active_feature == feature;
        self.opened_tabs.remove(index);
        self.pinned_tabs.retain(|pinned| *pinned != feature);

        if self.opened_tabs.is_empty() {
            self.opened_tabs.push(FeatureId::default());
        }

        if closing_active {
            let fallback_index = index.min(self.opened_tabs.len().saturating_sub(1));
            if let Some(feature) = self.opened_tabs.get(fallback_index).copied() {
                self.active_feature = feature;
            }
        }

        self.ensure_active_tab();
    }

    /// 关闭指定标签页左侧的普通标签页。
    ///
    /// 已置顶标签会被保留，避免批量操作破坏用户显式固定的工作上下文。
    pub fn close_tabs_to_left(&mut self, feature: FeatureId) {
        let Some(index) = self.tab_index(feature) else {
            return;
        };

        self.opened_tabs = self
            .opened_tabs
            .iter()
            .enumerate()
            .filter_map(|(tab_index, opened)| {
                (tab_index >= index || *opened == feature || self.is_tab_pinned(*opened))
                    .then_some(*opened)
            })
            .collect();
        self.ensure_active_or_select(feature);
    }

    /// 关闭指定标签页右侧的普通标签页。
    ///
    /// 已置顶标签会被保留，目标标签本身也会始终保留。
    pub fn close_tabs_to_right(&mut self, feature: FeatureId) {
        let Some(index) = self.tab_index(feature) else {
            return;
        };

        self.opened_tabs = self
            .opened_tabs
            .iter()
            .enumerate()
            .filter_map(|(tab_index, opened)| {
                (tab_index <= index || *opened == feature || self.is_tab_pinned(*opened))
                    .then_some(*opened)
            })
            .collect();
        self.ensure_active_or_select(feature);
    }

    /// 关闭除指定标签页和置顶标签页之外的其他标签页。
    ///
    /// 当目标标签本身未置顶时，它会保留在置顶标签之后，方便用户继续操作右键选中的页面。
    pub fn close_other_tabs(&mut self, feature: FeatureId) {
        if !self.opened_tabs.contains(&feature) {
            return;
        }

        self.opened_tabs = self
            .opened_tabs
            .iter()
            .copied()
            .filter(|opened| *opened == feature || self.is_tab_pinned(*opened))
            .collect();
        self.ensure_active_or_select(feature);
        self.reorder_tabs_by_pin();
    }

    /// 切换指定标签页的置顶状态。
    ///
    /// 置顶后标签会移动到标签栏左侧；取消置顶后会回到普通标签区域，但仍保留当前打开状态。
    pub fn toggle_pin_tab(&mut self, feature: FeatureId) {
        if !self.opened_tabs.contains(&feature) {
            return;
        }

        if self.is_tab_pinned(feature) {
            self.pinned_tabs.retain(|pinned| *pinned != feature);
        } else {
            self.pinned_tabs.push(feature);
        }

        self.reorder_tabs_by_pin();
        self.scroll_tab_into_view(self.active_feature);
    }

    fn open_feature_tab(&mut self, feature: FeatureId) {
        if !self.opened_tabs.contains(&feature) {
            self.opened_tabs.push(feature);
            self.reorder_tabs_by_pin();
        }
    }

    fn select_tab(&mut self, feature: FeatureId) {
        self.navigate_to_feature(feature, true);
    }

    fn select_pinned_tab(&mut self, index: usize) {
        if let Some(feature) = self.pinned_tabs.get(index).copied() {
            self.select_tab(feature);
        }
    }

    fn select_regular_tab(&mut self, index: usize) {
        if let Some(feature) = self.regular_tabs().get(index).copied() {
            self.select_tab(feature);
        }
    }

    fn tab_index(&self, feature: FeatureId) -> Option<usize> {
        self.opened_tabs
            .iter()
            .position(|opened| *opened == feature)
    }

    fn ensure_active_tab(&mut self) {
        if self.opened_tabs.is_empty() {
            self.opened_tabs.push(FeatureId::default());
        }

        if !self.opened_tabs.contains(&self.active_feature) {
            self.active_feature = self.opened_tabs[0];
        }

        self.pinned_tabs
            .retain(|pinned| self.opened_tabs.contains(pinned));
        self.scroll_tab_into_view(self.active_feature);
    }

    fn ensure_active_or_select(&mut self, fallback: FeatureId) {
        if !self.opened_tabs.contains(&self.active_feature) {
            self.active_feature = fallback;
        }

        self.ensure_active_tab();
    }

    fn reorder_tabs_by_pin(&mut self) {
        let mut pinned = Vec::new();
        for feature in self.pinned_tabs.iter().copied() {
            if self.opened_tabs.contains(&feature) && !pinned.contains(&feature) {
                pinned.push(feature);
            }
        }

        let mut unpinned = self
            .opened_tabs
            .iter()
            .copied()
            .filter(|feature| !pinned.contains(feature))
            .collect::<Vec<_>>();

        pinned.append(&mut unpinned);
        self.opened_tabs = pinned;
        self.pinned_tabs
            .retain(|pinned| self.opened_tabs.contains(pinned));
    }

    fn active_pinned_tab_index(&self) -> Option<usize> {
        self.pinned_tab_index(self.active_feature)
    }

    fn pinned_tab_index(&self, feature: FeatureId) -> Option<usize> {
        self.pinned_tabs
            .iter()
            .position(|pinned| *pinned == feature)
    }

    fn regular_tab_index(&self, feature: FeatureId) -> Option<usize> {
        self.opened_tabs
            .iter()
            .filter(|opened| !self.is_tab_pinned(**opened))
            .position(|opened| *opened == feature)
    }

    fn active_regular_tab_index(&self) -> Option<usize> {
        self.regular_tab_index(self.active_feature)
    }

    fn scroll_tab_into_view(&self, feature: FeatureId) {
        if let Some(index) = self.pinned_tab_index(feature) {
            self.pinned_tab_scroll_handle.scroll_to_item(index);
        } else if let Some(index) = self.regular_tab_index(feature) {
            self.regular_tab_scroll_handle.scroll_to_item(index);
        }
    }

    fn navigate_to_feature(&mut self, feature: FeatureId, record_history: bool) {
        self.open_feature_tab(feature);

        if self.active_feature == feature {
            self.scroll_tab_into_view(feature);
            return;
        }

        self.active_feature = feature;
        if record_history {
            self.push_navigation_history(feature);
        }
        self.scroll_tab_into_view(feature);
    }

    fn push_navigation_history(&mut self, feature: FeatureId) {
        if self.navigation_history.get(self.navigation_history_index) == Some(&feature) {
            return;
        }

        self.navigation_history
            .truncate(self.navigation_history_index + 1);
        self.navigation_history.push(feature);
        self.navigation_history_index = self.navigation_history.len().saturating_sub(1);
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let navigation_groups = feature_catalog()
            .iter()
            .map(|item| item.section())
            .fold(Vec::new(), |mut sections, section| {
                if !sections.contains(&section) {
                    sections.push(section);
                }
                sections
            })
            .into_iter()
            .map(|section| self.render_nav_group(section, cx))
            .collect::<Vec<_>>();
        let theme = cx.theme();

        Sidebar::new("console-sidebar")
            .w_full()
            .collapsible(SidebarCollapsible::None)
            .header(
                div()
                    .relative()
                    .w_full()
                    .pb_3()
                    .child(
                        SidebarHeader::new().child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .min_w_0()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size_8()
                                        .flex_shrink_0()
                                        .rounded_md()
                                        .bg(theme.tokens.sidebar_primary)
                                        .text_color(theme.sidebar_primary_foreground)
                                        .font_bold()
                                        .child("X"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_bold()
                                                .text_color(theme.sidebar_foreground)
                                                .child("Xuwe Console"),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child("桌面应用模板"),
                                        ),
                                ),
                        ),
                    )
                    .child(full_bleed_sidebar_separator(
                        SidebarSeparatorEdge::Bottom,
                        theme,
                    )),
            )
            .children(navigation_groups)
            .footer(self.render_account_footer(cx))
            .into_any_element()
    }

    fn render_account_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = auth::snapshot(cx);
        let menu_items = if snapshot.authenticated {
            account_actions::menu_actions(snapshot.display_name.to_string())
        } else {
            account_actions::signed_out_menu_actions()
        };
        let action_context = cx.focus_handle();
        let theme = cx.theme();
        let display_name = snapshot.display_name.clone();
        let avatar = if let Some(avatar_url) = snapshot.avatar_url.clone() {
            Avatar::new()
                .name(display_name.clone())
                .src(avatar_url)
                .small()
        } else {
            Avatar::new().name(display_name.clone()).small()
        };
        let status = snapshot.status.clone();

        div()
            .relative()
            .w_full()
            .pt_3()
            .child(
                SidebarFooter::new()
                    .justify_between()
                    .child(
                        h_flex().gap_2().child(avatar).child(
                            div().flex().flex_col().min_w_0().child(display_name).child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(status),
                            ),
                        ),
                    )
                    .child(Icon::new(IconName::ChevronsUpDown).size_4())
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                        menu_items.iter().cloned().fold(
                            menu.action_context(action_context.clone()).min_w(220.),
                            |menu, item| {
                                let menu_item =
                                    gpui_component::menu::PopupMenuItem::new(item.label())
                                        .icon(account_icon(item.kind()));
                                let menu_item = match item.kind() {
                                    AccountActionKind::SignIn => menu_item.on_click(|_, _, cx| {
                                        eprintln!("Console OIDC: 登录菜单已点击");
                                        if let Err(error) = auth::start_login(cx) {
                                            eprintln!("Console OIDC: 无法开始登录: {error}");
                                            auth::complete_login(Err(error), cx);
                                        }
                                    }),
                                    AccountActionKind::SignOut => {
                                        menu_item.on_click(|_, _, cx| auth::sign_out(cx))
                                    }
                                    AccountActionKind::Settings => {
                                        menu_item.on_click(|_, window, cx| {
                                            window.dispatch_action(Box::new(OpenSettings), cx);
                                        })
                                    }
                                    AccountActionKind::Profile => {
                                        menu_item.action(item.to_action())
                                    }
                                };
                                menu.item(menu_item)
                            },
                        )
                    }),
            )
            .child(full_bleed_sidebar_separator(
                SidebarSeparatorEdge::Top,
                theme,
            ))
    }

    fn open_account_profile(
        &mut self,
        _: &OpenAccountProfile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    fn sign_in_account(&mut self, _: &SignInAccount, _: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = auth::start_login(cx) {
            auth::complete_login(Err(error), cx);
        }
        cx.notify();
    }

    fn sign_out_account(&mut self, _: &SignOutAccount, _: &mut Window, cx: &mut Context<Self>) {
        auth::sign_out(cx);
        cx.notify();
    }

    fn render_nav_group(
        &self,
        section: &'static str,
        cx: &mut Context<Self>,
    ) -> SidebarGroup<SidebarMenu> {
        let items = feature_catalog()
            .iter()
            .copied()
            .filter(|item| item.section() == section)
            .map(|item| self.render_nav_item(item, cx))
            .collect::<Vec<_>>();

        SidebarGroup::new(section).child(SidebarMenu::new().children(items))
    }

    fn render_nav_item(&self, item: FeatureItem, cx: &mut Context<Self>) -> SidebarMenuItem {
        let feature = item.id();
        let children = item
            .children()
            .iter()
            .copied()
            .map(|child| self.render_nav_child(child, cx))
            .collect::<Vec<_>>();
        let has_children = !children.is_empty();
        let active = !has_children && self.active_feature() == feature;

        let menu_item = SidebarMenuItem::new(feature.title())
            .icon(feature_icon(feature))
            .active(active)
            .suffix(move |_, _| div().text_xs().child(nav_badge(feature)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_feature(feature);
                cx.notify();
            }));

        if has_children {
            menu_item
                .default_open(true)
                .click_to_toggle(true)
                .children(children)
        } else {
            menu_item
        }
    }

    fn render_nav_child(&self, item: FeatureChildItem, cx: &mut Context<Self>) -> SidebarMenuItem {
        let feature = item.id();
        let active = self.active_feature() == feature;

        SidebarMenuItem::new(item.title())
            .active(active)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_feature(feature);
                cx.notify();
            }))
    }

    fn render_tab(feature: FeatureId, is_pinned: bool, root_view: WeakEntity<Self>) -> Tab {
        let action_root = root_view.clone();
        let context_root = root_view.clone();
        let action = if is_pinned {
            Toggle::new(format!("pin-tab-{}", nav_badge(feature)))
                .xsmall()
                .checked(true)
                .icon(IconName::StarFill)
                .tooltip("取消置顶")
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    _ = action_root.update(cx, |this, cx| {
                        this.toggle_pin_tab(feature);
                        cx.notify();
                    });
                })
                .into_any_element()
        } else {
            Button::new(format!("close-tab-{}", nav_badge(feature)))
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("关闭标签")
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    _ = action_root.update(cx, |this, cx| {
                        this.close_tab(feature);
                        cx.notify();
                    });
                })
                .into_any_element()
        };

        Tab::new()
            .px_1()
            .prefix(Icon::new(feature_icon(feature)))
            .label(feature.title())
            .suffix(h_flex().gap_1().child(action))
            .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                _ = context_root.update(cx, |this, _| {
                    this.tab_context_feature = Some(feature);
                });
            })
    }

    fn render_title_bar_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let pinned_tabs = self.pinned_tabs().to_vec();
        let regular_tabs = self.regular_tabs();
        let active_pinned_tab_index = self.active_pinned_tab_index();
        let active_regular_tab_index = self.active_regular_tab_index();
        let root_view = cx.entity().downgrade();
        let title_bar_background = cx.theme().tokens.title_bar;
        let can_navigate_back = self.can_navigate_back();
        let can_navigate_forward = self.can_navigate_forward();

        h_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .overflow_hidden()
            .gap_2()
            .items_center()
            .child(
                div()
                    .id("console-open-tabs-zone")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .id("console-open-tabs-strip")
                            .absolute()
                            .left_0()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .h_full()
                            .min_w_0()
                            .overflow_hidden()
                            .items_center()
                            .child(
                                h_flex()
                                    .mx_1()
                                    .flex_shrink_0()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .child(
                                        Button::new("tabs-back")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::ArrowLeft)
                                            .disabled(!can_navigate_back)
                                            .tooltip("后退")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.navigate_back();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("tabs-forward")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::ArrowRight)
                                            .disabled(!can_navigate_forward)
                                            .tooltip("前进")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.navigate_forward();
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .when(!pinned_tabs.is_empty(), |this| {
                                this.child(
                                    div()
                                        .id("console-pinned-tabs-zone")
                                        .flex_none()
                                        .max_w(px(220.0))
                                        .min_w_0()
                                        .h_full()
                                        .overflow_hidden()
                                        .child(
                                            TabBar::new("console-pinned-tabs")
                                                .w_full()
                                                .h_full()
                                                .track_scroll(&self.pinned_tab_scroll_handle)
                                                .menu(pinned_tabs.len() > 2)
                                                .when_some(
                                                    active_pinned_tab_index,
                                                    |this, index| this.selected_index(index),
                                                )
                                                .on_click(cx.listener(
                                                    |this, index: &usize, _, cx| {
                                                        this.select_pinned_tab(*index);
                                                        cx.notify();
                                                    },
                                                ))
                                                .children(pinned_tabs.iter().copied().map(
                                                    |feature| {
                                                        Self::render_tab(
                                                            feature,
                                                            true,
                                                            root_view.clone(),
                                                        )
                                                    },
                                                )),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .id("console-regular-tabs-zone")
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_hidden()
                                    .child(
                                        TabBar::new("console-regular-tabs")
                                            .w_full()
                                            .h_full()
                                            .track_scroll(&self.regular_tab_scroll_handle)
                                            .menu(!regular_tabs.is_empty())
                                            .when_some(active_regular_tab_index, |this, index| {
                                                this.selected_index(index)
                                            })
                                            .on_click(cx.listener(|this, index: &usize, _, cx| {
                                                this.select_regular_tab(*index);
                                                cx.notify();
                                            }))
                                            .children(regular_tabs.iter().copied().map(
                                                |feature| {
                                                    Self::render_tab(
                                                        feature,
                                                        false,
                                                        root_view.clone(),
                                                    )
                                                },
                                            )),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("console-open-tabs-bottom-mask")
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(1.0))
                            .bg(title_bar_background),
                    )
                    .context_menu({
                        let root_view = root_view.clone();
                        move |menu, _, cx| {
                            let Some(root) = root_view.upgrade() else {
                                return menu;
                            };

                            let Some((
                                feature,
                                pinned,
                                can_close_left,
                                can_close_right,
                                can_close_other,
                            )) = ({
                                let root = root.read(cx);
                                let Some(feature) = root.tab_context_feature else {
                                    return menu;
                                };
                                let Some(index) = root.tab_index(feature) else {
                                    return menu;
                                };
                                let can_close_left = root
                                    .opened_tabs
                                    .iter()
                                    .take(index)
                                    .any(|opened| !root.is_tab_pinned(*opened));
                                let can_close_right = root
                                    .opened_tabs
                                    .iter()
                                    .skip(index + 1)
                                    .any(|opened| !root.is_tab_pinned(*opened));
                                let can_close_other = root.opened_tabs.iter().any(|opened| {
                                    *opened != feature && !root.is_tab_pinned(*opened)
                                });

                                Some((
                                    feature,
                                    root.is_tab_pinned(feature),
                                    can_close_left,
                                    can_close_right,
                                    can_close_other,
                                ))
                            })
                            else {
                                return menu;
                            };

                            menu.min_w(220.)
                                .item(PopupMenuItem::new("关闭").icon(IconName::Close).on_click({
                                    let root_view = root_view.clone();
                                    move |_, _, cx| {
                                        _ = root_view.update(cx, |this, cx| {
                                            this.close_tab(feature);
                                            cx.notify();
                                        });
                                    }
                                }))
                                .separator()
                                .item(
                                    PopupMenuItem::new("关闭左侧标签页")
                                        .icon(IconName::ArrowLeft)
                                        .disabled(!can_close_left)
                                        .on_click({
                                            let root_view = root_view.clone();
                                            move |_, _, cx| {
                                                _ = root_view.update(cx, |this, cx| {
                                                    this.close_tabs_to_left(feature);
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("关闭右侧标签页")
                                        .icon(IconName::ArrowRight)
                                        .disabled(!can_close_right)
                                        .on_click({
                                            let root_view = root_view.clone();
                                            move |_, _, cx| {
                                                _ = root_view.update(cx, |this, cx| {
                                                    this.close_tabs_to_right(feature);
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("关闭其他标签页")
                                        .disabled(!can_close_other)
                                        .on_click({
                                            let root_view = root_view.clone();
                                            move |_, _, cx| {
                                                _ = root_view.update(cx, |this, cx| {
                                                    this.close_other_tabs(feature);
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                )
                                .separator()
                                .item(
                                    PopupMenuItem::new(if pinned {
                                        "取消置顶标签页"
                                    } else {
                                        "置顶标签页"
                                    })
                                    .checked(pinned)
                                    .on_click({
                                        let root_view = root_view.clone();
                                        move |_, _, cx| {
                                            _ = root_view.update(cx, |this, cx| {
                                                this.toggle_pin_tab(feature);
                                                cx.notify();
                                            });
                                        }
                                    }),
                                )
                        }
                    }),
            )
            .child(
                div()
                    .id("titlebar-drag-space")
                    .flex_none()
                    .w(px(54.0))
                    .min_w(px(54.0))
                    .h_full(),
            )
            .into_any_element()
    }

    fn render_panel_header(&self, cx: &mut Context<Self>) -> PanelHeader {
        let active_feature = self.active_feature();
        let breadcrumb = feature_breadcrumb_path(active_feature).into_iter().fold(
            Breadcrumb::new(),
            |breadcrumb, (label, target)| {
                let item = match target {
                    Some(target) => {
                        BreadcrumbItem::new(label).on_click(cx.listener(move |this, _, _, cx| {
                            this.select_feature(target);
                            cx.notify();
                        }))
                    }
                    None => BreadcrumbItem::new(label),
                };

                breadcrumb.child(item)
            },
        );
        let pinned = self.is_tab_pinned(active_feature);

        PanelHeader::new(breadcrumb).action(
            Toggle::new("panel-pin-current-tab")
                .small()
                .checked(pinned)
                .icon(if pinned {
                    IconName::StarFill
                } else {
                    IconName::Star
                })
                .tooltip(if pinned {
                    "取消置顶当前标签"
                } else {
                    "置顶当前标签"
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_pin_tab(active_feature);
                    cx.notify();
                })),
        )
    }

    fn render_active_feature(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.active_feature() {
            FeatureId::Home => self.home_feature.render(window, cx),
            FeatureId::Projects | FeatureId::ProjectTemplates | FeatureId::ProjectEnvironments => {
                ProjectsFeature::render(cx)
            }
            FeatureId::Tasks => TasksFeature::render(cx),
            FeatureId::VirtualScroll => self.virtual_scroll_feature.render(window, cx),
            FeatureId::Reports
            | FeatureId::Analytics
            | FeatureId::Releases
            | FeatureId::Secrets
            | FeatureId::Integrations
            | FeatureId::AuditLogs
            | FeatureId::Team
            | FeatureId::Automation
            | FeatureId::Notifications
            | FeatureId::Billing
            | FeatureId::HelpCenter
            | FeatureId::Experiments => self.render_overflow_example(cx),
        }
    }

    fn render_overflow_example(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();

        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.tokens.group_box)
            .child(
                div()
                    .text_lg()
                    .font_bold()
                    .text_color(theme.foreground)
                    .child(self.active_feature().title()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("这是用于演示导航项增多后 Sidebar 中间区域滚动行为的占位页面。"),
            )
            .into_any_element()
    }
}

impl Render for RootView {
    /// 将根视图渲染为 GPUI 元素树。
    ///
    /// 渲染时会把控制台专属的导航、标签栏和当前 feature 页面传入共享工作区布局，
    /// 体现多个业务模块共同构成桌面程序、窗口结构由公共 UI crate 复用的职责边界。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !auth::snapshot(cx).authenticated {
            return LoginFeature::render(window, cx);
        }

        let sidebar = self.render_sidebar(cx);
        let title_bar_content = self.render_title_bar_content(cx);
        let panel_header = self.render_panel_header(cx);
        let content_scrollable = self.active_feature() != FeatureId::VirtualScroll;
        let active_feature = self.render_active_feature(window, cx);

        div()
            .key_context(account_actions::CONTEXT)
            .on_action(cx.listener(Self::sign_in_account))
            .on_action(cx.listener(Self::open_account_profile))
            .on_action(cx.listener(Self::sign_out_account))
            .size_full()
            .child(
                WorkspaceLayout::new(sidebar, title_bar_content, active_feature)
                    .with_panel_header(panel_header)
                    .with_content_scrollable(content_scrollable)
                    .render(window, cx),
            )
            .into_any_element()
    }
}

fn feature_breadcrumb_path(feature: FeatureId) -> Vec<(&'static str, Option<FeatureId>)> {
    let Some(item) = feature_catalog()
        .iter()
        .copied()
        .find(|item| item.contains(feature))
    else {
        return vec![(feature.title(), None)];
    };
    let section_target = feature_catalog()
        .iter()
        .copied()
        .find(|candidate| candidate.section() == item.section())
        .map(FeatureItem::id);
    let mut path = vec![(item.section(), section_target)];

    if item.id() != feature {
        path.push((item.id().title(), Some(item.id())));
    }
    path.push((feature.title(), None));
    path
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarSeparatorEdge {
    Top,
    Bottom,
}

fn full_bleed_sidebar_separator(
    edge: SidebarSeparatorEdge,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    let separator = div()
        .absolute()
        .left(rems(-0.75))
        .right(rems(-0.75))
        .h(px(1.0))
        .bg(theme.sidebar_border);

    match edge {
        SidebarSeparatorEdge::Top => separator.top_0(),
        SidebarSeparatorEdge::Bottom => separator.bottom_0(),
    }
}

fn account_icon(kind: AccountActionKind) -> IconName {
    match kind {
        AccountActionKind::SignIn => IconName::CircleUser,
        AccountActionKind::Profile => IconName::CircleUser,
        AccountActionKind::Settings => IconName::Settings2,
        AccountActionKind::SignOut => IconName::CircleX,
    }
}

fn feature_icon(feature: FeatureId) -> IconName {
    match feature {
        FeatureId::Home => IconName::LayoutDashboard,
        FeatureId::Projects | FeatureId::ProjectTemplates | FeatureId::ProjectEnvironments => {
            IconName::FolderOpen
        }
        FeatureId::Tasks => IconName::SquareTerminal,
        FeatureId::VirtualScroll => IconName::Frame,
        FeatureId::Reports => IconName::ChartPie,
        FeatureId::Analytics => IconName::Inspector,
        FeatureId::Releases => IconName::Globe,
        FeatureId::Secrets => IconName::EyeOff,
        FeatureId::Integrations => IconName::Building2,
        FeatureId::AuditLogs => IconName::BookOpen,
        FeatureId::Team => IconName::User,
        FeatureId::Automation => IconName::Bot,
        FeatureId::Notifications => IconName::Bell,
        FeatureId::Billing => IconName::Building2,
        FeatureId::HelpCenter => IconName::Info,
        FeatureId::Experiments => IconName::Palette,
    }
}

fn nav_badge(feature: FeatureId) -> &'static str {
    match feature {
        FeatureId::Home => "01",
        FeatureId::Projects | FeatureId::ProjectTemplates | FeatureId::ProjectEnvironments => "02",
        FeatureId::Tasks => "03",
        FeatureId::VirtualScroll => "04",
        FeatureId::Reports => "05",
        FeatureId::Analytics => "06",
        FeatureId::Releases => "07",
        FeatureId::Secrets => "08",
        FeatureId::Integrations => "09",
        FeatureId::AuditLogs => "10",
        FeatureId::Team => "11",
        FeatureId::Automation => "12",
        FeatureId::Notifications => "13",
        FeatureId::Billing => "14",
        FeatureId::HelpCenter => "15",
        FeatureId::Experiments => "16",
    }
}
