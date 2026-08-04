//! GPUI 更新确认、进度弹窗与应用级更新协调器。

use std::thread;

use gpui::{
    AnyElement, App, Context, Entity, Global, IntoElement, ParentElement as _, Render, Task,
    Window, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    h_flex,
    notification::Notification,
    progress::Progress,
    v_flex,
};

use crate::{CancellationToken, StagedUpdate, UpdateConfig, UpdateEvent, UpdateRelease, Updater};

struct UpdateCoordinatorGlobal {
    coordinator: Entity<UpdateCoordinator>,
}

impl Global for UpdateCoordinatorGlobal {}

/// 在主窗口创建完成后静默检查一次应用更新。
///
/// 启动时会先恢复并重新验证已经保留的待安装更新；恢复成功后直接启动 sidecar 并退出旧
/// 进程，只有没有有效缓存时才发起网络检查。检查阶段不会打开弹窗；只有发现新版本时才显示
/// 更新确认框。同一应用进程只会创建一个更新协调器，后续手动检查会复用正在进行的会话。
pub fn start_update_check_on_launch(config: UpdateConfig, window: &mut Window, cx: &mut App) {
    let coordinator = update_coordinator(config, cx);
    window.defer(cx, move |window, cx| {
        coordinator.update(cx, |this, cx| {
            this.start_launch_flow(window, cx);
        });
    });
}

/// 在当前 GPUI 窗口中打开完整的应用更新流程。
///
/// 手动入口会立即显示检查状态；发现新版本后先询问用户，再按用户选择前台或后台下载。
/// 更新准备完成后，用户可以选择立即退出、替换应用并重新启动。
pub fn open_update_dialog(config: UpdateConfig, window: &mut Window, cx: &mut App) {
    let coordinator = update_coordinator(config, cx);
    coordinator.update(cx, |this, cx| {
        this.start_check(true, window, cx);
    });
}

fn update_coordinator(config: UpdateConfig, cx: &mut App) -> Entity<UpdateCoordinator> {
    if let Some(global) = cx.try_global::<UpdateCoordinatorGlobal>() {
        return global.coordinator.clone();
    }

    let coordinator = cx.new(|_| UpdateCoordinator::new(config));
    cx.set_global(UpdateCoordinatorGlobal {
        coordinator: coordinator.clone(),
    });
    coordinator
}

struct UpdateCoordinator {
    config: UpdateConfig,
    status: UpdateDialogStatus,
    release: Option<UpdateRelease>,
    cancellation: Option<CancellationToken>,
    task: Option<Task<()>>,
    manual_check: bool,
    background_download: bool,
}

impl UpdateCoordinator {
    fn new(config: UpdateConfig) -> Self {
        Self {
            config,
            status: UpdateDialogStatus::Idle,
            release: None,
            cancellation: None,
            task: None,
            manual_check: false,
            background_download: false,
        }
    }

