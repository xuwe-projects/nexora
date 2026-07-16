ALTER TABLE account.system_initialization
    ADD COLUMN identity_issuer TEXT,
    ADD CONSTRAINT system_initialization_identity_issuer_valid CHECK (
        identity_issuer IS NULL
        OR (BTRIM(identity_issuer) <> '' AND LENGTH(identity_issuer) <= 2048)
    );

COMMENT ON COLUMN account.system_initialization.identity_issuer IS
    '当前部署唯一允许使用的规范 OIDC issuer URL；首次启动绑定后永久不可更换';
COMMENT ON CONSTRAINT system_initialization_identity_issuer_valid
    ON account.system_initialization IS
    '允许首次绑定前为空；绑定值必须非空且长度不超过 2048 个字符';

CREATE OR REPLACE FUNCTION account.protect_system_initialization()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION '系统初始化状态记录不可删除'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'system_initialization_immutable';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id THEN
        RAISE EXCEPTION '系统初始化状态单例主键不可修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'system_initialization_immutable';
    END IF;

    IF OLD.identity_issuer IS NOT NULL
        AND NEW.identity_issuer IS DISTINCT FROM OLD.identity_issuer
    THEN
        RAISE EXCEPTION '部署 OIDC issuer 首次绑定后不可修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'system_initialization_identity_issuer_immutable';
    END IF;

    IF OLD.is_initialized AND (
        NEW.is_initialized IS DISTINCT FROM OLD.is_initialized
        OR NEW.super_admin_user_id IS DISTINCT FROM OLD.super_admin_user_id
        OR NEW.initialized_at IS DISTINCT FROM OLD.initialized_at
    ) THEN
        RAISE EXCEPTION '系统初始化完成后不可回退或修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'system_initialization_immutable';
    END IF;

    IF NEW.is_initialized AND NEW.identity_issuer IS NULL THEN
        RAISE EXCEPTION '完成系统初始化前必须绑定部署 OIDC issuer'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'system_initialization_identity_issuer_required';
    END IF;

    IF NEW.is_initialized AND NOT EXISTS (
        SELECT 1 FROM account.users
        WHERE id = NEW.super_admin_user_id AND is_super_admin
    ) THEN
        RAISE EXCEPTION '完成系统初始化前必须设置有效的超级管理员'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'system_initialization_super_admin_required';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION account.protect_system_initialization() IS
    '保护初始化状态与部署级 OIDC issuer；issuer 仅允许从空值首次绑定，初始化完成后其余状态永久不可修改';

COMMENT ON TRIGGER system_initialization_protect
    ON account.system_initialization IS
    '保证部署 issuer 首次绑定后不可替换，并保证一次性系统初始化完成后永久不可回退';

INSERT INTO account.permissions (key, name, description)
VALUES (
    'users:provision',
    '开通用户',
    '把经过管理员确认的 OIDC subject 显式开通为本地用户'
);

INSERT INTO account.role_permissions (role_id, permission_id)
SELECT roles.id, permissions.id
FROM account.roles AS roles
CROSS JOIN account.permissions AS permissions
WHERE roles.key = 'admin'
  AND permissions.key = 'users:provision';
