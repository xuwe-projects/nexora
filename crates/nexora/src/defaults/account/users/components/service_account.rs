//! 使用公共 FormDialog 创建服务账号及可选初始凭据。

use std::collections::BTreeSet;

use chrono::{Duration, Utc};
use gpui::{Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*};
use gpui_component::{
    Disableable as _, Sizable as _, StyledExt as _, WindowExt as _,
    alert::Alert,
    button::ButtonVariant,
    checkbox::Checkbox,
    date_picker::{DatePicker, DatePickerState},
    dialog::DialogButtonProps,
    form::field,
    h_flex,
    input::{Input, InputEvent, InputState},
    radio::{Radio, RadioGroup},
    spinner::Spinner,
    v_flex,
};
use ui::{FormDialog, FormDialogState};

use crate::{
    defaults::account::has_permission,
    desktop::{
        api_session,
        contract::{
            CreateServiceAccountCredentialRequest, CreateServiceAccountRequest, RoleResponse,
            ServiceAccountCredentialType,
        },
    },
};

use super::{CredentialSecretDialog, UsersPage};

#[derive(Clone, Copy, PartialEq, Eq)]
enum InitialCredentialKind {
    ClientCredentials,
    PersonalAccessToken,
    None,
}

pub(in crate::defaults::account::users) struct CreateServiceAccountDialog {
    page: WeakEntity<UsersPage>,
    credential_secret: WeakEntity<CredentialSecretDialog>,
    form: Entity<FormDialogState>,
    username: Entity<InputState>,
    display_name: Entity<InputState>,
    description: Entity<InputState>,
    expiration: Entity<DatePickerState>,
    roles: Vec<RoleResponse>,
    selected_role_ids: BTreeSet<i64>,
    initial_credential: InitialCredentialKind,
    saving: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
    _task: Option<Task<()>>,
}

