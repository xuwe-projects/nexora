use std::time::Duration;

use gpui::{
    AnyView, App, Context, Div, Entity, IntoElement, Render, SharedString, Window, div, prelude::*,
    px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    form::{field, v_form},
    h_flex,
    input::{Input, InputState, NumberInput},
    v_flex,
};
use nexora::{
    Application as _, ApplicationOptions, Feature, FeatureElement,
    desktop::{
        CrudPanel, CrudPanelToolbar, FormDialog, FormDialogState, FormFieldEvent, FormFieldState,
        SidebarRegion, TableCell, TableHeaderCell,
    },
};

#[derive(Default, Feature)]
#[nexora(
    title = "首页",
    path = "/",
    section = "Examples",
    icon = "house",
    order = 0
)]
struct HomeFeature {
    search_input: Option<Entity<InputState>>,
    form_state: Option<Entity<FormDialogState>>,
    dialog_layer: Option<Entity<ShowcaseDialogLayer>>,
}

impl FeatureElement for HomeFeature {
    fn initialize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let username_input = cx.new(|cx| InputState::new(window, cx).placeholder("例如 alice"));
        let age_input = cx.new(|cx| InputState::new(window, cx).placeholder("例如 28"));
        let username_check_executor = cx.background_executor().clone();
        let username_field = FormFieldState::input("username", &username_input)
            .required("请输入用户名")
            .pattern(r"^[a-z][a-z0-9_]{2,15}$", "用户名格式不正确")
            .on_change(move |event: FormFieldEvent<SharedString>| {
                let executor = username_check_executor.clone();
                let value = event.value().clone();
                let target = event.current_target().clone();
                async move {
                    executor.timer(Duration::from_millis(240)).await;
                    if value.as_ref() == "taken" {
                        target.set_error("用户名已经存在");
                    } else {
                        target.clear_error();
                    }
                }
            })
            .build(window, cx);
        let age_field = FormFieldState::number_input::<i64>("age", &age_input)
            .required("请输入年龄")
            .parse_error("请输入有效的整数")
            .build(window, cx);
        let accepted_field = FormFieldState::checkbox("accepted", false)
            .required("请先确认校验规则")
            .build(window, cx);
        let form_state = cx.new(|cx| {
            FormDialogState::new(cx)
                .field(&username_field)
                .field(&age_field)
                .field(&accepted_field)
        });
        self.search_input =
            Some(cx.new(|cx| InputState::new(window, cx).placeholder("筛选组件名称")));
        self.form_state = Some(form_state.clone());
        self.dialog_layer = Some(cx.new(|_| ShowcaseDialogLayer {
            state: form_state,
            username_input,
            age_input,
            username_field,
            age_field,
            accepted_field,
            success: None,
        }));
    }

    fn panel_overlay(&self) -> Option<AnyView> {
        self.dialog_layer.clone().map(Into::into)
    }

    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let search_input = self
            .search_input
            .as_ref()
            .expect("desktop_basic 搜索输入必须先完成 initialize");

        let search_filter = v_form().child(
            field()
                .label("组件筛选")
                .description("静态示例控件，不会发起查询。")
                .w(px(280.0))
                .child(
                    Input::new(search_input)
                        .with_size(component_size)
                        .cleanable(true),
                ),
        );

        let open_form_action = Button::new("desktop-basic-open-form-dialog")
            .primary()
            .with_size(component_size)
            .icon(IconName::Plus)
            .label("打开表单")
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_form_dialog(window, cx);
            }));

        CrudPanel::new("Nexora 组件 Showcase", showcase_content(cx))
            .description("通过 nexora::desktop facade 展示轻量、可组合的桌面组件。")
            .toolbar(
                CrudPanelToolbar::new()
                    .filter(search_filter)
                    .action(open_form_action),
            )
            .refresh("desktop-basic-refresh", false, false, |_, _, _| {})
            .with_size(component_size)
    }
}

impl HomeFeature {
    fn open_form_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form_state = self
            .form_state
            .as_ref()
            .expect("desktop_basic 表单状态必须先完成 initialize");
        form_state.update(cx, |state, cx| {
            state.reset_fields(cx);
            state.open(window, cx);
        });
    }
}

struct ShowcaseDialogLayer {
    state: Entity<FormDialogState>,
    username_input: Entity<InputState>,
    age_input: Entity<InputState>,
    username_field: Entity<FormFieldState<SharedString>>,
    age_field: Entity<FormFieldState<i64>>,
    accepted_field: Entity<FormFieldState<bool>>,
    success: Option<SharedString>,
}

