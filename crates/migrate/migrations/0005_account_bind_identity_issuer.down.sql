DELETE FROM account.role_permissions AS role_permissions
USING account.permissions AS permissions
WHERE role_permissions.permission_id = permissions.id
  AND permissions.key = 'users:provision';

DELETE FROM account.permissions
WHERE key = 'users:provision';

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

    IF OLD.is_initialized THEN
        RAISE EXCEPTION '系统初始化完成后不可回退或修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'system_initialization_immutable';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id THEN
        RAISE EXCEPTION '系统初始化状态单例主键不可修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'system_initialization_immutable';
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
    '禁止删除初始化状态、修改单例主键或在完成后回退状态，并校验超级管理员';

COMMENT ON TRIGGER system_initialization_protect
    ON account.system_initialization IS
    '保证一次性系统初始化完成后永久不可回退';

ALTER TABLE account.system_initialization
    DROP CONSTRAINT system_initialization_identity_issuer_valid,
    DROP COLUMN identity_issuer;
