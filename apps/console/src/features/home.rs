//! Console 桌面应用首页功能模块。
//!
//! 该模块展示桌面程序模板的概览页，用于说明一个 feature 如何独立组织自己的页面内容。

use std::cmp::Ordering;

use gpui::{
    AnyElement, App, Context, Div, Entity, IntoElement, SharedString, Stateful, TextAlign, Window,
    div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, StyleSized as _, StyledExt as _, Theme,
    button::{Button, DropdownButton},
    group_box::{GroupBox, GroupBoxVariants as _},
    menu::{PopupMenu, PopupMenuItem},
    stepper::{Stepper, StepperItem},
    table::{Column, ColumnFixed, ColumnGroup, ColumnSort, DataTable, TableDelegate, TableState},
    tag::Tag,
};

/// 首页功能视图。
///
/// 该类型持有首页自己的组件状态，例如虚拟表单使用的 `DataTable` 状态。
/// `RootView` 只负责组合它，首页内部的表格交互状态由该类型自行维护。
#[derive(Default, nexora::Feature)]
#[nexora(
    title = "首页",
    path = "/",
    section = "工作台",
    icon = "layout-dashboard",
    order = 0
)]
pub struct HomeFeature {
    table: Option<Entity<TableState<VirtualFormTableDelegate>>>,
}

impl nexora::FeatureElement for HomeFeature {
    fn initialize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.table.get_or_insert_with(|| {
            cx.new(|cx| {
                TableState::new(VirtualFormTableDelegate::new(), window, cx)
                    .cell_selectable(true)
                    .row_header(false)
                    .col_movable(true)
                    .col_resizable(true)
                    .sortable(true)
            })
        });
    }

    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let table = self
            .table
            .as_ref()
            .expect("首页表格状态必须在 RootView 初始化阶段创建");
        let component_size = theme::component_size(cx);
        let theme = cx.theme();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_5()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.tokens.group_box)
                    .child(
                        div()
                            .text_xs()
                            .font_bold()
                            .text_color(theme.primary)
                            .child("DESKTOP TEMPLATE"),
                    )
                    .child(
                        div()
                            .text_xl()
                            .font_bold()
                            .text_color(theme.foreground)
                            .child("一个由多个 feature 组合出来的桌面控制台"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("这里展示应用壳、导航、功能页和状态区如何协作。真实项目可以把这些静态区块替换成业务 Entity。"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(metric_card("活跃项目", "3", "本地 workspace 已接入", theme))
                    .child(metric_card("待处理任务", "5", "构建与发布队列", theme))
                    .child(metric_card("运行模式", "Local", "当前使用本地打包配置", theme)),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        panel("推荐下一步")
                            .child(next_steps_stepper(theme))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("这些步骤展示模板从静态样例过渡到真实应用的自然顺序。"),
                            ),
                    )
                    .child(
                        panel("当前模板能力")
                            .child(capability("GPUI 运行器", "统一初始化窗口、组件库和应用配置", theme))
                            .child(capability("Feature 运行时", "注册表自动创建 Entity，feature 只实现渲染与生命周期", theme))
                            .child(capability("macOS 打包", "CLI 已支持 .app、DMG、签名、公证和校验文件", theme)),
                    ),
            )
            .child(virtual_form_panel(table, component_size, theme))
            .into_any_element()
    }
}

/// 首页虚拟表单数据表中的一条记录。
///
/// 该类型用于演示后台管理控制台常见的“申请单 / 审批单”列表结构，后续可以替换为真实后端返回的数据模型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualFormRow {
    id: &'static str,
    owner: &'static str,
    status: &'static str,
    priority: &'static str,
    amount: &'static str,
}

impl VirtualFormRow {
    const fn new(
        id: &'static str,
        owner: &'static str,
        status: &'static str,
        priority: &'static str,
        amount: &'static str,
    ) -> Self {
        Self {
            id,
            owner,
            status,
            priority,
            amount,
        }
    }

