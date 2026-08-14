//! 服务账号与凭据非敏感元数据的数据访问函数。

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use crate::{
    ProviderServiceAccountCredentials, ServiceAccountIdentity, StoreError,
    entities::account::{
        ServiceAccountCredential, ServiceAccountCredentialSource, ServiceAccountCredentialType,
        User,
    },
};

const USER_ID_GENERATION_ATTEMPTS: usize = 16;

pub(crate) async fn provision(
    identity: &ServiceAccountIdentity,
    role_ids: &[i64],
    granted_by: &str,
    pool: &PgPool,
) -> Result<User, StoreError> {
    let mut transaction = pool.begin().await?;
    ensure_operator(granted_by, &mut transaction).await?;
    if service_account_identity_exists(
        identity.identity_id.as_str(),
        identity.username.as_str(),
        &mut transaction,
    )
    .await?
    {
        return Err(StoreError::Conflict("service_account_already_exists"));
    }

    for _ in 0..USER_ID_GENERATION_ATTEMPTS {
        let user_id = super::identities::generate_user_id()?;
        let inserted = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO account.users (
                id, identity_id, username, email, display_name, user_type, description
            )
            VALUES ($1, $2, $3, NULL, $4, 'service_account', $5)
            ON CONFLICT DO NOTHING
            RETURNING id, identity_id, username, email, display_name, status, user_type,
                      description, is_super_admin, created_at, updated_at, last_login_at
            "#,
        )
        .bind(user_id)
        .bind(identity.identity_id.as_str())
        .bind(identity.username.as_str())
        .bind(identity.display_name.as_str())
        .bind(identity.description.as_deref())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(user) = inserted {
            super::users::grant_initial_service_account_roles(
                user.id.as_str(),
                role_ids,
                granted_by,
                &mut transaction,
            )
            .await?;
            transaction.commit().await?;
            return Ok(user);
        }
        if service_account_identity_exists(
            identity.identity_id.as_str(),
            identity.username.as_str(),
            &mut transaction,
        )
        .await?
        {
            return Err(StoreError::Conflict("service_account_already_exists"));
        }
    }
    Err(StoreError::Database(sqlx::Error::Protocol(
        "无法在限定次数内生成唯一的 8 位服务账号 ID".to_owned(),
    )))
}

pub(crate) async fn update_profile(
    user_id: &str,
    display_name: &str,
    description: Option<&str>,
    pool: &PgPool,
) -> Result<User, StoreError> {
    sqlx::query_as::<_, User>(
        r#"
        UPDATE account.users
        SET display_name = $2, description = $3, updated_at = NOW()
        WHERE id = $1 AND user_type = 'service_account'
        RETURNING id, identity_id, username, email, display_name, status, user_type,
                  description, is_super_admin, created_at, updated_at, last_login_at
        "#,
    )
    .bind(user_id)
    .bind(display_name)
    .bind(description)
    .fetch_optional(pool)
    .await?
    .ok_or(StoreError::NotFound("服务账号"))
}

pub(crate) async fn lock(
    service_account_id: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<User, StoreError> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT id, identity_id, username, email, display_name, status, user_type, description,
               is_super_admin, created_at, updated_at, last_login_at
        FROM account.users
        WHERE id = $1 AND user_type = 'service_account'
        FOR UPDATE
        "#,
    )
    .bind(service_account_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StoreError::NotFound("服务账号"))
}

pub(crate) async fn idempotency_key_exists(
    service_account_id: &str,
    idempotency_key: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM account.service_account_credentials
            WHERE service_account_id = $1 AND idempotency_key = $2
        )
        "#,
    )
    .bind(service_account_id)
    .bind(idempotency_key)
    .fetch_one(&mut **transaction)
    .await
}

pub(crate) async fn try_lock_client_secret_rotation(
    service_account_id: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(service_account_id)
        .fetch_one(&mut **transaction)
        .await
}