impl Render for ShowcaseDialogLayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let state = self.state.clone();
        let username_error = self.username_field.read(cx).visible_error().cloned();
        let age_error = self.age_field.read(cx).visible_error().cloned();
        let accepted_error = self.accepted_field.read(cx).visible_error().cloned();
        let accepted = self
            .accepted_field
            .read(cx)
            .value()
            .copied()
            .unwrap_or(false);
        let accepted_field = self.accepted_field.clone();

        let mut dialog = FormDialog::new("desktop-basic-form-dialog", self.state.clone())
            .title("FormDialog 示例")
            .description("失焦会显示字段错误；提交会等待已经运行的用户名检查。")
            .child(
                field()
                    .label("用户名")
                    .description("使用字母开头，可包含小写字母、数字和下划线。")
                    .required(true)
                    .child(field_control(
                        Input::new(&self.username_input).with_size(component_size),
                        username_error,
                        cx,
                    )),
            )
            .child(
                field()
                    .label("年龄")
                    .description("字段值在业务侧直接转换为 i64。")
                    .required(true)
                    .child(field_control(
                        NumberInput::new(&self.age_input).with_size(component_size),
                        age_error,
                        cx,
                    )),
            )
            .child(
                field().label("确认").required(true).child(field_control(
                    Checkbox::new("desktop-basic-accepted")
                        .with_size(component_size)
                        .label("确认阅读校验规则")
                        .checked(accepted)
                        .on_click(move |checked, window, cx| {
                            accepted_field.update(cx, |field, cx| {
                                field.update_checkbox(*checked, window, cx);
                            });
                        }),
                    accepted_error,
                    cx,
                )),
            )
            .submit_label("提交")
            .with_size(component_size);

        if let Some(success) = self.success.clone() {
            dialog = dialog.section(Alert::success("desktop-basic-form-success", success));
        }

        dialog.on_submit(cx.listener(move |this, _, _window, cx| {
            let username = this
                .username_field
                .read(cx)
                .value()
                .cloned()
                .unwrap_or_default();
            let age = this.age_field.read(cx).value().copied().unwrap_or_default();
            this.success = Some(format!("已通过本地校验：{username}，年龄 {age}").into());
            state.update(cx, |state, cx| state.mark_saved(cx));
            cx.notify();
        }))
    }
}

fn field_control(
    control: impl IntoElement,
    error: Option<SharedString>,
    cx: &mut App,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(control)
        .when_some(error, |this, error| {
            this.child(div().text_xs().text_color(cx.theme().danger).child(error))
        })
}

fn showcase_content(cx: &mut Context<HomeFeature>) -> impl IntoElement {
    v_flex()
        .w_full()
        .min_h_0()
        .gap_4()
        .child(
            showcase_grid()
                .child(showcase_card(
                    "CrudPanel",
                    "三段式资源页面骨架：摘要、工具栏和主体内容。",
                    "当前页面本身就是 CrudPanel 示例。",
                    cx,
                ))
                .child(showcase_card(
                    "Form 与 Field",
                    "官方表单负责标签、说明、必填语义与字段布局。",
                    "上方工具栏里的筛选输入由官方 Field 组合。",
                    cx,
                ))
                .child(showcase_card(
                    "SidebarRegion",
                    "Sidebar 插槽里的稳定交互区域，不会隐式注入 hover 或点击语义。",
                    "下面的品牌与上下文区域共用这个组件。",
                    cx,
                )),
        )
        .child(sidebar_region_example(cx))
        .child(table_cell_example(cx))
}

fn showcase_grid() -> Div {
    div().grid().grid_cols(3).gap_3().w_full().max_w_full()
}

fn showcase_card(
    title: &'static str,
    description: &'static str,
    note: &'static str,
    cx: &mut Context<HomeFeature>,
) -> impl IntoElement {
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .p_4()
        .child(div().text_sm().font_semibold().child(title))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(description),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(note),
        )
}

fn sidebar_region_example(cx: &mut Context<HomeFeature>) -> impl IntoElement {
    let theme = cx.theme();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .p_4()
        .child(div().text_sm().font_semibold().child("SidebarRegion 示例"))
        .child(
            h_flex()
                .gap_3()
                .child(
                    SidebarRegion::new("desktop-basic-brand-region")
                        .gap_3()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .p_3()
                        .child(div().size_6().rounded_full().bg(theme.primary))
                        .child(
                            v_flex()
                                .min_w_0()
                                .gap_0p5()
                                .child(div().text_sm().font_semibold().child("Nexora"))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child("品牌区域"),
                                ),
                        ),
                )
                .child(
                    SidebarRegion::new("desktop-basic-context-region")
                        .gap_3()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .p_3()
                        .child(div().text_sm().font_semibold().child("当前工作区")),
                ),
        )
}

fn table_cell_example(cx: &mut Context<HomeFeature>) -> impl IntoElement {
    let theme = cx.theme();

    div()
        .flex()
        .flex_col()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .overflow_hidden()
        .child(
            div()
                .grid()
                .grid_cols(3)
                .h(px(40.0))
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.tokens.group_box)
                .child(TableHeaderCell::new("组件").left())
                .child(TableHeaderCell::new("用途").left())
                .child(TableHeaderCell::new("状态").right()),
        )
        .child(table_row(
            "TableCell",
            "正文单元格对齐",
            TableCell::new("Ready").right(),
            cx,
        ))
        .child(table_row(
            "FormDialog",
            "Panel 内表单遮罩",
            TableCell::new("Interactive").right(),
            cx,
        ))
}

fn table_row(
    name: &'static str,
    usage: &'static str,
    status: TableCell,
    cx: &mut Context<HomeFeature>,
) -> impl IntoElement {
    div()
        .grid()
        .grid_cols(3)
        .h(px(44.0))
        .border_b_1()
        .border_color(cx.theme().border)
        .child(TableCell::new(name).left())
        .child(TableCell::new(usage).left())
        .child(status)
}

struct DesktopBasicApplication;

impl nexora::Application for DesktopBasicApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("Nexora Desktop Basic")
            .sidebar_subtitle("Cargo example")
            .sidebar_search(true)
            .initial_path("/")
            .window_size(900.0, 640.0)
    }
}

fn main() -> Result<(), nexora::ApplicationError> {
    DesktopBasicApplication.run()
}
