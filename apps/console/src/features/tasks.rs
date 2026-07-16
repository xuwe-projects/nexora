//! Console 任务管理功能模块。
//!
//! 该模块展示构建、打包和发布任务队列，说明异步工作流可以作为独立 feature 管理。

use gpui::{Context, IntoElement, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, Theme,
    button::{Button, ButtonVariants as _},
    table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow},
    tag::Tag,
};
use ui::Card;

/// 任务管理功能视图。
///
/// 当前实现展示静态任务队列。真实项目可以把这些行替换为后台任务 Entity 或事件订阅结果。
#[derive(Default, nexora::Feature)]
#[nexora(
    title = "任务",
    path = "/tasks",
    section = "工作台",
    icon = "square-terminal",
    order = 20
)]
pub struct TasksFeature;

impl nexora::FeatureElement for TasksFeature {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let theme = cx.theme();
        let rows = task_rows()
            .iter()
            .copied()
            .map(|row| task_table_row(row, theme))
            .collect::<Vec<_>>();

        Card::new()
            .p_4()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
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
                                    .child("任务队列"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("这里可以承载构建、打包、上传、公证等长耗时任务。"),
                            ),
                    )
                    .child(
                        Button::new("run-build")
                            .primary()
                            .small()
                            .icon(IconName::Play)
                            .label("运行构建")
                            .disabled(true),
                    ),
            )
            .child(
                Table::new()
                    .with_size(component_size)
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        TableHeader::new().child(
                            TableRow::new()
                                .child(TableHead::new().child("命令"))
                                .child(TableHead::new().w(px(120.)).child("阶段"))
                                .child(TableHead::new().w(px(120.)).child("状态"))
                                .child(TableHead::new().w(px(96.)).text_right().child("耗时")),
                        ),
                    )
                    .child(TableBody::new().children(rows)),
            )
            .into_any_element()
    }
}

/// 任务队列中的一行静态示例数据。
///
/// 真实应用可以把该类型替换为后台任务 Entity、命令执行记录或发布流水线事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRow {
    command: &'static str,
    kind: &'static str,
    status: TaskStatus,
    duration: &'static str,
}

impl TaskRow {
    /// 返回任务对应的命令或动作名称。
    ///
    /// 该文本会作为任务列表的主要识别信息展示。
    pub fn command(self) -> &'static str {
        self.command
    }

    /// 返回任务所属阶段。
    ///
    /// 阶段用于帮助使用者区分开发校验、本地打包和发布签名等流程节点。
    pub fn kind(self) -> &'static str {
        self.kind
    }

    /// 返回任务当前状态。
    ///
    /// 状态会被渲染为标签，用于表达该任务是否已通过、可运行或被外部条件阻塞。
    pub fn status(self) -> TaskStatus {
        self.status
    }

    /// 返回任务展示用耗时。
    ///
    /// 对尚未执行或被阻塞的任务，该值可以是说明性文本。
    pub fn duration(self) -> &'static str {
        self.duration
    }
}

/// 构建或发布任务的展示状态。
///
/// 该状态面向界面表达，不等同于后台任务系统中的完整状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// 任务已经执行完成并通过。
    Passed,
    /// 任务已经准备好，可以由用户或调度器触发。
    Ready,
    /// 任务因为缺少凭据、依赖或其他外部条件暂时不能执行。
    Blocked,
}

impl TaskStatus {
    /// 返回任务状态在界面标签中展示的短文本。
    ///
    /// 该文本保持英文小写，便于在表格中的紧凑标签里稳定展示。
    pub fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Ready => "ready",
            Self::Blocked => "blocked",
        }
    }

    fn tag(self) -> Tag {
        match self {
            Self::Passed => Tag::success(),
            Self::Ready => Tag::info(),
            Self::Blocked => Tag::warning(),
        }
        .small()
        .outline()
        .child(self.label())
    }
}

/// 返回任务管理页默认展示的模板任务队列。
///
/// 返回值顺序就是表格渲染顺序，用于稳定展示从开发校验到产物校验的示例流程。
pub fn task_rows() -> &'static [TaskRow] {
    static ROWS: [TaskRow; 4] = [
        TaskRow {
            command: "cargo check --workspace",
            kind: "开发校验",
            status: TaskStatus::Passed,
            duration: "18s",
        },
        TaskRow {
            command: "xuwecli build --mode local",
            kind: "本地打包",
            status: TaskStatus::Ready,
            duration: "等待中",
        },
        TaskRow {
            command: "codesign + notarytool",
            kind: "发布签名",
            status: TaskStatus::Blocked,
            duration: "缺少凭据",
        },
        TaskRow {
            command: "sha256 sidecar",
            kind: "产物校验",
            status: TaskStatus::Passed,
            duration: "1s",
        },
    ];

    &ROWS
}

fn task_table_row(row: TaskRow, theme: &Theme) -> TableRow {
    TableRow::new()
        .child(
            TableCell::new().child(
                div()
                    .font_bold()
                    .text_color(theme.foreground)
                    .child(row.command()),
            ),
        )
        .child(
            TableCell::new()
                .w(px(120.))
                .child(div().text_color(theme.muted_foreground).child(row.kind())),
        )
        .child(TableCell::new().w(px(120.)).child(row.status().tag()))
        .child(
            TableCell::new().w(px(96.)).text_right().child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(row.duration()),
            ),
        )
}
