DROP TRIGGER IF EXISTS user_roles_protect_super_admin ON account.user_roles;
DROP FUNCTION IF EXISTS account.protect_super_admin_role_assignment();

DROP TRIGGER IF EXISTS users_protect_super_admin ON account.users;
DROP FUNCTION IF EXISTS account.protect_super_admin_user();

DELETE FROM account.user_roles
WHERE user_id IN (
    SELECT id FROM account.users WHERE is_super_admin
);

DELETE FROM account.user_roles AS user_roles
USING account.roles AS roles
WHERE user_roles.role_id = roles.id
  AND roles.key = 'super-administrator';

DELETE FROM account.role_permissions AS role_permissions
USING account.roles AS roles
WHERE role_permissions.role_id = roles.id
  AND roles.key = 'super-administrator';

DELETE FROM account.roles WHERE key = 'super-administrator';

UPDATE account.roles
SET key = 'admin',
    name = '系统管理员',
    description = '拥有系统管理权限；作为普通用户角色仍然完整执行权限校验',
    updated_at = NOW()
WHERE key = 'administrator';

ALTER TABLE account.users
    DROP CONSTRAINT users_identity_unique,
    DROP CONSTRAINT users_issuer_valid,
    DROP CONSTRAINT users_subject_valid;

ALTER TABLE account.users RENAME COLUMN subject TO identity_id;
ALTER TABLE account.users DROP COLUMN issuer;

ALTER TABLE account.users
    ADD CONSTRAINT users_identity_id_unique UNIQUE (identity_id),
    ADD CONSTRAINT users_identity_id_valid
        CHECK (BTRIM(identity_id) <> '' AND LENGTH(identity_id) <= 255);

COMMENT ON COLUMN account.users.identity_id IS '认证授权服务中与用户对应的稳定唯一 ID';
COMMENT ON CONSTRAINT users_identity_id_unique ON account.users IS '保证一个认证授权身份只对应一个本地用户';
COMMENT ON CONSTRAINT users_identity_id_valid ON account.users IS '保证认证授权身份 ID 非空且长度不超过 255 个字符';
COMMENT ON COLUMN account.users.is_super_admin IS '是否为系统唯一超级管理员；该身份不绑定角色或权限并直接绕过权限校验';

CREATE FUNCTION account.protect_super_admin_user()
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

COMMENT ON FUNCTION account.protect_super_admin_user() IS '拒绝删除超级管理员用户，或修改其认证授权身份、访问状态和超级管理员标记';

CREATE TRIGGER users_protect_super_admin
BEFORE UPDATE OR DELETE ON account.users
FOR EACH ROW
EXECUTE FUNCTION account.protect_super_admin_user();

COMMENT ON TRIGGER users_protect_super_admin ON account.users IS '在更新或删除用户前保护超级管理员的不变属性';

CREATE FUNCTION account.protect_super_admin_role_assignment()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP IN ('UPDATE', 'DELETE') AND EXISTS (
        SELECT 1 FROM account.users
        WHERE id = OLD.user_id AND is_super_admin
    ) THEN
        RAISE EXCEPTION '超级管理员不能挂载任何角色'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'user_roles_super_admin_forbidden';
    END IF;

    IF TG_OP IN ('INSERT', 'UPDATE') AND EXISTS (
        SELECT 1 FROM account.users
        WHERE id = NEW.user_id AND is_super_admin
    ) THEN
        RAISE EXCEPTION '超级管理员不能挂载任何角色'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'user_roles_super_admin_forbidden';
    END IF;

    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION account.protect_super_admin_role_assignment() IS '拒绝为超级管理员增加、替换或删除角色关系，保证该身份始终不挂载角色';

CREATE TRIGGER user_roles_protect_super_admin
BEFORE INSERT OR UPDATE OR DELETE ON account.user_roles
FOR EACH ROW
EXECUTE FUNCTION account.protect_super_admin_role_assignment();

COMMENT ON TRIGGER user_roles_protect_super_admin ON account.user_roles IS '在角色关系写入前保证超级管理员不挂载任何角色';

CREATE TABLE account.system_initialization (
    id SMALLINT NOT NULL DEFAULT 1,
    is_initialized BOOLEAN NOT NULL DEFAULT FALSE,
    super_admin_user_id UUID,
    initialized_at TIMESTAMPTZ,
    CONSTRAINT system_initialization_pkey PRIMARY KEY (id),
    CONSTRAINT system_initialization_singleton CHECK (id = 1),
    CONSTRAINT system_initialization_state_consistent CHECK (
        (NOT is_initialized AND super_admin_user_id IS NULL AND initialized_at IS NULL)
        OR (is_initialized AND super_admin_user_id IS NOT NULL AND initialized_at IS NOT NULL)
    ),
    CONSTRAINT system_initialization_super_admin_user_id_fkey
        FOREIGN KEY (super_admin_user_id) REFERENCES account.users (id) ON DELETE RESTRICT
);

COMMENT ON TABLE account.system_initialization IS '系统一次性初始化状态；单例记录完成后禁止再次进入 setup 流程';
COMMENT ON COLUMN account.system_initialization.id IS '固定为 1 的单例主键';
COMMENT ON COLUMN account.system_initialization.is_initialized IS '系统是否已完成所有当前初始化步骤';
COMMENT ON COLUMN account.system_initialization.super_admin_user_id IS '初始化时选定的超级管理员本地用户 ID';
COMMENT ON COLUMN account.system_initialization.initialized_at IS '系统完成初始化的数据库时间';
COMMENT ON CONSTRAINT system_initialization_pkey ON account.system_initialization IS '保证初始化状态单例记录具有稳定主键';
COMMENT ON CONSTRAINT system_initialization_singleton ON account.system_initialization IS '保证初始化状态表只允许固定单例记录';
COMMENT ON CONSTRAINT system_initialization_state_consistent ON account.system_initialization IS '保证完成标记与超级管理员、完成时间同时存在或同时为空';
COMMENT ON CONSTRAINT system_initialization_super_admin_user_id_fkey ON account.system_initialization IS '防止删除完成系统初始化的超级管理员用户';

CREATE FUNCTION account.protect_system_initialization()
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

COMMENT ON FUNCTION account.protect_system_initialization() IS '禁止删除初始化状态、修改单例主键或在完成后回退状态，并校验超级管理员';

CREATE TRIGGER system_initialization_protect
BEFORE UPDATE OR DELETE ON account.system_initialization
FOR EACH ROW
EXECUTE FUNCTION account.protect_system_initialization();

COMMENT ON TRIGGER system_initialization_protect ON account.system_initialization IS '保证一次性系统初始化完成后永久不可回退';

INSERT INTO account.system_initialization (id, is_initialized)
VALUES (1, FALSE);

UPDATE account.system_initialization
SET is_initialized = TRUE,
    super_admin_user_id = users.id,
    initialized_at = NOW()
FROM account.users AS users
WHERE system_initialization.id = 1
  AND users.is_super_admin;
