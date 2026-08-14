DELETE FROM account.permissions
WHERE key IN (
    'service_accounts:provision',
    'service_accounts:profile.write',
    'service_accounts:credentials.read',
    'service_accounts:credentials.write'
);

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
        RETURN OLD;
    END IF;

    IF OLD.is_super_admin AND (
        NEW.id IS DISTINCT FROM OLD.id
        OR NEW.identity_id IS DISTINCT FROM OLD.identity_id
        OR NEW.status IS DISTINCT FROM OLD.status
        OR NOT NEW.is_super_admin
    ) THEN
        RAISE EXCEPTION '超级管理员身份和状态不可修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'users_super_admin_immutable';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION account.protect_super_admin_user() IS
    '拒绝删除超级管理员用户，或修改其用户 ID、认证授权身份、访问状态和超级管理员标记';

DROP TABLE account.service_account_credentials;
DROP TYPE account.service_account_credential_source;
DROP TYPE account.service_account_credential_status;
DROP TYPE account.service_account_credential_type;

ALTER TABLE account.users
    DROP CONSTRAINT users_description_length,
    DROP COLUMN description,
    DROP COLUMN user_type;

DROP TYPE account.user_type;