    fn start_launch_flow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.is_busy() {
            return;
        }
        self.cancel();
        self.status = UpdateDialogStatus::Checking;
        let updater = Updater::new(self.config.clone());
        let (sender, receiver) = async_channel::bounded(1);
        if let Err(error) = thread::Builder::new()
            .name("nexora-update-restore".to_owned())
            .spawn(move || {
                let install_failure = match updater.take_install_failure() {
                    Ok(failure) => failure,
                    Err(error) => {
                        tracing::warn!(error = %error, "无法读取上次 Windows 更新安装结果");
                        None
                    }
                };
                _ = sender.send_blocking((install_failure, updater.restore_pending()));
            })
        {
            tracing::warn!(error = %error, "无法启动待安装更新恢复线程");
            self.status = UpdateDialogStatus::Idle;
            self.start_check(false, window, cx);
            return;
        }

        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let Ok((install_failure, result)) = receiver.recv().await else {
                return;
            };
            _ = this.update_in(cx, |this, window, cx| {
                if let Some(message) = install_failure {
                    window
                        .push_notification(Notification::error(message).title("上次更新失败"), cx);
                }
                match result {
                    Ok(Some(staged)) => match staged.prepare_restart() {
                        Ok(()) => cx.quit(),
                        Err(error) => {
                            tracing::warn!(error = %error, "无法启动已恢复更新的 sidecar");
                            let message = error.to_string();
                            window.push_notification(
                                Notification::error(message.clone()).title("更新安装失败"),
                                cx,
                            );
                            this.status = UpdateDialogStatus::Failed(message);
                            cx.notify();
                        }
                    },
                    Ok(None) => {
                        this.status = UpdateDialogStatus::Idle;
                        this.start_check(false, window, cx);
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "恢复待安装更新失败");
                        this.status = UpdateDialogStatus::Idle;
                        this.start_check(false, window, cx);
                    }
                }
            });
        }));
        cx.notify();
    }

    fn start_check(&mut self, manual_check: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.is_busy() {
            if manual_check {
                open_progress_dialog(cx.entity(), self.mandatory(), window, cx);
            }
            return;
        }

        self.cancel();
        self.status = UpdateDialogStatus::Checking;
        self.release = None;
        self.manual_check = manual_check;
        self.background_download = false;

        let session = match Updater::new(self.config.clone()).check() {
            Ok(session) => session,
            Err(error) => {
                self.status = UpdateDialogStatus::Failed(error.to_string());
                if manual_check {
                    open_progress_dialog(cx.entity(), false, window, cx);
                } else {
                    tracing::warn!(error = %error, "启动更新检查失败");
                }
                cx.notify();
                return;
            }
        };
        let events = session.events();
        self.cancellation = Some(session.cancellation());
        if manual_check {
            open_progress_dialog(cx.entity(), false, window, cx);
        }
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let finished = matches!(
                    event,
                    UpdateEvent::UpToDate
                        | UpdateEvent::UpdateAvailable(_)
                        | UpdateEvent::Failed(_)
                        | UpdateEvent::Cancelled
                );
                if this
                    .update_in(cx, |this, window, cx| {
                        this.handle_check_event(event, window, cx);
                    })
                    .is_err()
                {
                    break;
                }
                if finished {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn handle_check_event(
        &mut self,
        event: UpdateEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            UpdateEvent::Checking => self.status = UpdateDialogStatus::Checking,
            UpdateEvent::UpToDate => {
                self.status = UpdateDialogStatus::UpToDate;
                self.cancellation = None;
            }
            UpdateEvent::UpdateAvailable(release) => {
                self.status = UpdateDialogStatus::UpdateAvailable;
                self.release = Some(release.clone());
                self.cancellation = None;
                if self.manual_check {
                    window.close_dialog(cx);
                    open_update_prompt(cx.entity(), release, window, cx);
                } else {
                    push_update_notification(cx.entity(), release, window, cx);
                }
            }
            UpdateEvent::Failed(message) => {
                self.cancellation = None;
                if self.manual_check {
                    self.status = UpdateDialogStatus::Failed(message);
                } else {
                    self.status = UpdateDialogStatus::Idle;
                    tracing::warn!(error = %message, "启动更新检查失败");
                }
            }
            UpdateEvent::Cancelled => {
                self.status = UpdateDialogStatus::Cancelled;
                self.cancellation = None;
            }
            UpdateEvent::Downloading { .. }
            | UpdateEvent::Verifying
            | UpdateEvent::Staging
            | UpdateEvent::ReadyToRestart(_) => {}
        }
        cx.notify();
    }

    fn start_download(
        &mut self,
        release: UpdateRelease,
        background: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel();
        self.release = Some(release.clone());
        self.status = UpdateDialogStatus::UpdateAvailable;
        self.background_download = background;

        let session = match Updater::new(self.config.clone()).download(release) {
            Ok(session) => session,
            Err(error) => {
                self.status = UpdateDialogStatus::Failed(error.to_string());
                if background {
                    window.push_notification(
                        Notification::error(error.to_string()).title("更新下载失败"),
                        cx,
                    );
                } else {
                    open_progress_dialog(cx.entity(), self.mandatory(), window, cx);
                }
                cx.notify();
                return;
            }
        };
        let events = session.events();
        self.cancellation = Some(session.cancellation());
        if !background {
            open_progress_dialog(cx.entity(), self.mandatory(), window, cx);
        }
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let finished = matches!(
                    event,
                    UpdateEvent::ReadyToRestart(_)
                        | UpdateEvent::Failed(_)
                        | UpdateEvent::Cancelled
                );
                if this
                    .update_in(cx, |this, window, cx| {
                        this.handle_download_event(event, window, cx);
                    })
                    .is_err()
                {
                    break;
                }
                if finished {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn handle_download_event(
        &mut self,
        event: UpdateEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            UpdateEvent::Downloading { downloaded, total } => {
                self.status = UpdateDialogStatus::Downloading { downloaded, total };
            }
            UpdateEvent::Verifying => self.status = UpdateDialogStatus::Verifying,
            UpdateEvent::Staging => self.status = UpdateDialogStatus::Staging,
            UpdateEvent::ReadyToRestart(staged) => {
                self.status = UpdateDialogStatus::ReadyToRestart(Box::new(staged));
                self.cancellation = None;
                if self.background_download {
                    open_progress_dialog(cx.entity(), self.mandatory(), window, cx);
                }
            }
            UpdateEvent::Failed(message) => {
                self.status = UpdateDialogStatus::Failed(message.clone());
                self.cancellation = None;
                if self.background_download {
                    window
                        .push_notification(Notification::error(message).title("更新下载失败"), cx);
                }
            }
            UpdateEvent::Cancelled => {
                self.status = UpdateDialogStatus::Cancelled;
                self.cancellation = None;
            }
            UpdateEvent::Checking | UpdateEvent::UpToDate | UpdateEvent::UpdateAvailable(_) => {}
        }
        cx.notify();
    }

    fn defer_update(&mut self, cx: &mut Context<Self>) {
        self.cancel();
        self.status = UpdateDialogStatus::Idle;
        self.release = None;
        cx.notify();
    }

    fn cancel(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.cancel();
        }
        self.task.take();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        let UpdateDialogStatus::ReadyToRestart(staged) = &self.status else {
            return;
        };

        match staged.prepare_restart() {
            Ok(()) => cx.quit(),
            Err(error) => {
                self.status = UpdateDialogStatus::Failed(error.to_string());
                cx.notify();
            }
        }
    }

    fn restart_later(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let UpdateDialogStatus::ReadyToRestart(staged) = &mut self.status else {
            return;
        };

        match staged.preserve_for_next_launch() {
            Ok(()) => {
                self.status = UpdateDialogStatus::Idle;
                self.release = None;
                window.close_dialog(cx);
                cx.notify();
            }
            Err(error) => {
                self.status = UpdateDialogStatus::Failed(error.to_string());
                cx.notify();
            }
        }
    }

    fn mandatory(&self) -> bool {
        self.release
            .as_ref()
            .is_some_and(|release| release.mandatory)
    }
}

