//! GPUI 更新进度弹窗。

use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement as _, Render, Task, Window, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    progress::Progress,
    v_flex,
};

use crate::{CancellationToken, StagedUpdate, UpdateConfig, UpdateEvent, UpdateRelease, Updater};

/// 在当前 GPUI 窗口中打开完整的应用更新弹窗。
///
/// 弹窗会立即开始检查当前通道的 `latest.json`，随后展示下载字节进度、校验与暂存状态。
/// 更新准备完成后，用户可以选择立即退出、替换 `.app` 并重新启动。
pub fn open_update_dialog(config: UpdateConfig, window: &mut Window, cx: &mut App) {
    let update = cx.new(|cx| UpdateDialog::new(config, cx));
    let cancel_update = update.clone();

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(px(420.0))
            .overlay_closable(false)
            .title("应用更新")
            .child(update.clone())
            .on_close({
                let cancel_update = cancel_update.clone();
                move |_, _, cx| {
                    cancel_update.update(cx, |this, _| this.cancel());
                }
            })
    });
}

struct UpdateDialog {
    status: UpdateDialogStatus,
    cancellation: CancellationToken,
    _task: Task<()>,
}

impl UpdateDialog {
    fn new(config: UpdateConfig, cx: &mut Context<Self>) -> Self {
        let session = match Updater::new(config).start() {
            Ok(session) => session,
            Err(error) => {
                return Self {
                    status: UpdateDialogStatus::Failed(error.to_string()),
                    cancellation: CancellationToken::default(),
                    _task: Task::ready(()),
                };
            }
        };
        let events = session.events();
        let cancellation = session.cancellation();
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let finished = matches!(
                    event,
                    UpdateEvent::UpToDate
                        | UpdateEvent::ReadyToRestart(_)
                        | UpdateEvent::Failed(_)
                        | UpdateEvent::Cancelled
                );
                if this
                    .update(cx, |this, cx| {
                        this.status = UpdateDialogStatus::from_event(event);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }

                if finished {
                    break;
                }
            }
        });

        Self {
            status: UpdateDialogStatus::Checking,
            cancellation,
            _task: task,
        }
    }

    fn cancel(&self) {
        self.cancellation.cancel();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        let UpdateDialogStatus::ReadyToRestart(staged) = &self.status else {
            return;
        };

        match staged.prepare_restart() {
            Ok(()) => {
                cx.quit();
            }
            Err(error) => {
                self.status = UpdateDialogStatus::Failed(error.to_string());
                cx.notify();
            }
        }
    }
}

