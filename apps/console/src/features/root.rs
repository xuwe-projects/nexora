//! 控制台应用的根视图。
//!
//! 该模块定义主窗口中最外层的业务视图，运行器会将其嵌入 `gpui_component::Root`。

use crate::features::{
    FeatureChildItem, FeatureId, FeatureItem, feature_catalog, home::HomeFeature,
    projects::ProjectsFeature, settings::SettingsFeature, tasks::TasksFeature,
    virtual_scroll::VirtualScrollFeature,
};
use actions::account::{
    self as account_actions, AccountActionKind, AccountActionSpec, OpenAccountProfile,
    OpenAccountSettings, SignOutAccount,
};
use gpui::{
    Anchor, AnyElement, Context, IntoElement, MouseButton, Render, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem,
    },
    tab::{Tab, TabBar},
};
use ui::layout::SidebarShell;

/// 控制台主窗口的业务根视图。
///
/// 该视图持有当前选中的功能区，并负责把侧边栏、顶部状态栏和各个 feature 页面组合成完整窗口。
pub struct RootView {
    /// 当前在主内容区展示的功能区。
    active_feature: FeatureId,
    /// 顶部标签栏中已经打开过的功能区。
    opened_tabs: Vec<FeatureId>,
    /// 顶部标签栏中被置顶的功能区。
    pinned_tabs: Vec<FeatureId>,
    /// 最近一次右键点击的标签页，用于构建标签页上下文菜单。
    tab_context_feature: Option<FeatureId>,
    home_feature: HomeFeature,
    virtual_scroll_feature: VirtualScrollFeature,
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

    /// 判断指定功能区对应的标签页是否已经置顶。
    ///
    /// 置顶状态只影响标签栏排序和批量关闭行为，不改变 feature 自身的业务状态。
    pub fn is_tab_pinned(&self, feature: FeatureId) -> bool {
        self.pinned_tabs.contains(&feature)
    }

    /// 返回当前选中功能区在顶部标签栏中的索引。
    ///
    /// 当当前功能区已经存在于 `opened_tabs` 时返回对应位置；如果后续调用方直接修改状态导致不一致，
    /// 返回 `None`，渲染层会回退到第一个标签。
    pub fn active_tab_index(&self) -> Option<usize> {
        self.opened_tabs
            .iter()
            .position(|feature| *feature == self.active_feature)
    }