pub(crate) async fn insert_client_secret(
    service_account_id: &str,
    name: &str,
    created_by: &str,
    created_at: DateTime<Utc>,
    idempotency_key: Option<&str>,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<ServiceAccountCredential, StoreError> {
    sqlx::query(
        r#"
        UPDATE account.service_account_credentials
        SET status = 'revoked', revoked_at = NOW(), revoked_by = $2,
            last_synchronized_at = NOW()
        WHERE service_account_id = $1
          AND credential_type = 'client_credentials'
          AND status = 'active'
        "#,
    )
    .bind(service_account_id)
    .bind(created_by)
    .execute(&mut **transaction)
    .await?;
    insert_credential(
        service_account_id,
        ServiceAccountCredentialType::ClientCredentials,
        name,
        None,
        Some(created_by),
        created_at,
        None,
        ServiceAccountCredentialSource::Nexora,
        idempotency_key,
        transaction,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_personal_access_token(
    service_account_id: &str,
    name: &str,
    provider_credential_id: &str,
    created_by: &str,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    idempotency_key: Option<&str>,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<ServiceAccountCredential, StoreError> {
    insert_credential(
        service_account_id,
        ServiceAccountCredentialType::PersonalAccessToken,
        name,
        Some(provider_credential_id),
        Some(created_by),
        created_at,
        expires_at,
        ServiceAccountCredentialSource::Nexora,
        idempotency_key,
        transaction,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_credential(
    service_account_id: &str,
    credential_type: ServiceAccountCredentialType,
    name: &str,
    provider_credential_id: Option<&str>,
    created_by: Option<&str>,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    source: ServiceAccountCredentialSource,
    idempotency_key: Option<&str>,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<ServiceAccountCredential, StoreError> {
    Ok(sqlx::query_as::<_, ServiceAccountCredential>(
        r#"
        INSERT INTO account.service_account_credentials (
            service_account_id, credential_type, name, provider_credential_id, created_by,
            created_at, expires_at, source, idempotency_key
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, service_account_id, credential_type, name, provider_credential_id,
                  created_by, created_at, expires_at, status, source, revoked_by, revoked_at,
                  last_synchronized_at
        "#,
    )
    .bind(service_account_id)
    .bind(credential_type)
    .bind(name)
    .bind(provider_credential_id)
    .bind(created_by)
    .bind(created_at)
    .bind(expires_at)
    .bind(source)
    .bind(idempotency_key)
    .fetch_one(&mut **transaction)
    .await?)
}

pub(crate) async fn lock_credential(
    service_account_id: &str,
    credential_id: i64,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<ServiceAccountCredential, StoreError> {
    sqlx::query_as::<_, ServiceAccountCredential>(
        r#"
        SELECT id, service_account_id, credential_type, name, provider_credential_id,
               created_by, created_at, expires_at, status, source, revoked_by, revoked_at,
               last_synchronized_at
        FROM account.service_account_credentials
        WHERE service_account_id = $1 AND id = $2
        FOR UPDATE
        "#,
    )
    .bind(service_account_id)
    .bind(credential_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StoreError::NotFound("服务账号凭据"))
}

pub(crate) async fn revoke(
    credential_id: i64,
    revoked_by: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<ServiceAccountCredential, StoreError> {
    sqlx::query_as::<_, ServiceAccountCredential>(
        r#"
        UPDATE account.service_account_credentials
        SET status = 'revoked', revoked_by = $2, revoked_at = NOW(),
            last_synchronized_at = NOW()
        WHERE id = $1 AND status = 'active'
        RETURNING id, service_account_id, credential_type, name, provider_credential_id,
                  created_by, created_at, expires_at, status, source, revoked_by, revoked_at,
                  last_synchronized_at
        "#,
    )
    .bind(credential_id)
    .bind(revoked_by)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StoreError::Conflict("credential_already_revoked"))
}

pub(crate) async fn reconcile(
    service_account_id: &str,
    snapshot: &ProviderServiceAccountCredentials,
    pool: &PgPool,
) -> Result<Vec<ServiceAccountCredential>, StoreError> {
    let mut transaction = pool.begin().await?;
    lock(service_account_id, &mut transaction).await?;
    if snapshot.has_client_secret {
        let has_local = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM account.service_account_credentials
                WHERE service_account_id = $1
                  AND credential_type = 'client_credentials' AND status = 'active'
            )
            "#,
        )
        .bind(service_account_id)
        .fetch_one(&mut *transaction)
        .await?;
        if !has_local {
            insert_credential(
                service_account_id,
                ServiceAccountCredentialType::ClientCredentials,
                "Provider Client Secret",
                None,
                None,
                Utc::now(),
                None,
                ServiceAccountCredentialSource::ProviderExternal,
                None,
                &mut transaction,
            )
            .await?;
        }
    } else {
        revoke_missing_client_secret(service_account_id, &mut transaction).await?;
    }

    let provider_token_ids = snapshot
        .personal_access_tokens
        .iter()
        .map(|token| token.token_id.as_str())
        .collect::<Vec<_>>();
    for token in &snapshot.personal_access_tokens {
        sqlx::query(
            r#"
            INSERT INTO account.service_account_credentials (
                service_account_id, credential_type, name, provider_credential_id, created_at,
                expires_at, source
            )
            VALUES ($1, 'personal_access_token', 'Provider PAT', $2, $3, $4,
                    'provider_external')
            ON CONFLICT (service_account_id, credential_type, provider_credential_id)
                WHERE provider_credential_id IS NOT NULL
            DO UPDATE SET expires_at = EXCLUDED.expires_at, status = 'active', revoked_by = NULL,
                          revoked_at = NULL, last_synchronized_at = NOW()
            "#,
        )
        .bind(service_account_id)
        .bind(token.token_id.as_str())
        .bind(token.created_at)
        .bind(token.expires_at)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        r#"
        UPDATE account.service_account_credentials
        SET status = 'revoked', revoked_at = NOW(), revoked_by = NULL,
            last_synchronized_at = NOW()
        WHERE service_account_id = $1
          AND credential_type = 'personal_access_token'
          AND status = 'active'
          AND NOT (provider_credential_id = ANY($2::text[]))
        "#,
    )
    .bind(service_account_id)
    .bind(provider_token_ids)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    query(service_account_id, pool).await.map_err(Into::into)
}

pub(crate) async fn query(
    service_account_id: &str,
    pool: &PgPool,
) -> Result<Vec<ServiceAccountCredential>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, service_account_id, credential_type, name, provider_credential_id,
               created_by, created_at, expires_at, status, source, revoked_by, revoked_at,
               last_synchronized_at
        FROM account.service_account_credentials
        WHERE service_account_id = $1
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(service_account_id)
    .fetch_all(pool)
    .await
}

async fn revoke_missing_client_secret(
    service_account_id: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE account.service_account_credentials
        SET status = 'revoked', revoked_at = NOW(), revoked_by = NULL,
            last_synchronized_at = NOW()
        WHERE service_account_id = $1
          AND credential_type = 'client_credentials' AND status = 'active'
        "#,
    )
    .bind(service_account_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn service_account_identity_exists(
    identity_id: &str,
    username: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM account.users
            WHERE identity_id = $1 OR LOWER(username) = LOWER($2)
        )
        "#,
    )
    .bind(identity_id)
    .bind(username)
    .fetch_one(&mut **transaction)
    .await
}

async fn ensure_operator(
    user_id: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StoreError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM account.users WHERE id = $1)")
            .bind(user_id)
            .fetch_one(&mut **transaction)
            .await?;
    if exists {
        Ok(())
    } else {
        Err(StoreError::NotFound("授权人"))
    }
}
