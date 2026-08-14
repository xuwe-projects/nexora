//! 服务账号资料与凭据管理 FormDialog。

use chrono::{Duration, Utc};
use gpui::{
    App, Context, Entity, Render, Subscription, Task, WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::{
    Disableable as _, Sizable as _, StyledExt as _, WindowExt as _,
    alert::Alert,
    button::{Button, ButtonVariant, ButtonVariants as _},
    date_picker::{DatePicker, DatePickerEvent, DatePickerState},
    dialog::DialogButtonProps,
    form::field,
    h_flex,
    input::{Input, InputEvent, InputState},
    radio::{Radio, RadioGroup},
    spinner::Spinner,
    table::{Column, DataTable, TableState},
    tag::Tag,
    v_flex,
};
use ui::{CrudTableDelegate, FormDialog, FormDialogState, TableCell};

use contracts::patch::PatchField;

use crate::{
    defaults::account::has_permission,
    desktop::{
        api_session,
        contract::{
            CreateServiceAccountCredentialRequest, ServiceAccountCredentialResponse,
            ServiceAccountCredentialStatus, ServiceAccountCredentialType,
            UpdateServiceAccountRequest, UserResponse,
        },
    },
};

use super::{CredentialSecretDialog, UsersPage};

#[derive(Clone, Copy, PartialEq, Eq)]
enum CredentialKind {
    ClientCredentials,
    PersonalAccessToken,
}

#[derive(Clone, nexora::CrudTableRow)]
struct CredentialTableRow {
    #[nexora(row_id)]
    id: i64,
    #[nexora(column(key = "name", name = "凭据名称", width = 180., min_width = 140.))]
    name: String,
    #[nexora(column(key = "type", name = "类型", width = 150., min_width = 130.))]
    credential_type: String,
    #[nexora(column(
        key = "status",
        name = "状态",
        width = 90.,
        min_width = 80.,
        status,
        render = Self::render_status,
        text = Self::status_text
    ))]
    status: ServiceAccountCredentialStatus,
    #[nexora(column(key = "created_by", name = "创建人", width = 110., min_width = 96.))]
    created_by: String,
    #[nexora(column(key = "created_at", name = "创建时间", width = 170., min_width = 150.))]
    created_at: String,
    #[nexora(column(key = "expires_at", name = "到期时间", width = 170., min_width = 150.))]
    expires_at: String,
    #[nexora(column(key = "revoked", name = "撤销信息", width = 190., min_width = 150.))]
    revoked: String,
    #[nexora(column(
        key = "source",
        name = "来源",
        width = 110.,
        min_width = 96.,
        render = Self::render_source,
        text = Self::source_text
    ))]
    source_label: String,
    #[nexora(skip)]
    source: ServiceAccountCredentialResponse,
}

impl From<ServiceAccountCredentialResponse> for CredentialTableRow {
    fn from(source: ServiceAccountCredentialResponse) -> Self {
        let credential_type = match source.credential_type {
            ServiceAccountCredentialType::ClientCredentials => "Client Credentials",
            ServiceAccountCredentialType::PersonalAccessToken => "PAT",
            ServiceAccountCredentialType::Invalid => "未知凭据",
        }
        .to_owned();
        let revoked = match (&source.revoked_by, source.revoked_at) {
            (Some(user_id), Some(at)) => format!("{user_id} · {}", format_timestamp(at)),
            (_, Some(at)) => format!("Provider · {}", format_timestamp(at)),
            _ => "—".to_owned(),
        };
        let source_label = match source.source {
            crate::desktop::contract::ServiceAccountCredentialSource::Nexora => "Nexora",
            crate::desktop::contract::ServiceAccountCredentialSource::ProviderExternal => {
                "外部创建"
            }
        }
        .to_owned();
        Self {
            id: source.id,
            name: source.name.clone(),
            credential_type,
            status: source.status,
            created_by: source
                .created_by
                .clone()
                .unwrap_or_else(|| "Provider".to_owned()),
            created_at: format_timestamp(source.created_at),
            expires_at: source
                .expires_at
                .map(format_timestamp)
                .unwrap_or_else(|| "永不过期".to_owned()),
            revoked,
            source_label,
            source,
        }
    }
}

impl CredentialTableRow {
    fn status_text(row: &Self, _: &App) -> String {
        match row.status {
            ServiceAccountCredentialStatus::Active => "有效",
            ServiceAccountCredentialStatus::Revoked => "已撤销",
        }
        .to_owned()
    }

