//! 认证授权身份同步查询。

use rand::{TryRngCore as _, rngs::OsRng};
use sqlx::{PgPool, Postgres, Transaction};

use crate::{
    StoreError,
    entities::account::{ExternalIdentity, User},
};

const USER_ID_LENGTH: usize = 8;
const USER_ID_GENERATION_ATTEMPTS: usize = 16;
const USER_ID_ALPHABET: &[u8; 62] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const UNBIASED_BYTE_LIMIT: u8 = 248;

/// 更新认证授权身份对应的已有本地用户；不存在时不创建任何记录。
pub(crate) async fn sync_existing(
    identity: &ExternalIdentity,
    pool: &PgPool,
) -> Result<Option<User>, StoreError> {
    Ok(sqlx::query_as::<_, User>(
        r#"
        UPDATE account.users
        SET email = COALESCE($2, email),
            display_name = $3,
            avatar_url = COALESCE($4, avatar_url),
            updated_at = NOW(),
            last_login_at = NOW()
        WHERE identity_id = $1
        RETURNING id, identity_id, email, display_name, avatar_url, status,
                  is_super_admin, created_at, updated_at, last_login_at
        "#,
    )
    .bind(identity.identity_id.as_str())
    .bind(identity.email.as_deref())
    .bind(identity.display_name.as_str())
    .bind(identity.avatar_url.as_deref())
    .fetch_optional(pool)
    .await?)
}

pub(super) async fn upsert(
    identity: &ExternalIdentity,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<User, sqlx::Error> {
    if let Some(user) = update_existing(identity, transaction).await? {
        return Ok(user);
    }

    for _ in 0..USER_ID_GENERATION_ATTEMPTS {
        let user_id = generate_user_id()?;
        let inserted = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO account.users (id, identity_id, email, display_name, avatar_url)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT DO NOTHING
            RETURNING id, identity_id, email, display_name, avatar_url, status,
                      is_super_admin, created_at, updated_at, last_login_at
            "#,
        )
        .bind(user_id)
        .bind(identity.identity_id.as_str())
        .bind(identity.email.as_deref())
        .bind(identity.display_name.as_str())
        .bind(identity.avatar_url.as_deref())
        .fetch_optional(&mut **transaction)
        .await?;
        if let Some(user) = inserted {
            return Ok(user);
        }
        if let Some(user) = update_existing(identity, transaction).await? {
            return Ok(user);
        }
    }

    Err(sqlx::Error::Protocol(
        "无法在限定次数内生成唯一的 8 位用户 ID".to_owned(),
    ))
}

async fn update_existing(
    identity: &ExternalIdentity,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"
        UPDATE account.users
        SET email = COALESCE($2, email),
            display_name = $3,
            avatar_url = COALESCE($4, avatar_url),
            updated_at = NOW(),
            last_login_at = NOW()
        WHERE identity_id = $1
        RETURNING id, identity_id, email, display_name, avatar_url, status,
                  is_super_admin, created_at, updated_at, last_login_at
        "#,
    )
    .bind(identity.identity_id.as_str())
    .bind(identity.email.as_deref())
    .bind(identity.display_name.as_str())
    .bind(identity.avatar_url.as_deref())
    .fetch_optional(&mut **transaction)
    .await
}

fn generate_user_id() -> Result<String, sqlx::Error> {
    let mut identifier = String::with_capacity(USER_ID_LENGTH);
    let mut random_bytes = [0_u8; USER_ID_LENGTH * 2];
    while identifier.len() < USER_ID_LENGTH {
        OsRng
            .try_fill_bytes(&mut random_bytes)
            .map_err(|error| sqlx::Error::Protocol(format!("操作系统随机源不可用: {error}")))?;
        for byte in random_bytes {
            if byte < UNBIASED_BYTE_LIMIT {
                identifier.push(USER_ID_ALPHABET[usize::from(byte % 62)] as char);
                if identifier.len() == USER_ID_LENGTH {
                    break;
                }
            }
        }
    }
    Ok(identifier)
}
