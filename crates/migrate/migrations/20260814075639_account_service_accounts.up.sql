CREATE TYPE account.user_type AS ENUM (
    'human',
    'service_account'
);

COMMENT ON TYPE account.user_type IS
    '账号主体类型：human=真人用户，service_account=设备、集成或服务间调用使用的非人类身份';

ALTER TABLE account.users
    ADD COLUMN user_type account.user_type,
    ADD COLUMN description TEXT,
    ADD CONSTRAINT users_description_length
        CHECK (description IS NULL OR CHAR_LENGTH(description) <= 500);

UPDATE account.users
SET user_type = CASE
    WHEN is_super_admin THEN 'human'::account.user_type
    WHEN username IS NULL AND email IS NULL THEN 'service_account'::account.user_type
    ELSE 'human'::account.user_type
END;

ALTER TABLE account.users
    ALTER COLUMN user_type SET DEFAULT 'human',
    ALTER COLUMN user_type SET NOT NULL;

COMMENT ON COLUMN account.users.user_type IS
    '显式账号主体类型；历史超级管理员固定回填为 human，用户名和邮箱均为空的普通历史账号回填为 service_account';
COMMENT ON COLUMN account.users.description IS
    '服务账号或人员账号的可选管理说明，不参与认证和授权判断';
COMMENT ON CONSTRAINT users_description_length ON account.users IS
    '限制可选账号说明长度不超过 ZITADEL machine description 支持的 500 个字符';

CREATE INDEX users_user_type_created_at_id_idx
    ON account.users (user_type, created_at DESC, id DESC);
COMMENT ON INDEX account.users_user_type_created_at_id_idx IS
    '支持按显式账号主体类型和创建时间稳定分页查询用户';

CREATE UNIQUE INDEX users_service_account_username_unique_idx
    ON account.users (LOWER(username))
    WHERE user_type = 'service_account' AND username IS NOT NULL;
COMMENT ON INDEX account.users_service_account_username_unique_idx IS
    '保证新建服务账号的稳定 username 在本地按大小写不敏感方式唯一，同时兼容历史无 username 服务账号';

CREATE TYPE account.service_account_credential_type AS ENUM (
    'client_credentials',
    'personal_access_token'
);

COMMENT ON TYPE account.service_account_credential_type IS
    '服务账号凭据类型：client_credentials=OAuth Client Secret，personal_access_token=个人访问令牌';

CREATE TYPE account.service_account_credential_status AS ENUM (
    'active',
    'revoked'
);

COMMENT ON TYPE account.service_account_credential_status IS
    '服务账号凭据协调后的当前状态：active=Provider 中有效，revoked=已经撤销或不再存在';

CREATE TYPE account.service_account_credential_source AS ENUM (
    'nexora',
    'provider_external'
);

COMMENT ON TYPE account.service_account_credential_source IS
    '服务账号凭据元数据来源：nexora=由本系统创建，provider_external=在身份 Provider 外部创建后协调发现';

CREATE TABLE account.service_account_credentials (
    id BIGSERIAL NOT NULL,
    service_account_id VARCHAR(8) NOT NULL,
    credential_type account.service_account_credential_type NOT NULL,
    name TEXT NOT NULL,
    provider_credential_id TEXT,
    created_by VARCHAR(8),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    status account.service_account_credential_status NOT NULL DEFAULT 'active',
    source account.service_account_credential_source NOT NULL DEFAULT 'nexora',
    revoked_by VARCHAR(8),
    revoked_at TIMESTAMPTZ,
    last_synchronized_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    idempotency_key TEXT,
    CONSTRAINT service_account_credentials_pkey PRIMARY KEY (id),
    CONSTRAINT service_account_credentials_service_account_id_fkey
        FOREIGN KEY (service_account_id) REFERENCES account.users (id) ON DELETE RESTRICT,
    CONSTRAINT service_account_credentials_created_by_fkey
        FOREIGN KEY (created_by) REFERENCES account.users (id) ON DELETE SET NULL,
    CONSTRAINT service_account_credentials_revoked_by_fkey
        FOREIGN KEY (revoked_by) REFERENCES account.users (id) ON DELETE SET NULL,
    CONSTRAINT service_account_credentials_name_valid
        CHECK (BTRIM(name) <> '' AND CHAR_LENGTH(name) <= 200),
    CONSTRAINT service_account_credentials_provider_id_valid
        CHECK (
            provider_credential_id IS NULL
            OR (BTRIM(provider_credential_id) <> '' AND CHAR_LENGTH(provider_credential_id) <= 255)
        ),
    CONSTRAINT service_account_credentials_expiration_valid
        CHECK (expires_at IS NULL OR expires_at > created_at),
    CONSTRAINT service_account_credentials_type_expiration_valid
        CHECK (credential_type = 'personal_access_token' OR expires_at IS NULL),
    CONSTRAINT service_account_credentials_revocation_consistent
        CHECK (
            (status = 'active' AND revoked_at IS NULL AND revoked_by IS NULL)
            OR (status = 'revoked' AND revoked_at IS NOT NULL)
        ),
    CONSTRAINT service_account_credentials_source_consistent
        CHECK (source = 'nexora' OR created_by IS NULL),
    CONSTRAINT service_account_credentials_idempotency_key_valid
        CHECK (
            idempotency_key IS NULL
            OR (BTRIM(idempotency_key) <> '' AND CHAR_LENGTH(idempotency_key) <= 255)
        )
);