impl CreateServiceAccountDialog {
    pub(in crate::defaults::account::users) fn new(
        page: WeakEntity<UsersPage>,
        credential_secret: WeakEntity<CredentialSecretDialog>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(FormDialogState::new);
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("dispenser-line-a"));
        let display_name = cx.new(|cx| InputState::new(window, cx).placeholder("A 线点料机"));
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("可选用途说明"));
        let expiration = cx.new(|cx| DatePickerState::new(window, cx).date_format("%Y-%m-%d"));
        let subscriptions = vec![
            track_input(cx, &form, &username, "username", "稳定 username"),
            track_input(cx, &form, &display_name, "display_name", "展示名称"),
            track_input(cx, &form, &description, "description", "说明"),
        ];
        Self {
            page,
            credential_secret,
            form,
            username,
            display_name,
            description,
            expiration,
            roles: Vec::new(),
            selected_role_ids: BTreeSet::new(),
            initial_credential: InitialCredentialKind::None,
            saving: false,
            error: None,
            _subscriptions: subscriptions,
            _task: None,
        }
    }

    pub(super) fn open(
        &mut self,
        roles: Vec<RoleResponse>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving {
            return;
        }
        self.reset(window, cx);
        self.roles = roles;
        self.error = None;
        self.form.update(cx, |form, cx| form.open(window, cx));
        cx.notify();
    }

    fn toggle_role(&mut self, role_id: i64, checked: bool, cx: &mut Context<Self>) {
        if !can_assign_initial_roles(cx) {
            return;
        }
        if checked {
            self.selected_role_ids.insert(role_id);
        } else {
            self.selected_role_ids.remove(&role_id);
        }
        self.form.update(cx, |form, cx| {
            form.set_field_draft(
                "role_ids",
                "初始角色",
                "",
                self.selected_role_ids
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
                cx,
            );
        });
        cx.notify();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || !has_permission(cx, "service_accounts:provision") {
            return;
        }
        if text(&self.username, cx).is_empty() || text(&self.display_name, cx).is_empty() {
            self.error = Some("稳定 username 和展示名称不能为空".to_owned());
            cx.notify();
            return;
        }
        if self.initial_credential == InitialCredentialKind::PersonalAccessToken
            && pat_expiration_is_high_risk(&self.expiration, cx)
        {
            self.form
                .update(cx, |form, cx| form.set_submitting(false, cx));
            let risk_message = pat_expiration_risk_message_for_state(&self.expiration, cx);
            let dialog = cx.entity().downgrade();
            window.open_alert_dialog(cx, move |alert, _, _| {
                let dialog = dialog.clone();
                alert
                    .title("创建高风险 PAT")
                    .description(risk_message)
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text("确认创建")
                            .ok_variant(ButtonVariant::Danger)
                            .cancel_text("返回修改")
                            .show_cancel(true),
                    )
                    .on_ok(move |_, window, cx| {
                        _ = dialog.update(cx, |dialog, cx| dialog.start_create(window, cx));
                        true
                    })
            });
            return;
        }
        self.start_create(window, cx);
    }

    fn start_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        let request = CreateServiceAccountRequest {
            username: text(&self.username, cx),
            display_name: text(&self.display_name, cx),
            description: optional_text(&self.description, cx),
            role_ids: if can_assign_initial_roles(cx) {
                self.selected_role_ids.iter().copied().collect()
            } else {
                Vec::new()
            },
        };
        let initial_credential = if has_permission(cx, "service_accounts:credentials.write") {
            self.initial_credential
        } else {
            InitialCredentialKind::None
        };
        let expires_at = self
            .expiration
            .read(cx)
            .date()
            .start()
            .and_then(|date| date.and_hms_opt(23, 59, 59))
            .map(|date| date.and_utc().timestamp());
        self.saving = true;
        self.error = None;
        self.form
            .update(cx, |form, cx| form.set_submitting(true, cx));
        let page = self.page.clone();
        let form = self.form.clone();
        let credential_secret = self.credential_secret.clone();
        let background = cx.background_spawn(async move {
            let user = session
                .create_service_account(&request)
                .map_err(|error| (None, error.user_message()))?;
            let credential_type = match initial_credential {
                InitialCredentialKind::ClientCredentials => {
                    Some(ServiceAccountCredentialType::ClientCredentials)
                }
                InitialCredentialKind::PersonalAccessToken => {
                    Some(ServiceAccountCredentialType::PersonalAccessToken)
                }
                InitialCredentialKind::None => None,
            };
            let Some(credential_type) = credential_type else {
                return Ok((user, None));
            };
            let credential_request = CreateServiceAccountCredentialRequest {
                credential_type,
                name: match credential_type {
                    ServiceAccountCredentialType::ClientCredentials => {
                        "初始 Client Credentials".to_owned()
                    }
                    ServiceAccountCredentialType::PersonalAccessToken => {
                        "初始 Personal Access Token".to_owned()
                    }
                    ServiceAccountCredentialType::Invalid => {
                        unreachable!("UI 不会创建未知凭据类型")
                    }
                },
                expires_at: (credential_type == ServiceAccountCredentialType::PersonalAccessToken)
                    .then_some(expires_at)
                    .flatten(),
            };
            let credential = session
                .create_service_account_credential(
                    user.id.as_str(),
                    next_idempotency_key(user.id.as_str()).as_str(),
                    &credential_request,
                )
                .map_err(|error| {
                    (
                        Some(user.clone()),
                        format!("凭据生成失败：{}", error.user_message()),
                    )
                })?;
            Ok((user, Some(credential)))
        });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                form.update(cx, |form, cx| form.set_submitting(false, cx));
                match result {
                    Ok((user, credential)) => {
                        _ = page.update(cx, |page, cx| {
                            page.service_account_created(user.display_name.clone(), cx);
                        });
                        form.update(cx, |form, cx| {
                            form.mark_saved(cx);
                            form.close(window, cx);
                        });
                        this.reset(window, cx);
                        if let Some(credential) = credential {
                            _ = credential_secret.update(cx, |dialog, cx| {
                                dialog.open(credential, window, cx);
                            });
                        }
                    }
                    Err((Some(user), error)) => {
                        _ = page.update(cx, |page, cx| {
                            page.service_account_created_with_credential_error(
                                user.display_name,
                                error,
                                cx,
                            );
                        });
                        form.update(cx, |form, cx| {
                            form.mark_saved(cx);
                            form.close(window, cx);
                        });
                        this.reset(window, cx);
                    }
                    Err((None, error)) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [&self.username, &self.display_name, &self.description] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.expiration.update(cx, |picker, cx| {
            picker.set_date(gpui_component::calendar::Date::Single(None), window, cx)
        });
        self.selected_role_ids.clear();
        self.initial_credential = InitialCredentialKind::None;
        self.form.update(cx, FormDialogState::reset_fields);
    }
}