    /// 返回这条虚拟表单记录的业务编号。
    ///
    /// 编号用于模拟真实系统中的申请单或审批单主键，表格的第一列会固定展示该值。
    pub fn id(self) -> &'static str {
        self.id
    }

    /// 返回这条虚拟表单记录的负责人姓名。
    ///
    /// 负责人用于演示后台控制台中常见的归属人字段，也会在右键菜单中作为上下文文案使用。
    pub fn owner(self) -> &'static str {
        self.owner
    }

    /// 返回这条虚拟表单记录的处理状态。
    ///
    /// 状态会被渲染成不同颜色的 `Tag`，用于展示数据表中状态字段的视觉表达方式。
    pub fn status(self) -> &'static str {
        self.status
    }

    /// 返回这条虚拟表单记录的优先级。
    ///
    /// 优先级用于演示可排序的枚举类字段，真实项目可以替换为更严格的业务枚举。
    pub fn priority(self) -> &'static str {
        self.priority
    }

    /// 返回这条虚拟表单记录关联的金额。
    ///
    /// 金额用于演示右对齐数值列和表格中财务类字段的基础排版。
    pub fn amount(self) -> &'static str {
        self.amount
    }
}

/// 返回首页虚拟表单表格使用的静态样例数据。
///
/// 该函数为首页渲染和集成测试提供稳定数据源，避免示例界面依赖随机数据导致测试不稳定。
pub fn virtual_form_rows() -> &'static [VirtualFormRow] {
    static ROWS: [VirtualFormRow; 10] = [
        VirtualFormRow::new("REQ-2401", "Jason Lee", "待审核", "高", "$12,480"),
        VirtualFormRow::new("REQ-2402", "Mia Chen", "处理中", "中", "$8,900"),
        VirtualFormRow::new("REQ-2403", "Noah Wang", "已通过", "低", "$3,260"),
        VirtualFormRow::new("REQ-2404", "Ava Lin", "待审核", "高", "$21,700"),
        VirtualFormRow::new("REQ-2405", "Ethan Zhou", "已驳回", "中", "$6,450"),
        VirtualFormRow::new("REQ-2406", "Sophia Wu", "处理中", "高", "$15,320"),
        VirtualFormRow::new("REQ-2407", "Liam Xu", "已通过", "低", "$2,180"),
        VirtualFormRow::new("REQ-2408", "Olivia Qian", "待审核", "中", "$9,640"),
        VirtualFormRow::new("REQ-2409", "Lucas Sun", "处理中", "低", "$4,510"),
        VirtualFormRow::new("REQ-2410", "Emma Zhao", "已通过", "高", "$18,260"),
    ];

    &ROWS
}

/// 返回首页虚拟表单下拉筛选器展示的视图模式。
///
/// 当前示例只展示菜单交互，不保存筛选状态；真实项目可以把这些模式接入 feature 自己的 Entity 状态。
pub fn virtual_form_view_modes() -> [&'static str; 3] {
    ["全部记录", "只看待审核", "只看高优先级"]
}

fn metric_card(
    label: &'static str,
    value: &'static str,
    detail: &'static str,
    theme: &Theme,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .flex_1()
        .min_w_0()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.tokens.group_box)
        .child(
            div()
                .text_xs()
                .font_bold()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(
            div()
                .flex()
                .items_end()
                .gap_2()
                .child(
                    div()
                        .text_xl()
                        .font_bold()
                        .text_color(theme.foreground)
                        .child(value),
                )
                .child(Tag::success().small().outline().child("ready").mb_1()),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(detail),
        )
        .into_any_element()
}

fn panel(title: &'static str) -> GroupBox {
    GroupBox::new()
        .fill()
        .title(title)
        .gap_3()
        .flex_1()
        .min_w_0()
}

fn virtual_form_panel(
    table: &Entity<TableState<VirtualFormTableDelegate>>,
    component_size: gpui_component::Size,
    theme: &Theme,
) -> AnyElement {
    panel("虚拟表单")
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_bold()
                                .text_color(theme.foreground)
                                .child("审批请求"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child("使用 DataTable 展示固定列、分组表头、排序和右键菜单。"),
                        ),
                )
                .child(virtual_form_dropdown()),
        )
        .child(
            div().h(px(320.)).w_full().child(
                DataTable::new(table)
                    .with_size(component_size)
                    .stripe(true)
                    .scrollbar_visible(true, true),
            ),
        )
        .into_any_element()
}