fn push_update_notification(
    coordinator: Entity<UpdateCoordinator>,
    release: UpdateRelease,
    window: &mut Window,
    cx: &mut App,
) {
    let version = format!("v{} ({})", release.version, release.build_number);
    window.push_notification(
        Notification::info(format!("{version} 已可用"))
            .title("发现应用更新")
            .action(move |_notification, _, cx| {
                let coordinator = coordinator.clone();
                let release = release.clone();
                let notification = cx.entity().downgrade();
                Button::new("background-update-review")
                    .label("查看更新")
                    .primary()
                    .on_click(move |_, window, cx| {
                        _ = notification.update(cx, |notification, cx| {
                            notification.dismiss(window, cx);
                        });
                        open_update_prompt(coordinator.clone(), release.clone(), window, cx);
                    })
            }),
        cx,
    );
}

impl Drop for UpdateCoordinator {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl Render for UpdateCoordinator {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.status {
            UpdateDialogStatus::Idle => v_flex().child("尚未开始检查更新。").into_any_element(),
            UpdateDialogStatus::Checking => update_progress_content(
                "正在检查更新...",
                Progress::new("update-checking").loading(true),
                None,
            ),
            UpdateDialogStatus::UpdateAvailable => update_progress_content(
                "正在准备下载更新...",
                Progress::new("update-preparing").loading(true),
                self.release.as_ref().map(|release| {
                    format!("新版本 v{} ({})", release.version, release.build_number)
                }),
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
            UpdateDialogStatus::ReadyToRestart(staged) => {
                ready_content(staged, self.mandatory(), cx)
            }
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

        v_flex().min_w_0().gap_4().child(content).when(
            self.status.can_cancel() && !self.mandatory(),
            |this| {
                this.child(
                    h_flex().justify_end().child(
                        Button::new("update-cancel")
                            .label("取消")
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel();
                                this.status = UpdateDialogStatus::Cancelled;
                                window.close_dialog(cx);
                                cx.notify();
                            })),
                    ),
                )
            },
        )
    }
}