COMMENT ON TABLE account.service_account_credentials IS
    '服务账号 Client Credentials 与 PAT 的非敏感管理元数据；永不保存 Client Secret、PAT 明文或可恢复密文';
COMMENT ON COLUMN account.service_account_credentials.id IS
    '数据库生成的服务账号凭据本地主键';
COMMENT ON COLUMN account.service_account_credentials.service_account_id IS
    '凭据所属服务账号的 8 位本地用户 ID';
COMMENT ON COLUMN account.service_account_credentials.credential_type IS
    '凭据类型，取值来自 account.service_account_credential_type';
COMMENT ON COLUMN account.service_account_credentials.name IS
    '管理员为凭据设置的本地管理名称；外部创建凭据使用稳定的外部创建名称';
COMMENT ON COLUMN account.service_account_credentials.provider_credential_id IS
    '身份 Provider 返回的凭据或 Token ID；Provider 不提供稳定 ID 时为空';
COMMENT ON COLUMN account.service_account_credentials.created_by IS
    '在 Nexora 创建该凭据的本地操作者 ID；Provider 外部创建或操作者删除后为空';
COMMENT ON COLUMN account.service_account_credentials.created_at IS
    'Provider 凭据创建时间；无法取得 Provider 时间时使用首次协调发现时间';
COMMENT ON COLUMN account.service_account_credentials.expires_at IS
    '可选凭据到期时间；PAT 为空表示永不过期，Client Credentials 必须为空';
COMMENT ON COLUMN account.service_account_credentials.status IS
    '最近一次与 Provider 协调后的凭据状态';
COMMENT ON COLUMN account.service_account_credentials.source IS
    '凭据元数据来源，用于区分 Nexora 创建与 Provider 外部创建';
COMMENT ON COLUMN account.service_account_credentials.revoked_by IS
    '在 Nexora 撤销该凭据的本地操作者 ID；Provider 外部撤销或操作者删除后为空';
COMMENT ON COLUMN account.service_account_credentials.revoked_at IS
    '凭据撤销时间；Provider 外部撤销无法取得准确时间时使用协调发现时间';
COMMENT ON COLUMN account.service_account_credentials.last_synchronized_at IS
    '最近一次成功读取 Provider 状态并协调本地元数据的时间';
COMMENT ON COLUMN account.service_account_credentials.idempotency_key IS
    '凭据创建请求的可选幂等键，仅用于阻止重试重复创建或连续轮换，不包含 Secret 或 PAT';
COMMENT ON CONSTRAINT service_account_credentials_pkey ON account.service_account_credentials IS
    '保证每条服务账号凭据元数据具有唯一稳定主键';
COMMENT ON CONSTRAINT service_account_credentials_service_account_id_fkey ON account.service_account_credentials IS
    '禁止删除仍有关联凭据元数据的服务账号';
COMMENT ON CONSTRAINT service_account_credentials_created_by_fkey ON account.service_account_credentials IS
    '创建操作者删除时只清空审计引用，不删除凭据元数据';
COMMENT ON CONSTRAINT service_account_credentials_revoked_by_fkey ON account.service_account_credentials IS
    '撤销操作者删除时只清空审计引用，不改变凭据状态';
COMMENT ON CONSTRAINT service_account_credentials_name_valid ON account.service_account_credentials IS
    '保证凭据管理名称非空且长度不超过 200 个字符';
COMMENT ON CONSTRAINT service_account_credentials_provider_id_valid ON account.service_account_credentials IS
    '限制可选 Provider 凭据 ID 为不超过 255 个字符的非空文本';
COMMENT ON CONSTRAINT service_account_credentials_expiration_valid ON account.service_account_credentials IS
    '提供到期时间时保证它晚于凭据创建时间';
COMMENT ON CONSTRAINT service_account_credentials_type_expiration_valid ON account.service_account_credentials IS
    '仅 PAT 可以设置到期时间，Client Credentials 必须保持为空';
COMMENT ON CONSTRAINT service_account_credentials_revocation_consistent ON account.service_account_credentials IS
    '保证有效凭据没有撤销信息，已撤销凭据至少具有撤销时间';
COMMENT ON CONSTRAINT service_account_credentials_source_consistent ON account.service_account_credentials IS
    '保证 Provider 外部创建的凭据不伪造 Nexora 本地创建人';
COMMENT ON CONSTRAINT service_account_credentials_idempotency_key_valid ON account.service_account_credentials IS
    '限制可选幂等键为不超过 255 个字符的非空文本';
