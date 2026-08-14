//! 一次性服务账号 Secret/PAT 展示对话框。

use gpui::{Context, Entity, Render, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Sizable as _, alert::Alert, clipboard::Clipboard, h_flex, v_flex,
};
use ui::{FormDialog, FormDialogState};

use crate::desktop::contract::{
    CreateServiceAccountCredentialResponse, ServiceAccountCredentialSecret,
};

pub(in crate::defaults::account::users) struct CredentialSecretDialog {
    form: Entity<FormDialogState>,
    secret: Option<ServiceAccountCredentialSecret>,
}

impl CredentialSecretDialog {
    pub(in crate::defaults::account::users) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            form: cx.new(FormDialogState::new),
            secret: None,
        }
    }

    pub(super) fn open(
        &mut self,
        response: CreateServiceAccountCredentialResponse,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.secret = Some(response.secret);
        self.form.update(cx, |form, cx| {
            form.reset_fields(cx);
            form.open(window, cx);
        });
        cx.notify();
    }

    fn close_and_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.secret = None;
        self.form.update(cx, |form, cx| form.close(window, cx));
        cx.notify();
    }
}

impl Render for CredentialSecretDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let content = v_flex()
            .gap_3()
            .child(
                Alert::warning(
                    "service-account-one-time-secret-warning",
                    "关闭后无法再次读取。请先复制到设备的安全密钥存储。",
                )
                .title("敏感内容仅显示一次"),
            )
            .children(self.secret.as_ref().map(|secret| {
                match secret {
                    ServiceAccountCredentialSecret::ClientCredentials {
                        client_id,
                        client_secret,
                    } => v_flex()
                        .gap_2()
                        .child(secret_row(
                            "Client ID",
                            client_id,
                            "copy-service-account-client-id",
                            cx,
                        ))
                        .child(secret_row(
                            "Client Secret",
                            client_secret,
                            "copy-service-account-client-secret",
                            cx,
                        ))
                        .into_any_element(),
                    ServiceAccountCredentialSecret::PersonalAccessToken { token } => secret_row(
                        "Personal Access Token",
                        token,
                        "copy-service-account-pat",
                        cx,
                    )
                    .into_any_element(),
                }
            }));
        let dialog = cx.entity().downgrade();
        let dialog_for_cancel = dialog.clone();
        FormDialog::new("credential-secret-dialog", self.form.clone())
            .title("保存一次性凭据")
            .description("只有确认已保存后才关闭此对话框；组件关闭时会立即清除内存中的明文。")
            .section(content)
            .cancel_label("继续复制")
            .submit_label("我已保存并关闭")
            .submit_disabled(self.secret.is_none())
            .with_size(component_size)
            .on_cancel(move |_, _, cx| {
                _ = dialog_for_cancel.update(cx, |_, cx| cx.notify());
            })
            .on_submit(move |_, window, cx| {
                _ = dialog.update(cx, |dialog, cx| dialog.close_and_clear(window, cx));
            })
    }
}

fn secret_row(
    label: &'static str,
    value: &str,
    id: &'static str,
    cx: &gpui::App,
) -> impl IntoElement {
    v_flex().gap_1().child(div().text_sm().child(label)).child(
        h_flex()
            .gap_2()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().tokens.group_box)
            .child(div().min_w_0().flex_1().truncate().child(value.to_owned()))
            .child(
                Clipboard::new(id)
                    .value(value.to_owned())
                    .tooltip(format!("复制 {label}")),
            ),
    )
}