fn virtual_form_dropdown() -> AnyElement {
    DropdownButton::new("home-virtual-form-view-mode")
        .button(
            Button::new("home-virtual-form-view-mode-button")
                .outline()
                .small()
                .icon(IconName::Search)
                .label("全部记录"),
        )
        .dropdown_menu(|menu, _, _| {
            virtual_form_view_modes().into_iter().enumerate().fold(
                menu.min_w(180.),
                |menu, (index, mode)| {
                    menu.item(
                        PopupMenuItem::new(mode)
                            .icon(if index == 0 {
                                IconName::CircleCheck
                            } else {
                                IconName::Search
                            })
                            .checked(index == 0)
                            .disabled(true),
                    )
                },
            )
        })
        .into_any_element()
}

/// 返回首页推荐下一步操作。
///
/// 这些步骤用于模板首页和集成测试共同确认示例应用的引导顺序。
pub fn next_steps() -> [&'static str; 3] {
    [
        "把首页替换成真实工作台数据",
        "为项目、任务、设置补充独立 Entity",
        "把常用命令接入 actions 和快捷键",
    ]
}

fn next_steps_stepper(theme: &Theme) -> AnyElement {
    Stepper::new("home-next-steps")
        .vertical()
        .selected_index(0)
        .items_center()
        .items(next_steps().into_iter().enumerate().map(|(index, step)| {
            StepperItem::new()
                .pb_4()
                .icon(match index {
                    0 => IconName::LayoutDashboard,
                    1 => IconName::Frame,
                    _ => IconName::SquareTerminal,
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_bold()
                                .text_color(theme.foreground)
                                .child(format!("Step {}", index + 1)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(step),
                        ),
                )
        }))
        .into_any_element()
}

fn capability(name: &'static str, detail: &'static str, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .border_l_2()
        .border_color(theme.primary)
        .pl_3()
        .child(
            div()
                .text_sm()
                .font_bold()
                .text_color(theme.foreground)
                .child(name),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(detail),
        )
        .into_any_element()
}

#[derive(Debug, Clone)]
struct VirtualFormTableDelegate {
    rows: Vec<VirtualFormRow>,
    columns: Vec<Column>,
}

impl VirtualFormTableDelegate {
    fn new() -> Self {
        Self {
            rows: virtual_form_rows().to_vec(),
            columns: vec![
                Column::new("id", "编号")
                    .width(112.)
                    .fixed(ColumnFixed::Left)
                    .min_width(96.)
                    .max_width(150.)
                    .sortable(),
                Column::new("owner", "负责人")
                    .width(150.)
                    .fixed(ColumnFixed::Left)
                    .min_width(120.)
                    .max_width(220.)
                    .sortable(),
                Column::new("status", "状态")
                    .width(120.)
                    .min_width(104.)
                    .sortable()
                    .text_center(),
                Column::new("priority", "优先级")
                    .width(110.)
                    .min_width(96.)
                    .sortable()
                    .text_center(),
                Column::new("amount", "金额")
                    .width(130.)
                    .min_width(112.)
                    .sortable()
                    .text_right(),
            ],
        }
    }

    fn row_text(&self, row_ix: usize, key: &str) -> SharedString {
        let Some(row) = self.rows.get(row_ix) else {
            return SharedString::new("");
        };

        match key {
            "id" => row.id(),
            "owner" => row.owner(),
            "status" => row.status(),
            "priority" => row.priority(),
            "amount" => row.amount(),
            _ => "",
        }
        .into()
    }

    fn render_text_cell(&self, row_ix: usize, col_ix: usize, cx: &App) -> AnyElement {
        let Some(col) = self.columns.get(col_ix) else {
            return "--".into_any_element();
        };
        let text = self.row_text(row_ix, col.key.as_ref());
        let component_size = theme::component_size(cx);
        let theme = cx.theme();

        div()
            .h_full()
            .flex()
            .items_center()
            .table_cell_size(component_size)
            .text_color(theme.muted_foreground)
            .when(col.align == TextAlign::Center, |this| this.justify_center())
            .when(col.align == TextAlign::Right, |this| {
                this.justify_end().font_medium()
            })
            .when(col.key.as_ref() == "id", |this| {
                this.font_medium().text_color(theme.foreground)
            })
            .when(col.key.as_ref() == "owner", |this| {
                this.text_color(theme.foreground)
            })
            .child(text)
            .into_any_element()
    }

    fn render_status(row: VirtualFormRow) -> AnyElement {
        match row.status() {
            "待审核" => Tag::warning().small().outline().child(row.status()),
            "处理中" => Tag::info().small().outline().child(row.status()),
            "已通过" => Tag::success().small().outline().child(row.status()),
            "已驳回" => Tag::danger().small().outline().child(row.status()),
            _ => Tag::secondary().small().outline().child(row.status()),
        }
        .into_any_element()
    }

    fn render_priority(row: VirtualFormRow) -> AnyElement {
        match row.priority() {
            "高" => Tag::danger().small().outline().child(row.priority()),
            "中" => Tag::warning().small().outline().child(row.priority()),
            _ => Tag::secondary().small().outline().child(row.priority()),
        }
        .into_any_element()
    }
}

impl TableDelegate for VirtualFormTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn group_headers(&self, _: &App) -> Option<Vec<Vec<ColumnGroup>>> {
        Some(vec![vec![
            ColumnGroup::new("请求信息", 2),
            ColumnGroup::new("处理状态", 2),
            ColumnGroup::new("金额", 1),
        ]])
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let col = self.column(col_ix, cx);

        div()
            .size_full()
            .font_bold()
            .text_color(cx.theme().table_head_foreground)
            .child(col.name.clone())
            .when(col.align == TextAlign::Center, |this| {
                this.flex().items_center().justify_center()
            })
            .when(col.align == TextAlign::Right, |this| {
                this.flex().items_center().justify_end()
            })
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let row_id = self.rows.get(row_ix).map_or("missing", |row| row.id());
        div().id(row_id)
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let Some(row) = self.rows.get(row_ix).copied() else {
            return menu;
        };

        menu.item(
            PopupMenuItem::new(format!("打开 {}", row.id()))
                .icon(IconName::PanelLeftOpen)
                .disabled(true),
        )
        .item(
            PopupMenuItem::new(format!("查看 {}", row.owner()))
                .icon(IconName::CircleUser)
                .disabled(true),
        )
        .separator()
        .item(
            PopupMenuItem::new("复制记录编号")
                .icon(IconName::Copy)
                .disabled(true),
        )
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(row) = self.rows.get(row_ix).copied() else {
            return "--".into_any_element();
        };
        let Some(col) = self.columns.get(col_ix) else {
            return "--".into_any_element();
        };

        match col.key.as_ref() {
            "status" => div()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child(Self::render_status(row))
                .into_any_element(),
            "priority" => div()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child(Self::render_priority(row))
                .into_any_element(),
            _ => self.render_text_cell(row_ix, col_ix, cx),
        }
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        let Some(col) = self.columns.get(col_ix) else {
            return;
        };

        match col.key.as_ref() {
            "id" => self
                .rows
                .sort_by(|left, right| sort_order(sort, left.id().cmp(right.id()))),
            "owner" => self
                .rows
                .sort_by(|left, right| sort_order(sort, left.owner().cmp(right.owner()))),
            "status" => self
                .rows
                .sort_by(|left, right| sort_order(sort, left.status().cmp(right.status()))),
            "priority" => self.rows.sort_by(|left, right| {
                sort_order(sort, priority_rank(*left).cmp(&priority_rank(*right)))
            }),
            "amount" => self.rows.sort_by(|left, right| {
                sort_order(sort, amount_cents(*left).cmp(&amount_cents(*right)))
            }),
            _ => {}
        }
    }
}

fn sort_order(sort: ColumnSort, ordering: Ordering) -> Ordering {
    match sort {
        ColumnSort::Descending => ordering.reverse(),
        _ => ordering,
    }
}

fn priority_rank(row: VirtualFormRow) -> u8 {
    match row.priority() {
        "高" => 0,
        "中" => 1,
        _ => 2,
    }
}

fn amount_cents(row: VirtualFormRow) -> u32 {
    row.amount()
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or_default()
}