impl Drop for UpdateDialog {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl Render for UpdateDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.status {
            UpdateDialogStatus::Checking => update_progress_content(
                "正在检查更新...",
                Progress::new("update-checking").loading(true),
                None,
            ),
            UpdateDialogStatus::UpdateAvailable(release) => update_progress_content(
                format!("发现新版本 v{}", release.version),
                Progress::new("update-found").loading(true),
                Some("正在准备下载...".to_owned()),
            ),
            UpdateDialogStatus::Downloading { downloaded, total } => {
                let progress = total
                    .filter(|total| *total > 0)
                    .map(|total| (*downloaded as f32 / total as f32) * 100.0);
                let detail = total
                    .map(|total| format!("{} / {}", format_bytes(*downloaded), format_bytes(total)))
                    .unwrap_or_else(|| format!("已下载 {}", format_bytes(*downloaded)));
                update_progress_content(
                    "正在下载更新...",
                    progress
                        .map(|value| Progress::new("update-downloading").value(value))
                        .unwrap_or_else(|| Progress::new("update-downloading").loading(true)),
                    Some(detail),
                )
            }
            UpdateDialogStatus::Verifying => update_progress_content(
                "正在验证更新...",
                Progress::new("update-verifying").loading(true),
                Some("正在校验安装包和应用签名".to_owned()),
            ),
            UpdateDialogStatus::Staging => update_progress_content(
                "正在安装更新...",
                Progress::new("update-staging").loading(true),
                Some("正在准备退出后替换应用".to_owned()),
            ),
            UpdateDialogStatus::ReadyToRestart(staged) => ready_content(staged, cx),
            UpdateDialogStatus::UpToDate => v_flex()
                .gap_4()
                .child(
                    h_flex()
                        .gap_2()
                        .child(Icon::new(IconName::CircleCheck).text_color(cx.theme().success))
                        .child("当前已经是最新版本。"),
                )
                .child(
                    h_flex().justify_end().child(
                        Button::new("update-close")
                            .label("完成")
                            .primary()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .into_any_element(),
            UpdateDialogStatus::Failed(message) => v_flex()
                .gap_4()
                .child(
                    h_flex()
                        .items_start()
                        .gap_2()
                        .child(Icon::new(IconName::CircleX).text_color(cx.theme().danger))
                        .child(v_flex().gap_1().child("更新失败").child(message.clone())),
                )
                .child(
                    h_flex().justify_end().child(
                        Button::new("update-failed-close")
                            .label("关闭")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .into_any_element(),
            UpdateDialogStatus::Cancelled => v_flex()
                .gap_4()
                .child("更新已取消。")
                .child(
                    h_flex().justify_end().child(
                        Button::new("update-cancelled-close")
                            .label("关闭")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .into_any_element(),
        };

        v_flex()
            .min_w_0()
            .gap_4()
            .child(content)
            .when(self.status.can_cancel(), |this| {
                this.child(
                    h_flex().justify_end().child(
                        Button::new("update-cancel")
                            .label("取消")
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel();
                                window.close_dialog(cx);
                            })),
                    ),
                )
            })
    }
}

fn ready_content(staged: &StagedUpdate, cx: &mut Context<UpdateDialog>) -> AnyElement {
    let version = format!(
        "v{} ({})",
        staged.release().version,
        staged.release().bundle_version
    );
    v_flex()
        .gap_4()
        .child(
            h_flex()
                .gap_2()
                .child(Icon::new(IconName::CircleCheck).text_color(cx.theme().success))
                .child("安装成功，请重启后查看最新功能。"),
        )
        .child(
            h_flex().justify_between().child(version).child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("update-later")
                            .label("取消")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("update-restart")
                            .label("立即重启")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.restart(cx))),
                    ),
            ),
        )
        .into_any_element()
}

enum UpdateDialogStatus {
    Checking,
    UpdateAvailable(UpdateRelease),
    Downloading { downloaded: u64, total: Option<u64> },
    Verifying,
    Staging,
    ReadyToRestart(StagedUpdate),
    UpToDate,
    Failed(String),
    Cancelled,
}

impl UpdateDialogStatus {
    fn from_event(event: UpdateEvent) -> Self {
        match event {
            UpdateEvent::Checking => Self::Checking,
            UpdateEvent::UpToDate => Self::UpToDate,
            UpdateEvent::UpdateAvailable(release) => Self::UpdateAvailable(release),
            UpdateEvent::Downloading { downloaded, total } => {
                Self::Downloading { downloaded, total }
            }
            UpdateEvent::Verifying => Self::Verifying,
            UpdateEvent::Staging => Self::Staging,
            UpdateEvent::ReadyToRestart(staged) => Self::ReadyToRestart(staged),
            UpdateEvent::Failed(message) => Self::Failed(message),
            UpdateEvent::Cancelled => Self::Cancelled,
        }
    }

    fn can_cancel(&self) -> bool {
        matches!(
            self,
            Self::Checking
                | Self::UpdateAvailable(_)
                | Self::Downloading { .. }
                | Self::Verifying
                | Self::Staging
        )
    }
}

fn update_progress_content(
    title: impl IntoElement,
    progress: Progress,
    detail: Option<String>,
) -> AnyElement {
    v_flex()
        .gap_3()
        .child(title)
        .child(progress)
        .when_some(detail, |this, detail| this.child(detail))
        .into_any_element()
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;

    if bytes >= GIB {
        return format!("{:.1} GB", bytes / GIB);
    }
    if bytes >= MIB {
        return format!("{:.1} MB", bytes / MIB);
    }
    if bytes >= KIB {
        return format!("{:.1} KB", bytes / KIB);
    }

    format!("{} B", bytes as u64)
}