COMMENT ON SEQUENCE account.service_account_credentials_id_seq IS
    '为服务账号凭据元数据生成 BIGSERIAL 本地主键';
COMMENT ON INDEX account.service_account_credentials_pkey IS
    '支撑服务账号凭据元数据主键约束的唯一索引';

CREATE INDEX service_account_credentials_account_created_idx
    ON account.service_account_credentials (service_account_id, created_at DESC, id DESC);
COMMENT ON INDEX account.service_account_credentials_account_created_idx IS
    '支持按服务账号和创建时间稳定查询凭据元数据';

CREATE UNIQUE INDEX service_account_credentials_provider_id_unique_idx
    ON account.service_account_credentials (service_account_id, credential_type, provider_credential_id)
    WHERE provider_credential_id IS NOT NULL;
COMMENT ON INDEX account.service_account_credentials_provider_id_unique_idx IS
    '防止同一服务账号的同一 Provider 凭据在协调时重复写入';

CREATE UNIQUE INDEX service_account_credentials_idempotency_unique_idx
    ON account.service_account_credentials (service_account_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
COMMENT ON INDEX account.service_account_credentials_idempotency_unique_idx IS
    '保证同一服务账号的凭据创建幂等键只对应一次 Provider 操作';

CREATE UNIQUE INDEX service_account_credentials_active_client_secret_idx
    ON account.service_account_credentials (service_account_id)
    WHERE credential_type = 'client_credentials' AND status = 'active';
COMMENT ON INDEX account.service_account_credentials_active_client_secret_idx IS
    '保证每个服务账号最多只有一个当前有效的 Client Secret 元数据';

CREATE OR REPLACE FUNCTION account.protect_super_admin_user()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.is_super_admin THEN
            RAISE EXCEPTION '超级管理员用户不可删除'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'users_super_admin_immutable';
        END IF;
        IF OLD.user_type = 'service_account' THEN
            RAISE EXCEPTION '服务账号不可删除，只能停用'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'users_service_account_delete_forbidden';
        END IF;
        RETURN OLD;
    END IF;

    IF OLD.is_super_admin AND (
        NEW.id IS DISTINCT FROM OLD.id
        OR NEW.identity_id IS DISTINCT FROM OLD.identity_id
        OR NEW.status IS DISTINCT FROM OLD.status
        OR NEW.user_type IS DISTINCT FROM OLD.user_type
        OR NOT NEW.is_super_admin
    ) THEN
        RAISE EXCEPTION '超级管理员身份、类型和状态不可修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'users_super_admin_immutable';
    END IF;

    IF NEW.user_type IS DISTINCT FROM OLD.user_type THEN
        RAISE EXCEPTION '用户类型创建后不可修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'users_user_type_immutable';
    END IF;

    IF OLD.user_type = 'service_account'
        AND NEW.username IS DISTINCT FROM OLD.username
    THEN
        RAISE EXCEPTION '服务账号稳定标识创建后不可修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'users_service_account_identifier_immutable';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION account.protect_super_admin_user() IS
    '拒绝删除服务账号和超级管理员，保护超级管理员身份、类型与状态，并保证全部用户类型及服务账号稳定 username 创建后不可修改';

INSERT INTO account.permissions (key, name, description)
VALUES
    ('service_accounts:provision', '开通服务账号', '在身份 Provider 和本地 Account 中创建服务账号'),
    ('service_accounts:profile.write', '管理服务账号资料', '修改服务账号展示名称和说明'),
    ('service_accounts:credentials.read', '查看服务账号凭据', '查看服务账号凭据的非敏感元数据和 Provider 协调状态'),
    ('service_accounts:credentials.write', '管理服务账号凭据', '生成、轮换和撤销服务账号 Client Credentials 或 PAT');

INSERT INTO account.role_permissions (role_id, permission_id)
SELECT roles.id, permissions.id
FROM account.roles AS roles
CROSS JOIN account.permissions AS permissions
WHERE roles.key = 'admin'
  AND permissions.key LIKE 'service_accounts:%';

INSERT INTO account.permission_implications (permission_id, implied_permission_id)
SELECT source.id, implied.id
FROM account.permissions AS source
JOIN account.permissions AS implied
    ON implied.key = CASE source.key
        WHEN 'service_accounts:provision' THEN 'users:read'
        WHEN 'service_accounts:profile.write' THEN 'users:read'
        WHEN 'service_accounts:credentials.read' THEN 'users:read'
        WHEN 'service_accounts:credentials.write' THEN 'service_accounts:credentials.read'
        ELSE NULL
    END
WHERE source.key LIKE 'service_accounts:%';

INSERT INTO account.permission_implications (permission_id, implied_permission_id)
SELECT source.id, implied.id
FROM account.permissions AS source
JOIN account.permissions AS implied ON implied.key = 'users:read'
WHERE source.key = 'service_accounts:credentials.write';