impl Render for CreateServiceAccountDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let can_assign_roles = can_assign_initial_roles(cx);
        let can_create_credential = has_permission(cx, "service_accounts:credentials.write");
        let roles = self.roles.iter().map(|role| {
            let role_id = role.id;
            Checkbox::new(format!("default-service-account-role-{role_id}"))
                .with_size(component_size)
                .label(format!("{}（{}）", role.name, role.key))
                .checked(self.selected_role_ids.contains(&role_id))
                .disabled(self.saving || !can_assign_roles)
                .on_click(cx.listener(move |this, checked, _, cx| {
                    this.toggle_role(role_id, *checked, cx);
                }))
        });
        let status = v_flex()
            .gap_3()
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("create-service-account-error", error).title("创建失败"))
            })
            .when(self.saving, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child("正在创建服务账号与初始凭据…"),
                )
            });
        let role_section = v_flex()
            .gap_2()
            .child(div().text_sm().font_semibold().child("初始角色"))
            .child(div().text_xs().child("可选；不会自动附加 member 角色。"))
            .when(!can_assign_roles, |this| {
                this.child(Alert::info(
                    "service-account-roles-forbidden",
                    "当前账号不能选择初始角色。",
                ))
            })
            .children(roles);
        let initial_credential = self.initial_credential;
        let selector_dialog = cx.entity().downgrade();
        let credential_section = v_flex()
            .gap_2()
            .child(div().text_sm().font_semibold().child("初始凭据"))
            .child(
                RadioGroup::horizontal("service-account-initial-credential")
                    .selected_index(Some(match initial_credential {
                        InitialCredentialKind::ClientCredentials => 0,
                        InitialCredentialKind::PersonalAccessToken => 1,
                        InitialCredentialKind::None => 2,
                    }))
                    .disabled(self.saving || !can_create_credential)
                    .child(
                        Radio::new("initial-client-credentials")
                            .label("Client Credentials（推荐）"),
                    )
                    .child(
                        Radio::new("initial-personal-access-token").label("Personal Access Token"),
                    )
                    .child(Radio::new("initial-no-credential").label("暂不生成凭据"))
                    .on_click(move |index, _, cx| {
                        _ = selector_dialog.update(cx, |dialog, cx| {
                            dialog.initial_credential = match *index {
                                0 => InitialCredentialKind::ClientCredentials,
                                1 => InitialCredentialKind::PersonalAccessToken,
                                _ => InitialCredentialKind::None,
                            };
                            cx.notify();
                        });
                    }),
            )
            .when(!can_create_credential, |this| {
                this.child(Alert::info(
                    "service-account-credentials-forbidden",
                    "当前账号不能生成初始凭据；服务账号将以无凭据状态创建。",
                ))
            })
            .when(
                initial_credential == InitialCredentialKind::PersonalAccessToken,
                |this| {
                    this.child(
                        field()
                            .label("PAT 到期日期")
                            .description("留空表示永不过期；提交时会再次确认风险。")
                            .child(
                                DatePicker::new(&self.expiration)
                                    .cleanable(true)
                                    .disabled(self.saving || !can_create_credential),
                            ),
                    )
                    .when(
                        pat_expiration_is_high_risk(&self.expiration, cx),
                        |this| {
                            this.child(Alert::warning(
                                "service-account-pat-high-risk-expiration",
                                pat_expiration_risk_message_for_state(&self.expiration, cx),
                            ))
                        },
                    )
                },
            );
        let dialog = cx.entity().downgrade();
        FormDialog::new("create-service-account-dialog", self.form.clone())
            .title("创建服务账号")
            .description("创建 JWT machine user；可同时生成一份初始凭据。")
            .section(status)
            .child(
                field()
                    .label("稳定 username / Client ID")
                    .description("创建后不可修改。")
                    .required(true)
                    .child(Input::new(&self.username).disabled(self.saving)),
            )
            .child(
                field()
                    .label("展示名称")
                    .required(true)
                    .child(Input::new(&self.display_name).disabled(self.saving)),
            )
            .child(
                field()
                    .label("说明")
                    .description("可选；最多 500 个字符。")
                    .child(Input::new(&self.description).disabled(self.saving)),
            )
            .section(role_section)
            .section(credential_section)
            .submit_label("创建服务账号")
            .with_size(component_size)
            .on_submit(move |_, window, cx| {
                _ = dialog.update(cx, |dialog, cx| dialog.create(window, cx));
            })
    }
}

fn track_input(
    cx: &mut Context<CreateServiceAccountDialog>,
    form: &Entity<FormDialogState>,
    input: &Entity<InputState>,
    key: &'static str,
    label: &'static str,
) -> Subscription {
    let form = form.clone();
    cx.subscribe(input, move |_, input, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            form.update(cx, |form, cx| {
                form.set_field_draft(key, label, "", input.read(cx).value().to_string(), cx);
            });
        }
    })
}

fn text(input: &Entity<InputState>, cx: &gpui::App) -> String {
    input.read(cx).value().trim().to_owned()
}

fn optional_text(input: &Entity<InputState>, cx: &gpui::App) -> Option<String> {
    let value = text(input, cx);
    (!value.is_empty()).then_some(value)
}

fn pat_expiration_is_high_risk(expiration: &Entity<DatePickerState>, cx: &gpui::App) -> bool {
    expiration
        .read(cx)
        .date()
        .start()
        .is_none_or(|date| date > Utc::now().date_naive() + Duration::days(365))
}

fn pat_expiration_risk_message_for_state(
    expiration: &Entity<DatePickerState>,
    cx: &gpui::App,
) -> &'static str {
    if expiration.read(cx).date().start().is_none() {
        "永不过期 PAT 泄露后会持续有效，必须手动撤销。请确认仍要创建。"
    } else {
        "有效期超过一年属于超长期 PAT，泄露风险较高。请确认仍要创建。"
    }
}

fn can_assign_initial_roles(cx: &gpui::App) -> bool {
    has_permission(cx, "service_accounts:provision")
        && has_permission(cx, "users:roles.write")
        && has_permission(cx, "roles:read")
}

fn next_idempotency_key(service_account_id: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(
        "desktop-initial-{service_account_id}-{}-{timestamp}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