    fn render_status(row: &Self, _: &mut Window, _: &mut App) -> TableCell {
        let tag = match row.status {
            ServiceAccountCredentialStatus::Active => Tag::success().small().child("有效"),
            ServiceAccountCredentialStatus::Revoked => Tag::secondary().small().child("已撤销"),
        };
        TableCell::new(tag).center()
    }

    fn source_text(row: &Self, _: &App) -> String {
        row.source_label.clone()
    }

    fn render_source(row: &Self, _: &mut Window, _: &mut App) -> TableCell {
        let tag = if row.source_label == "外部创建" {
            Tag::info().small().child(row.source_label.clone())
        } else {
            Tag::secondary().small().child(row.source_label.clone())
        };
        TableCell::new(tag).center()
    }

    fn render_actions(
        row: &Self,
        manager: WeakEntity<ServiceAccountCredentials>,
        _: &mut Window,
        cx: &mut App,
    ) -> TableCell {
        let credential_id = row.id;
        let active = row.source.status == ServiceAccountCredentialStatus::Active;
        let can_write = has_permission(cx, "service_accounts:credentials.write");
        let busy = manager
            .upgrade()
            .is_some_and(|manager| manager.read(cx).busy);
        TableCell::new(
            Button::new(format!("revoke-service-credential-{credential_id}"))
                .danger()
                .label("撤销")
                .disabled(busy || !active || !can_write)
                .on_click(move |_, window, cx| {
                    _ = manager.update(cx, |manager, cx| {
                        manager.confirm_revoke(credential_id, window, cx);
                    });
                }),
        )
        .center()
    }
}

pub(in crate::defaults::account::users) struct ServiceAccountCredentials {
    page: WeakEntity<UsersPage>,
    credential_secret: WeakEntity<CredentialSecretDialog>,
    form: Entity<FormDialogState>,
    display_name: Entity<InputState>,
    description: Entity<InputState>,
    credential_name: Entity<InputState>,
    expiration: Entity<DatePickerState>,
    user: Option<UserResponse>,
    credentials: Vec<ServiceAccountCredentialResponse>,
    credential_table: Entity<TableState<CrudTableDelegate<CredentialTableRow>>>,
    credential_kind: CredentialKind,
    loading: bool,
    busy: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
    _task: Option<Task<()>>,
}

