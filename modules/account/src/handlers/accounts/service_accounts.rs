//! 服务账号资料与凭据管理 handlers。

use api::{ApiJson, ApiPath};
use axum::{
    Json,
    extract::{FromRequestParts, State},
    http::{StatusCode, header::LOCATION, request::Parts},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use contracts::{
    account::{
        CreateServiceAccountCredentialRequest, CreateServiceAccountCredentialResponse,
        CreateServiceAccountRequest, ServiceAccountCredentialResponse,
        ServiceAccountCredentialSecret, ServiceAccountCredentialType as ApiCredentialType,
        UpdateServiceAccountRequest, UserResponse,
    },
    patch::PatchField,
};
use kernel::ValidationError;

use crate::{
    Account, AccountError, AccountState, ApiError, CreateServiceAccountIdentity,
    ServiceAccountCredentialType, UserType,
    authorization::{
        Authorized, RequiredPermission,
        accounts::{
            ProvisionServiceAccounts, ReadServiceAccountCredentials,
            WriteServiceAccountCredentials, WriteServiceAccountProfiles, WriteUserRoles,
        },
    },
    handlers::accounts::{service_account_credential_response, user_response},
};

pub(crate) struct IdempotencyKey(Option<String>);

impl<S> FromRequestParts<S> for IdempotencyKey
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let value = parts
            .headers
            .get("idempotency-key")
            .map(|value| {
                value.to_str().map(str::to_owned).map_err(|_| {
                    ApiError::from(AccountError::InvalidInput(ValidationError::new(
                        "idempotency_key",
                        "Idempotency-Key 必须是有效的 HTTP 文本",
                    )))
                })
            })
            .transpose()?;
        Ok(Self(value))
    }
}

pub(crate) async fn create_service_account(
    authorization: Authorized<ProvisionServiceAccounts>,
    State(state): State<AccountState>,
    ApiJson(request): ApiJson<CreateServiceAccountRequest>,
) -> Result<Response, ApiError> {
    let roles_write = <WriteUserRoles as RequiredPermission>::KEY;
    if !request.role_ids.is_empty() && !authorization.profile().allows(roles_write.clone()) {
        return Err(AccountError::Forbidden(roles_write).into());
    }
    let granted_by = authorization.profile().user.id.clone();
    let user = Account { state }
        .create_service_account(
            CreateServiceAccountIdentity {
                username: request.username,
                display_name: request.display_name,
                description: request.description,
            },
            request.role_ids.as_slice(),
            granted_by.as_str(),
        )
        .await?;
    let location = format!("/users/{}", user.id);
    Ok((
        StatusCode::CREATED,
        [(LOCATION, location)],
        Json(user_response(user)),
    )
        .into_response())
}

pub(crate) async fn update_service_account(
    _authorization: Authorized<WriteServiceAccountProfiles>,
    State(state): State<AccountState>,
    ApiPath(service_account_id): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateServiceAccountRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    if request.username.is_some() {
        return Err(AccountError::Conflict {
            code: "service_account_identifier_immutable",
            message: "服务账号 username 创建后不可修改",
        }
        .into());
    }
    if request.display_name.is_none() && request.description == PatchField::Missing {
        return Err(AccountError::InvalidInput(ValidationError::new(
            "body",
            "至少需要提供一个要修改的服务账号字段",
        ))
        .into());
    }
    let account = Account { state };
    let current = account.user_access(service_account_id.as_str()).await?.user;
    if current.user_type != UserType::ServiceAccount {
        return Err(AccountError::Conflict {
            code: "service_account_required",
            message: "该操作只允许用于服务账号",
        }
        .into());
    }
    let display_name = request
        .display_name
        .as_deref()
        .unwrap_or(current.display_name.as_str());
    let description = match &request.description {
        PatchField::Missing => current.description.as_deref(),
        PatchField::Null => None,
        PatchField::Value(value) => Some(value.as_str()),
    };
    let user = account
        .update_service_account_profile(service_account_id.as_str(), display_name, description)
        .await?;
    Ok(Json(user_response(user)))
}

pub(crate) async fn list_credentials(
    _authorization: Authorized<ReadServiceAccountCredentials>,
    State(state): State<AccountState>,
    ApiPath(service_account_id): ApiPath<String>,
) -> Result<Json<Vec<ServiceAccountCredentialResponse>>, ApiError> {
    let credentials = Account { state }
        .service_account_credentials(service_account_id.as_str())
        .await?;
    Ok(Json(
        credentials
            .into_iter()
            .map(service_account_credential_response)
            .collect(),
    ))
}

pub(crate) async fn create_credential(
    authorization: Authorized<WriteServiceAccountCredentials>,
    State(state): State<AccountState>,
    ApiPath(service_account_id): ApiPath<String>,
    IdempotencyKey(idempotency_key): IdempotencyKey,
    ApiJson(request): ApiJson<CreateServiceAccountCredentialRequest>,
) -> Result<Response, ApiError> {
    let expires_at = request
        .expires_at
        .map(|value| {
            DateTime::<Utc>::from_timestamp(value, 0).ok_or_else(|| {
                AccountError::InvalidInput(ValidationError::new(
                    "expires_at",
                    "到期时间必须是有效的 Unix 秒时间戳",
                ))
            })
        })
        .transpose()?;
    let credential_type = match request.credential_type {
        ApiCredentialType::ClientCredentials => ServiceAccountCredentialType::ClientCredentials,
        ApiCredentialType::PersonalAccessToken => ServiceAccountCredentialType::PersonalAccessToken,
        ApiCredentialType::Invalid => {
            return Err(AccountError::InvalidInput(ValidationError::new(
                "credential_type",
                "不支持该服务账号凭据类型",
            ))
            .into());
        }
    };
    let created = Account { state }
        .create_service_account_credential(
            service_account_id.as_str(),
            credential_type,
            request.name.as_str(),
            expires_at,
            idempotency_key.as_deref(),
            authorization.profile().user.id.as_str(),
        )
        .await?;
    let credential_id = created.credential.id;
    let secret = match created.client_id {
        Some(client_id) => ServiceAccountCredentialSecret::ClientCredentials {
            client_id,
            client_secret: created.secret,
        },
        None => ServiceAccountCredentialSecret::PersonalAccessToken {
            token: created.secret,
        },
    };
    let response = CreateServiceAccountCredentialResponse {
        credential: service_account_credential_response(created.credential),
        secret,
    };
    let location = format!("/service-accounts/{service_account_id}/credentials/{credential_id}");
    Ok((StatusCode::CREATED, [(LOCATION, location)], Json(response)).into_response())
}

pub(crate) async fn revoke_credential(
    authorization: Authorized<WriteServiceAccountCredentials>,
    State(state): State<AccountState>,
    ApiPath((service_account_id, credential_id)): ApiPath<(String, i64)>,
) -> Result<StatusCode, ApiError> {
    Account { state }
        .revoke_service_account_credential(
            service_account_id.as_str(),
            credential_id,
            authorization.profile().user.id.as_str(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
