//! Console 项目管理功能模块。
//!
//! 该模块展示项目列表页的基础结构，用于演示一个 feature 如何封装自己的列表内容。

use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _, Theme,
    button::Button,
    table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow},
    tag::Tag,
};
use ui::Card;

/// 项目管理功能视图。
///
/// 当前实现使用静态项目数据作为模板示例，真实应用可以在这里接入项目扫描、最近打开记录或远程同步状态。
#[derive(Default, nexora::Feature)]
#[nexora(
    title = "项目",
    path = "/projects",
    group = "project-management",
    icon = "folder-open",
    order = 10
)]
pub struct ProjectsFeature;

impl nexora::FeatureElement for ProjectsFeature {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_content(cx)
    }
}

/// 渲染项目页面及其两个目录子页面共享的内容。
pub(crate) fn render_content<T>(cx: &mut Context<T>) -> AnyElement
where
    T: 'static,
{
    let component_size = theme::component_size(cx);
    let theme = cx.theme();
    let rows = project_rows()
        .iter()
        .copied()
        .map(|row| project_table_row(row, theme))
        .collect::<Vec<_>>();

    Card::new()
        .p_4()
        .gap_4()
        .child(section_header(
            "项目工作区",
            "把项目列表、最近打开和环境状态收束在一个 feature 中。",
            theme,
        ))
        .child(
            Table::new()
                .with_size(component_size)
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .child(
                    TableHeader::new().child(
                        TableRow::new()
                            .child(TableHead::new().w(px(220.)).child("项目"))
                            .child(TableHead::new().child("职责"))
                            .child(TableHead::new().w(px(140.)).child("状态")),
                    ),
                )
                .child(TableBody::new().children(rows)),
        )
        .into_any_element()
}

/// 项目列表中的一行静态示例数据。
///
/// 真实应用可以把该类型替换为从工作区扫描、最近打开记录或远程服务中加载的数据模型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectRow {
    name: &'static str,
    path: &'static str,
    description: &'static str,
    status: ProjectStatus,
}

impl ProjectRow {
    /// 返回项目在列表中展示的名称。
    ///
    /// 该名称通常对应 Cargo package、桌面应用或内部运行时模块的短名称。
    pub fn name(self) -> &'static str {
        self.name
    }

    /// 返回项目在当前 workspace 中的相对路径。
    ///
    /// 路径用于帮助使用者快速定位该项目所在目录。
    pub fn path(self) -> &'static str {
        self.path
    }

    /// 返回项目职责说明。
    ///
    /// 该说明会显示在项目列表中，帮助区分不同 workspace 成员的责任边界。
    pub fn description(self) -> &'static str {
        self.description
    }

    /// 返回项目当前状态。
    ///
    /// 状态会被渲染成标签，用于标识该项目在模板中的角色。
    pub fn status(self) -> ProjectStatus {
        self.status
    }
}

/// 项目在模板工作区中的状态。
///
/// 该状态只描述示例项目的角色，真实项目可以按业务需要扩展为构建状态、同步状态等。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStatus {
    /// 当前主要桌面入口项目。
    Active,
    /// 被多个应用复用的核心运行时项目。
    Core,
    /// 辅助本地构建、打包或发布的工具项目。
    Tooling,
}

impl ProjectStatus {
    /// 返回项目状态在界面标签中展示的短文本。
    ///
    /// 该文本保持英文小写，便于在紧凑标签中稳定展示。
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Core => "core",
            Self::Tooling => "tooling",
        }
    }

    fn tag(self) -> Tag {
        match self {
            Self::Active => Tag::success(),
            Self::Core => Tag::info(),
            Self::Tooling => Tag::warning(),
        }
        .small()
        .outline()
        .child(self.label())
    }
}

/// 返回项目管理页默认展示的模板项目列表。
///
/// 返回值顺序就是表格渲染顺序，用于保持示例界面和集成测试中的导航认知一致。
pub fn project_rows() -> &'static [ProjectRow] {
    static ROWS: [ProjectRow; 3] = [
        ProjectRow {
            name: "Console",
            path: "examples/console",
            description: "GPUI 桌面入口",
            status: ProjectStatus::Active,
        },
        ProjectRow {
            name: "Desktop Runtime",
            path: "crates/desktop",
            description: "统一启动与窗口配置",
            status: ProjectStatus::Core,
        },
        ProjectRow {
            name: "Nexora CLI",
            path: "crates/nexora",
            description: "Nexora 项目创建与初始化工具",
            status: ProjectStatus::Tooling,
        },
    ];

    &ROWS
}

fn section_header(title: &'static str, description: &'static str, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_lg()
                        .font_bold()
                        .text_color(theme.foreground)
                        .child(title),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(description),
                ),
        )
        .child(
            Button::new("import-project")
                .small()
                .outline()
                .icon(IconName::FolderOpen)
                .label("导入项目")
                .disabled(true),
        )
        .into_any_element()
}

fn project_table_row(row: ProjectRow, theme: &Theme) -> TableRow {
    TableRow::new()
        .child(
            TableCell::new().w(px(220.)).child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(IconName::FolderOpen).text_color(theme.muted_foreground))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .font_bold()
                                    .text_color(theme.foreground)
                                    .child(row.name()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(row.path()),
                            ),
                    ),
            ),
        )
        .child(
            TableCell::new().child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(row.description()),
            ),
        )
        .child(TableCell::new().w(px(140.)).child(row.status().tag()))
}