    /// 返回侧边栏底部账户区域展示的用户名。
    ///
    /// 当前模板使用固定样例值模拟登录用户，后续接入真实账号体系时可以把该值替换为用户状态。
    pub fn account_display_name(&self) -> &'static str {
        "Jason Lee"
    }

    /// 返回侧边栏底部账户区域展示的套餐名称。
    ///
    /// 当前模板展示 `Free`，用于演示桌面控制台常见的账户入口结构。
    pub fn account_plan_label(&self) -> &'static str {
        "Free"
    }

    /// 返回侧边栏账户弹出菜单中的动作配置。
    ///
    /// 菜单第一项是当前用户资料入口，后续项提供设置和退出登录能力；渲染层会根据这些配置生成 `PopupMenu`。
    pub fn account_menu_actions(&self) -> Vec<AccountActionSpec> {
        account_actions::menu_actions(self.account_display_name())
    }

    /// 切换当前选中的功能区。
    ///
    /// RootView 只保存导航状态，各个 feature 的业务状态仍应由对应模块自行管理。
    pub fn select_feature(&mut self, feature: FeatureId) {
        self.open_feature_tab(feature);
        self.active_feature = feature;
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
    }

    fn open_feature_tab(&mut self, feature: FeatureId) {
        if !self.opened_tabs.contains(&feature) {
            self.opened_tabs.push(feature);
            self.reorder_tabs_by_pin();
        }
    }

    fn select_opened_tab(&mut self, index: usize) {
        if let Some(feature) = self.opened_tabs.get(index).copied() {
            self.active_feature = feature;
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

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();

        Sidebar::new("console-sidebar")
            .w_full()
            .collapsible(SidebarCollapsible::None)
            .header(
                div()
                    .w_full()
                    .border_b_1()
                    .border_color(theme.sidebar_border)
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
                    ),
            )
            .child(self.render_nav_group("工作台", cx))
            .child(self.render_nav_group("扩展示例", cx))
            .child(self.render_nav_group("系统", cx))
            .footer(self.render_account_footer(cx))
            .into_any_element()
    }

    fn render_account_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let menu_items = self.account_menu_actions();
        let action_context = cx.focus_handle();
        let theme = cx.theme();

        div()
            .w_full()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(
                SidebarFooter::new()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(IconName::CircleUser)
                            .child(self.account_display_name()),
                    )
                    .child(Icon::new(IconName::ChevronsUpDown).size_4())
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                        menu_items.iter().cloned().fold(
                            menu.action_context(action_context.clone()).min_w(220.),
                            |menu, item| {
                                menu.item(
                                    gpui_component::menu::PopupMenuItem::new(item.label())
                                        .icon(account_icon(item.kind()))
                                        .action(item.to_action()),
                                )
                            },
                        )
                    }),
            )
    }

    fn open_account_profile(
        &mut self,
        _: &OpenAccountProfile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    fn open_account_settings(
        &mut self,
        _: &OpenAccountSettings,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_feature(FeatureId::Settings);
        cx.notify();
    }

    fn sign_out_account(&mut self, _: &SignOutAccount, _: &mut Window, cx: &mut Context<Self>) {
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

    fn render_top_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let active_tab_index = self.active_tab_index().unwrap_or_default();
        let opened_tabs = self.opened_tabs().to_vec();
        let pinned_tabs = self.pinned_tabs().to_vec();
        let root_view = cx.entity().downgrade();
        let title_bar_background = cx.theme().tokens.title_bar;
        let tab_separator_color = cx.theme().border;

        TitleBar::new()
            .border_b(px(0.0))
            .child(
                h_flex()
                    .w_full()
                    .h_full()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .px_3()
                    .child(
                        div()
                            .id("console-open-tabs-zone")
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(
                                TabBar::new("console-open-tabs")
                                    .w_full()
                                    .h_full()
                                    .menu(true)
                                    .selected_index(active_tab_index)
                                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                                        this.select_opened_tab(*index);
                                        cx.notify();
                                    }))
                                    .prefix(
                                        h_flex()
                                            .mx_1()
                                            .child(
                                                Button::new("tabs-back")
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(IconName::ArrowLeft),
                                            )
                                            .child(
                                                Button::new("tabs-forward")
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(IconName::ArrowRight),
                                            ),
                                    )
                                    .children(opened_tabs.iter().copied().enumerate().map(
                                        |(tab_index, feature)| {
                                            let is_pinned = pinned_tabs.contains(&feature);
                                            let has_separator = tab_index + 1 < opened_tabs.len();
                                            let action_root = root_view.clone();
                                            let context_root = root_view.clone();
                                            let action_icon = if is_pinned {
                                                IconName::Check
                                            } else {
                                                IconName::Close
                                            };
                                            let action_tooltip = if is_pinned {
                                                "取消置顶"
                                            } else {
                                                "关闭标签"
                                            };

                                            Tab::new()
                                                .prefix(Icon::new(feature_icon(feature)))
                                                .label(feature.title())
                                                .suffix(
                                                    h_flex()
                                                        .gap_1()
                                                        .child(
                                                            Button::new(format!(
                                                                "close-tab-{}",
                                                                nav_badge(feature)
                                                            ))
                                                            .ghost()
                                                            .xsmall()
                                                            .icon(action_icon)
                                                            .tooltip(action_tooltip)
                                                            .on_click(move |_, _, cx| {
                                                                cx.stop_propagation();
                                                                _ = action_root.update(
                                                                    cx,
                                                                    |this, cx| {
                                                                        if is_pinned {
                                                                            this.toggle_pin_tab(
                                                                                feature,
                                                                            );
                                                                        } else {
                                                                            this.close_tab(feature);
                                                                        }
                                                                        cx.notify();
                                                                    },
                                                                );
                                                            }),
                                                        )
                                                        .when(has_separator, |this| {
                                                            this.child(
                                                                div()
                                                                    .w(px(1.0))
                                                                    .h(px(18.0))
                                                                    .bg(tab_separator_color),
                                                            )
                                                        }),
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Right,
                                                    move |_, _, cx| {
                                                        _ = context_root.update(cx, |this, _| {
                                                            this.tab_context_feature =
                                                                Some(feature);
                                                        });
                                                    },
                                                )
                                        },
                                    ))
                                    .suffix(
                                        h_flex()
                                            .mx_1()
                                            .child(
                                                Button::new("tabs-inbox")
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(IconName::Inbox),
                                            )
                                            .child(
                                                Button::new("tabs-more")
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(IconName::Ellipsis),
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
                                        let can_close_other =
                                            root.opened_tabs.iter().any(|opened| {
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
                                        .item(
                                            PopupMenuItem::new("关闭")
                                                .icon(IconName::Close)
                                                .on_click({
                                                    let root_view = root_view.clone();
                                                    move |_, _, cx| {
                                                        _ = root_view.update(cx, |this, cx| {
                                                            this.close_tab(feature);
                                                            cx.notify();
                                                        });
                                                    }
                                                }),
                                        )
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
                            .w(px(80.0))
                            .h_full(),
                    ),
            )
            .into_any_element()
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
            FeatureId::Settings => SettingsFeature::render(cx),
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
            .bg(theme.tokens.background)
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
    /// 渲染结果由应用壳和当前选中的 feature 页面组成，体现多个功能模块共同构成桌面程序的结构。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = self.render_sidebar(cx);
        let top_bar = self.render_top_bar(cx);
        let content_scrollable = self.active_feature() != FeatureId::VirtualScroll;
        let active_feature = self.render_active_feature(window, cx);

        div()
            .key_context(account_actions::CONTEXT)
            .on_action(cx.listener(Self::open_account_profile))
            .on_action(cx.listener(Self::open_account_settings))
            .on_action(cx.listener(Self::sign_out_account))
            .size_full()
            .child(
                SidebarShell::new(sidebar, top_bar, active_feature)
                    .with_content_scrollable(content_scrollable)
                    .render(cx),
            )
    }
}

fn account_icon(kind: AccountActionKind) -> IconName {
    match kind {
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
        FeatureId::Settings => IconName::Settings2,
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
        FeatureId::Settings => "17",
    }
}