impl ServiceAccountCredentials {
    pub(in crate::defaults::account::users) fn new(
        page: WeakEntity<UsersPage>,
        credential_secret: WeakEntity<CredentialSecretDialog>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(FormDialogState::new);
        let display_name = cx.new(|cx| InputState::new(window, cx).placeholder("展示名称"));
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("可选用途说明"));
        let credential_name = cx.new(|cx| InputState::new(window, cx).placeholder("凭据管理名称"));
        let expiration = cx.new(|cx| DatePickerState::new(window, cx).date_format("%Y-%m-%d"));
        let manager = cx.entity().downgrade();
        let delegate = CrudTableDelegate::new(Vec::new())
            .empty_title("暂无凭据")
            .empty_description("创建 Client Secret 或 PAT 后会显示在这里")
            .action_column(
                Column::new("actions", "操作")
                    .width(px(86.))
                    .min_width(px(80.))
                    .max_width(px(96.))
                    .selectable(false),
                move |row: &CredentialTableRow, window, cx| {
                    CredentialTableRow::render_actions(row, manager.clone(), window, cx)
                },
            );
        let credential_table = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .sortable(false)
                .row_selectable(false)
                .col_selectable(false)
        });
        let display_form = form.clone();
        let description_form = form.clone();
        let subscriptions = vec![
            cx.subscribe(&display_name, move |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let original = this
                        .user
                        .as_ref()
                        .map(|user| user.display_name.clone())
                        .unwrap_or_default();
                    display_form.update(cx, |form, cx| {
                        form.set_field_draft(
                            "display_name",
                            "展示名称",
                            original,
                            input.read(cx).value().to_string(),
                            cx,
                        );
                    });
                }
            }),
            cx.subscribe(&description, move |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let original = this
                        .user
                        .as_ref()
                        .and_then(|user| user.description.clone())
                        .unwrap_or_default();
                    description_form.update(cx, |form, cx| {
                        form.set_field_draft(
                            "description",
                            "说明",
                            original,
                            input.read(cx).value().to_string(),
                            cx,
                        );
                    });
                }
            }),
            cx.subscribe(&expiration, |_, _, event: &DatePickerEvent, cx| {
                if matches!(event, DatePickerEvent::Change(_)) {
                    cx.notify();
                }
            }),
        ];
        Self {
            page,
            credential_secret,
            form,
            display_name,
            description,
            credential_name,
            expiration,
            user: None,
            credentials: Vec::new(),
            credential_table,
            credential_kind: CredentialKind::ClientCredentials,
            loading: false,
            busy: false,
            error: None,
            _subscriptions: subscriptions,
            _task: None,
        }
    }

    pub(super) fn open(&mut self, user: UserResponse, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.display_name.update(cx, |input, cx| {
            input.set_value(user.display_name.clone(), window, cx)
        });
        self.description.update(cx, |input, cx| {
            input.set_value(user.description.clone().unwrap_or_default(), window, cx)
        });
        self.credential_name
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.expiration.update(cx, |picker, cx| {
            picker.set_date(gpui_component::calendar::Date::Single(None), window, cx)
        });
        self.form.update(cx, |form, cx| {
            form.reset_fields(cx);
            form.set_field_draft(
                "display_name",
                "展示名称",
                user.display_name.clone(),
                user.display_name.clone(),
                cx,
            );
            let description = user.description.clone().unwrap_or_default();
            form.set_field_draft("description", "说明", description.clone(), description, cx);
            form.open(window, cx);
        });
        self.user = Some(user);
        self.credentials.clear();
        self.refresh_credential_table(cx);
        self.credential_kind = CredentialKind::ClientCredentials;
        self.error = None;
        self.load(cx);
    }

    pub(super) fn is_busy(&self) -> bool {
        self.busy
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if !has_permission(cx, "service_accounts:credentials.read") {
            self.loading = false;
            return;
        }
        let Some(user) = &self.user else { return };
        let Some(session) = api_session(cx) else {
            self.error = Some("当前登录会话不可用，请重新登录".to_owned());
            cx.notify();
            return;
        };
        self.loading = true;
        self.error = None;
        let user_id = user.id.clone();
        let background = cx.background_spawn(async move {
            session.list_service_account_credentials(user_id.as_str())
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(credentials) => {
                        this.credentials = credentials;
                        this.refresh_credential_table(cx);
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn refresh_credential_table(&self, cx: &mut Context<Self>) {
        let rows = self
            .credentials
            .iter()
            .cloned()
            .map(CredentialTableRow::from)
            .collect();
        self.credential_table.update(cx, |table, cx| {
            table.delegate_mut().replace_rows(rows);
            cx.notify();
        });
    }

    fn save_profile(&mut self, cx: &mut Context<Self>) {
        if self.busy || !has_permission(cx, "service_accounts:profile.write") {
            return;
        }
        let Some(user) = &self.user else { return };
        let Some(session) = api_session(cx) else {
            return;
        };
        let display_name = input_text(&self.display_name, cx);
        if display_name.is_empty() {
            self.error = Some("展示名称不能为空".to_owned());
            cx.notify();
            return;
        }
        let description = input_text(&self.description, cx);
        let request = UpdateServiceAccountRequest {
            username: None,
            display_name: Some(display_name),
            description: if description.is_empty() {
                PatchField::Null
            } else {
                PatchField::Value(description)
            },
        };
        let user_id = user.id.clone();
        self.busy = true;
        self.error = None;
        let page = self.page.clone();
        let form = self.form.clone();
        let background = cx.background_spawn(async move {
            session.update_service_account(user_id.as_str(), &request)
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(user) => {
                        let display_name = user.display_name.clone();
                        this.user = Some(user);
                        form.update(cx, |form, cx| form.mark_saved(cx));
                        _ = page.update(cx, |page, cx| {
                            page.service_account_updated(display_name, cx);
                        });
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn confirm_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !has_permission(cx, "service_accounts:credentials.write") {
            return;
        }
        if input_text(&self.credential_name, cx).is_empty() {
            self.error = Some("凭据名称不能为空".to_owned());
            cx.notify();
            return;
        }
        let manager = cx.entity().downgrade();
        let (title, description, action) = match self.credential_kind {
            CredentialKind::ClientCredentials => (
                "生成或轮换 Client Secret",
                "新 Secret 只显示一次；已有 Secret 会立即失效。请确认设备切换计划。",
                "生成并轮换",
            ),
            CredentialKind::PersonalAccessToken
                if pat_expiration_is_high_risk(&self.expiration, cx) =>
            {
                (
                    "创建高风险 PAT",
                    pat_expiration_risk_message(&self.expiration, cx),
                    "确认创建",
                )
            }
            CredentialKind::PersonalAccessToken => (
                "创建 Personal Access Token",
                "PAT 只显示一次，请在关闭结果前复制并安全保存。",
                "确认创建",
            ),
        };
        window.open_alert_dialog(cx, move |alert, _, _| {
            let manager = manager.clone();
            alert
                .title(title)
                .description(description)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(action)
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    _ = manager.update(cx, |manager, cx| {
                        manager.create_credential(window, cx);
                    });
                    true
                })
        });
    }

    fn create_credential(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(user), Some(session)) = (&self.user, api_session(cx)) else {
            return;
        };
        let name = input_text(&self.credential_name, cx);
        let expires_at = self
            .expiration
            .read(cx)
            .date()
            .start()
            .and_then(|date| date.and_hms_opt(23, 59, 59))
            .map(|date| date.and_utc().timestamp());
        let credential_type = match self.credential_kind {
            CredentialKind::ClientCredentials => ServiceAccountCredentialType::ClientCredentials,
            CredentialKind::PersonalAccessToken => {
                ServiceAccountCredentialType::PersonalAccessToken
            }
        };
        let request = CreateServiceAccountCredentialRequest {
            credential_type,
            name,
            expires_at: (credential_type == ServiceAccountCredentialType::PersonalAccessToken)
                .then_some(expires_at)
                .flatten(),
        };
        let user_id = user.id.clone();
        let idempotency_key = next_idempotency_key(user_id.as_str());
        self.busy = true;
        self.error = None;
        let credential_secret = self.credential_secret.clone();
        let background = cx.background_spawn(async move {
            session.create_service_account_credential(
                user_id.as_str(),
                idempotency_key.as_str(),
                &request,
            )
        });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = background.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(created) => {
                        this.credentials.insert(0, created.credential.clone());
                        this.refresh_credential_table(cx);
                        _ = credential_secret.update(cx, |dialog, cx| {
                            dialog.open(created, window, cx);
                        });
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn confirm_revoke(&mut self, credential_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let manager = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let manager = manager.clone();
            alert
                .title("撤销服务账号凭据")
                .description("撤销立即生效，且无法恢复；同账号其他凭据不受影响。")
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("撤销凭据")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    _ = manager.update(cx, |manager, cx| manager.revoke(credential_id, cx));
                    true
                })
        });
    }

    fn revoke(&mut self, credential_id: i64, cx: &mut Context<Self>) {
        let (Some(user), Some(session)) = (&self.user, api_session(cx)) else {
            return;
        };
        let user_id = user.id.clone();
        self.busy = true;
        self.error = None;
        let background = cx.background_spawn(async move {
            session.revoke_service_account_credential(user_id.as_str(), credential_id)
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        if let Some(credential) = this
                            .credentials
                            .iter_mut()
                            .find(|credential| credential.id == credential_id)
                        {
                            credential.status = ServiceAccountCredentialStatus::Revoked;
                        }
                        this.refresh_credential_table(cx);
                    }
                    Err(error) => this.error = Some(error.user_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.form.update(cx, |form, cx| form.close(window, cx));
        cx.notify();
    }
}

impl Render for ServiceAccountCredentials {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_size = theme::component_size(cx);
        let can_profile = has_permission(cx, "service_accounts:profile.write");
        let can_read = has_permission(cx, "service_accounts:credentials.read");
        let can_write = has_permission(cx, "service_accounts:credentials.write");
        let profile = v_flex()
            .gap_3()
            .child(div().font_semibold().child("服务账号资料"))
            .child(
                field()
                    .label("展示名称")
                    .required(true)
                    .child(Input::new(&self.display_name).disabled(self.busy || !can_profile)),
            )
            .child(
                field()
                    .label("说明")
                    .child(Input::new(&self.description).disabled(self.busy || !can_profile)),
            )
            .child(
                h_flex().justify_end().child(
                    Button::new("save-service-account-profile")
                        .with_size(component_size)
                        .outline()
                        .label("保存资料")
                        .loading(self.busy)
                        .disabled(self.busy || !can_profile)
                        .on_click(cx.listener(|this, _, _, cx| this.save_profile(cx))),
                ),
            );
        let kind = self.credential_kind;
        let manager = cx.entity().downgrade();
        let create = v_flex()
            .gap_3()
            .child(div().font_semibold().child("创建凭据"))
            .child(
                RadioGroup::horizontal("service-account-credential-kind")
                    .selected_index(Some(match kind {
                        CredentialKind::ClientCredentials => 0,
                        CredentialKind::PersonalAccessToken => 1,
                    }))
                    .disabled(self.busy || !can_write)
                    .child(Radio::new("client-credentials").label("Client Credentials（推荐）"))
                    .child(Radio::new("personal-access-token").label("Personal Access Token"))
                    .on_click(move |index, _, cx| {
                        _ = manager.update(cx, |manager, cx| {
                            manager.credential_kind = if *index == 0 {
                                CredentialKind::ClientCredentials
                            } else {
                                CredentialKind::PersonalAccessToken
                            };
                            cx.notify();
                        });
                    }),
            )
            .child(
                field()
                    .label("凭据名称")
                    .required(true)
                    .child(Input::new(&self.credential_name).disabled(self.busy || !can_write)),
            )
            .when(kind == CredentialKind::PersonalAccessToken, |this| {
                this.child(
                    field()
                        .label("PAT 到期日期")
                        .description("留空表示永不过期，将在创建前再次确认风险。")
                        .child(
                            DatePicker::new(&self.expiration)
                                .cleanable(true)
                                .disabled(self.busy || !can_write),
                        ),
                )
            })
            .child(
                h_flex().justify_end().child(
                    Button::new("create-service-account-credential")
                        .with_size(component_size)
                        .primary()
                        .label(match kind {
                            CredentialKind::ClientCredentials => "生成或轮换 Secret",
                            CredentialKind::PersonalAccessToken => "创建 PAT",
                        })
                        .loading(self.busy)
                        .disabled(self.busy || !can_write)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.confirm_create(window, cx);
                        })),
                ),
            );
        let content = v_flex()
            .gap_4()
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("service-account-credentials-error", error))
            })
            .when(self.loading, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child("正在协调 Provider 凭据状态…"),
                )
            })
            .child(profile)
            .when(!can_read, |this| {
                this.child(Alert::info(
                    "service-account-credentials-read-forbidden",
                    "当前账号只能编辑资料，不能查看凭据元数据。",
                ))
            })
            .child(create)
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(div().font_semibold().child("凭据列表"))
                    .child(
                        Button::new("refresh-service-account-credentials")
                            .with_size(component_size)
                            .outline()
                            .label("刷新 Provider 状态")
                            .loading(self.loading)
                            .disabled(self.loading || self.busy || !can_read)
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .h(px(300.))
                    .child(DataTable::new(&self.credential_table).with_size(component_size)),
            );
        let manager = cx.entity().downgrade();
        FormDialog::new("service-account-credentials-dialog", self.form.clone())
            .title(
                self.user
                    .as_ref()
                    .map(|user| format!("管理 {}", user.display_name))
                    .unwrap_or_else(|| "管理服务账号".to_owned()),
            )
            .description("修改资料、协调凭据状态，并管理一次性 Secret/PAT。")
            .section(content)
            .submit_label("关闭")
            .submit_disabled(self.busy)
            .with_size(component_size)
            .on_submit(move |_, window, cx| {
                _ = manager.update(cx, |manager, cx| manager.close(window, cx));
            })
    }
}

fn format_timestamp(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|value| value.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| timestamp.to_string())
}

fn input_text(input: &Entity<InputState>, cx: &gpui::App) -> String {
    input.read(cx).value().trim().to_owned()
}

fn next_idempotency_key(service_account_id: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(
        "desktop-{service_account_id}-{}-{timestamp}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn pat_expiration_is_high_risk(expiration: &Entity<DatePickerState>, cx: &App) -> bool {
    expiration
        .read(cx)
        .date()
        .start()
        .is_none_or(|date| date > Utc::now().date_naive() + Duration::days(365))
}

fn pat_expiration_risk_message(expiration: &Entity<DatePickerState>, cx: &App) -> &'static str {
    if expiration.read(cx).date().start().is_none() {
        "永不过期 PAT 风险较高，泄露后会持续有效直至手动撤销。"
    } else {
        "有效期超过一年属于超长期 PAT，泄露风险较高。请确认仍要创建。"
    }
}