fn open_update_prompt(
    coordinator: Entity<UpdateCoordinator>,
    release: UpdateRelease,
    window: &mut Window,
    cx: &mut App,
) {
    let version = format!("v{} ({})", release.version, release.build_number);
    let mandatory = release.mandatory;
    window.open_alert_dialog(cx, move |alert, _, _| {
        let immediate_coordinator = coordinator.clone();
        let immediate_release = release.clone();
        let footer = DialogFooter::new()
            .when(!mandatory, |footer| {
                let later_coordinator = coordinator.clone();
                let background_coordinator = coordinator.clone();
                let background_release = release.clone();
                footer
                    .child(
                        Button::new("update-later")
                            .label("稍后")
                            .outline()
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                later_coordinator.update(cx, |this, cx| this.defer_update(cx));
                            }),
                    )
                    .child(
                        Button::new("update-background")
                            .label("后台下载")
                            .outline()
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                background_coordinator.update(cx, |this, cx| {
                                    this.start_download(
                                        background_release.clone(),
                                        true,
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    )
            })
            .child(
                Button::new("update-immediate")
                    .label("立即更新")
                    .primary()
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        immediate_coordinator.update(cx, |this, cx| {
                            this.start_download(immediate_release.clone(), false, window, cx);
                        });
                    }),
            );
        alert
            .title(if mandatory {
                "需要更新后才能继续使用"
            } else {
                "发现新版本"
            })
            .description(if mandatory {
                format!("当前版本已停止支持，请更新到 {version}。")
            } else {
                format!("{version} 已可用，现在要更新吗？")
            })
            .footer(footer)
            .overlay_closable(false)
            .close_button(false)
            .keyboard(false)
    });
}

fn open_progress_dialog(
    coordinator: Entity<UpdateCoordinator>,
    mandatory: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if window.has_active_dialog(cx) {
        return;
    }
    let cancel_update = coordinator.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(px(420.0))
            .overlay_closable(false)
            .close_button(false)
            .keyboard(!mandatory)
            .title("应用更新")
            .child(coordinator.clone())
            .on_close({
                let cancel_update = cancel_update.clone();
                move |_, _, cx| {
                    let cancel_update = cancel_update.clone();
                    cx.defer(move |cx| {
                        cancel_update.update(cx, |this, _| {
                            if !this.mandatory() {
                                this.cancel();
                            }
                        });
                    });
                }
            })
    });
}

fn ready_content(
    staged: &StagedUpdate,
    mandatory: bool,
    cx: &mut Context<UpdateCoordinator>,
) -> AnyElement {
    let version = format!(
        "v{} ({})",
        staged.release().version,
        staged.release().build_number
    );
    v_flex()
        .gap_4()
        .child(
            h_flex()
                .gap_2()
                .child(Icon::new(IconName::CircleCheck).text_color(cx.theme().success))
                .child("更新已准备完成，请重启后查看最新功能。"),
        )
        .child(
            h_flex().justify_between().child(version).child(
                h_flex()
                    .gap_2()
                    .when(!mandatory, |buttons| {
                        buttons.child(
                            Button::new("update-restart-later")
                                .label("稍后重启")
                                .outline()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.restart_later(window, cx);
                                })),
                        )
                    })
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
    Idle,
    Checking,
    UpdateAvailable,
    Downloading { downloaded: u64, total: Option<u64> },
    Verifying,
    Staging,
    ReadyToRestart(Box<StagedUpdate>),
    UpToDate,
    Failed(String),
    Cancelled,
}

impl UpdateDialogStatus {
    fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking
                | Self::UpdateAvailable
                | Self::Downloading { .. }
                | Self::Verifying
                | Self::Staging
        )
    }

    fn can_cancel(&self) -> bool {
        self.is_busy()
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
