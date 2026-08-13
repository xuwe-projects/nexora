use std::time::Duration;

use gpui::{
    AnyView, App, Context, Entity, IntoElement, Render, SharedString, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    form::field,
    input::{Input, InputState, NumberInput},
    v_flex,
};
use nexora::{
    Application as _, ApplicationOptions, Feature, FeatureElement,
    desktop::{
        CrudListState, CrudPage, CrudPanel, FormDialog, FormDialogState, FormFieldEvent,
        FormFieldState, PageQuery,
    },
};
use serde::{Deserialize, Serialize};

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
    showcase: Option<Entity<CrudListState<ShowcaseRow, ShowcaseQuery>>>,
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
        let showcase = CrudListState::create(
            ShowcaseQuery::default(),
            |query| async move {
                let rows = showcase_rows();
                let total = rows.len();
                Ok(CrudPage::new(
                    rows,
                    query.page.page,
                    query.page.page_size,
                    total,
                ))
            },
            window,
            cx,
        )
        .expect("desktop_basic 查询必须可序列化");
        showcase.update(cx, CrudListState::load_current);
        self.showcase = Some(showcase);
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
        let showcase = self
            .showcase
            .as_ref()
            .expect("desktop_basic CRUD 状态必须先完成 initialize");

        let search_filter = field()
            .label("组件筛选")
            .description("静态示例控件，不会发起查询。")
            .w(px(280.0))
            .child(
                Input::new(search_input)
                    .with_size(component_size)
                    .cleanable(true),
            );

        let open_form_action = Button::new("desktop-basic-open-form-dialog")
            .primary()
            .with_size(component_size)
            .icon(IconName::Plus)
            .label("打开表单")
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_form_dialog(window, cx);
            }));

        CrudPanel::new(
            "desktop-basic-showcase",
            "Nexora 组件 Showcase",
            showcase.clone(),
        )
        .description("通过 nexora::desktop facade 展示轻量、可组合的桌面组件。")
        .filter(search_filter)
        .header_action(open_form_action)
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

#[derive(Clone, Default, Serialize, Deserialize, nexora::CrudQuery)]
#[nexora(page_size(default = 25, min = 15, max = 100, options = [15, 25, 50, 100]))]
struct ShowcaseQuery {
    #[nexora(pagination)]
    #[serde(flatten)]
    page: PageQuery,
    #[nexora(filter(label = "关键词", placeholder = "组件名称", control = "input"))]
    keyword: Option<String>,
}

#[derive(Clone, nexora::CrudTableRow)]
struct ShowcaseRow {
    #[nexora(row_id, skip)]
    id: &'static str,
    #[nexora(column(title = "组件", width = 180.))]
    component: &'static str,
    #[nexora(column(title = "用途", width = 340.))]
    usage: &'static str,
    #[nexora(column(title = "状态", width = 120., align = "right"))]
    status: &'static str,
}

fn showcase_rows() -> Vec<ShowcaseRow> {
    vec![
        ShowcaseRow {
            id: "crud-panel",
            component: "CrudPanel",
            usage: "强类型分页、缓存与表格骨架",
            status: "Ready",
        },
        ShowcaseRow {
            id: "form-field",
            component: "Form / Field",
            usage: "官方字段布局与无视觉校验状态",
            status: "Ready",
        },
        ShowcaseRow {
            id: "sidebar-region",
            component: "SidebarRegion",
            usage: "Sidebar 插槽稳定交互区域",
            status: "Ready",
        },
    ]
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
