DROP TRIGGER IF EXISTS system_initialization_protect ON account.system_initialization;
DROP FUNCTION IF EXISTS account.protect_system_initialization();

DROP TRIGGER IF EXISTS user_roles_protect_super_admin ON account.user_roles;
DROP FUNCTION IF EXISTS account.protect_super_admin_role_assignment();

DROP TRIGGER IF EXISTS users_protect_super_admin ON account.users;
DROP FUNCTION IF EXISTS account.protect_super_admin_user();

DROP INDEX account.users_created_at_id_idx;
DROP INDEX account.role_permissions_permission_id_idx;
DROP INDEX account.user_roles_role_id_idx;

CREATE SEQUENCE account.roles_id_seq AS BIGINT;
COMMENT ON SEQUENCE account.roles_id_seq IS '为 account.roles 生成 BIGSERIAL 角色主键';

CREATE SEQUENCE account.permissions_id_seq AS BIGINT;
COMMENT ON SEQUENCE account.permissions_id_seq IS '为 account.permissions 生成 BIGSERIAL 权限主键';

ALTER TABLE account.users
    ADD COLUMN id_varchar VARCHAR(8);

-- 逐行回填可区分大小写的 8 位随机字母数字 ID，并在迁移内避免碰撞。
DO $$
DECLARE
    alphabet CONSTANT TEXT := 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    user_record RECORD;
    candidate VARCHAR(8);
BEGIN
    FOR user_record IN SELECT id FROM account.users ORDER BY id LOOP
        LOOP
            SELECT string_agg(
                substr(alphabet, 1 + floor(random() * length(alphabet))::INTEGER, 1),
                ''
            )
            INTO candidate
            FROM generate_series(1, 8);

            EXIT WHEN NOT EXISTS (
                SELECT 1 FROM account.users WHERE id_varchar = candidate
            );
        END LOOP;

        UPDATE account.users
        SET id_varchar = candidate
        WHERE id = user_record.id;
    END LOOP;
END;
$$;

ALTER TABLE account.roles
    ADD COLUMN id_bigint BIGINT NOT NULL DEFAULT nextval('account.roles_id_seq');

ALTER TABLE account.permissions
    ADD COLUMN id_bigint BIGINT NOT NULL DEFAULT nextval('account.permissions_id_seq');

ALTER TABLE account.user_roles
    ADD COLUMN user_id_varchar VARCHAR(8),
    ADD COLUMN role_id_bigint BIGINT,
    ADD COLUMN granted_by_varchar VARCHAR(8);

UPDATE account.user_roles AS user_roles
SET user_id_varchar = users.id_varchar,
    role_id_bigint = roles.id_bigint
FROM account.users AS users,
     account.roles AS roles
WHERE users.id = user_roles.user_id
  AND roles.id = user_roles.role_id;

UPDATE account.user_roles AS user_roles
SET granted_by_varchar = users.id_varchar
FROM account.users AS users
WHERE users.id = user_roles.granted_by;

ALTER TABLE account.role_permissions
    ADD COLUMN role_id_bigint BIGINT,
    ADD COLUMN permission_id_bigint BIGINT;

UPDATE account.role_permissions AS role_permissions
SET role_id_bigint = roles.id_bigint,
    permission_id_bigint = permissions.id_bigint
FROM account.roles AS roles,
     account.permissions AS permissions
WHERE roles.id = role_permissions.role_id
  AND permissions.id = role_permissions.permission_id;

ALTER TABLE account.system_initialization
    ADD COLUMN super_admin_user_id_varchar VARCHAR(8);

UPDATE account.system_initialization AS initialization
SET super_admin_user_id_varchar = users.id_varchar
FROM account.users AS users
WHERE users.id = initialization.super_admin_user_id;

-- 任何旧外键没有完整映射时都立即终止。SQLx 会在事务中执行 PostgreSQL 迁移，失败会回滚
-- 本迁移此前的所有 DDL/DML，避免丢弃旧列后才发现关联数据不完整。
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM account.users WHERE id_varchar IS NULL) THEN
        RAISE EXCEPTION '用户 ID 回填不完整，拒绝继续迁移';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM account.user_roles
        WHERE user_id_varchar IS NULL
           OR role_id_bigint IS NULL
           OR (granted_by IS NOT NULL AND granted_by_varchar IS NULL)
    ) THEN
        RAISE EXCEPTION '用户角色关联 ID 回填不完整，拒绝继续迁移';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM account.role_permissions
        WHERE role_id_bigint IS NULL OR permission_id_bigint IS NULL
    ) THEN
        RAISE EXCEPTION '角色权限关联 ID 回填不完整，拒绝继续迁移';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM account.system_initialization
        WHERE super_admin_user_id IS NOT NULL
          AND super_admin_user_id_varchar IS NULL
    ) THEN
        RAISE EXCEPTION '系统初始化超级管理员 ID 回填不完整，拒绝继续迁移';
    END IF;
END;
$$;

ALTER TABLE account.system_initialization
    DROP CONSTRAINT system_initialization_super_admin_user_id_fkey,
    DROP CONSTRAINT system_initialization_state_consistent;

ALTER TABLE account.user_roles
    DROP CONSTRAINT user_roles_pkey,
    DROP CONSTRAINT user_roles_user_id_fkey,
    DROP CONSTRAINT user_roles_role_id_fkey,
    DROP CONSTRAINT user_roles_granted_by_fkey;

ALTER TABLE account.role_permissions
    DROP CONSTRAINT role_permissions_pkey,
    DROP CONSTRAINT role_permissions_role_id_fkey,
    DROP CONSTRAINT role_permissions_permission_id_fkey;

ALTER TABLE account.users DROP CONSTRAINT users_pkey;
ALTER TABLE account.roles DROP CONSTRAINT roles_pkey;
ALTER TABLE account.permissions DROP CONSTRAINT permissions_pkey;

ALTER TABLE account.system_initialization DROP COLUMN super_admin_user_id;
ALTER TABLE account.system_initialization
    RENAME COLUMN super_admin_user_id_varchar TO super_admin_user_id;

ALTER TABLE account.user_roles
    DROP COLUMN user_id,
    DROP COLUMN role_id,
    DROP COLUMN granted_by;
ALTER TABLE account.user_roles RENAME COLUMN user_id_varchar TO user_id;
ALTER TABLE account.user_roles RENAME COLUMN role_id_bigint TO role_id;
ALTER TABLE account.user_roles RENAME COLUMN granted_by_varchar TO granted_by;

ALTER TABLE account.role_permissions
    DROP COLUMN role_id,
    DROP COLUMN permission_id;
ALTER TABLE account.role_permissions RENAME COLUMN role_id_bigint TO role_id;
ALTER TABLE account.role_permissions RENAME COLUMN permission_id_bigint TO permission_id;

ALTER TABLE account.users DROP COLUMN id;
ALTER TABLE account.users RENAME COLUMN id_varchar TO id;

ALTER TABLE account.roles DROP COLUMN id;
ALTER TABLE account.roles RENAME COLUMN id_bigint TO id;

ALTER TABLE account.permissions DROP COLUMN id;
ALTER TABLE account.permissions RENAME COLUMN id_bigint TO id;

ALTER SEQUENCE account.roles_id_seq OWNED BY account.roles.id;
ALTER SEQUENCE account.permissions_id_seq OWNED BY account.permissions.id;

ALTER TABLE account.users
    ALTER COLUMN id SET NOT NULL,
    ADD CONSTRAINT users_pkey PRIMARY KEY (id),
    ADD CONSTRAINT users_id_format CHECK (id ~ '^[A-Za-z0-9]{8}$');

ALTER TABLE account.roles
    ADD CONSTRAINT roles_pkey PRIMARY KEY (id);

ALTER TABLE account.permissions
    ADD CONSTRAINT permissions_pkey PRIMARY KEY (id);

ALTER TABLE account.user_roles
    ALTER COLUMN user_id SET NOT NULL,
    ALTER COLUMN role_id SET NOT NULL,
    ADD CONSTRAINT user_roles_pkey PRIMARY KEY (user_id, role_id),
    ADD CONSTRAINT user_roles_user_id_fkey
        FOREIGN KEY (user_id) REFERENCES account.users (id) ON DELETE CASCADE,
    ADD CONSTRAINT user_roles_role_id_fkey
        FOREIGN KEY (role_id) REFERENCES account.roles (id) ON DELETE RESTRICT,
    ADD CONSTRAINT user_roles_granted_by_fkey
        FOREIGN KEY (granted_by) REFERENCES account.users (id) ON DELETE SET NULL;

ALTER TABLE account.role_permissions
    ALTER COLUMN role_id SET NOT NULL,
    ALTER COLUMN permission_id SET NOT NULL,
    ADD CONSTRAINT role_permissions_pkey PRIMARY KEY (role_id, permission_id),
    ADD CONSTRAINT role_permissions_role_id_fkey
        FOREIGN KEY (role_id) REFERENCES account.roles (id) ON DELETE CASCADE,
    ADD CONSTRAINT role_permissions_permission_id_fkey
        FOREIGN KEY (permission_id) REFERENCES account.permissions (id) ON DELETE CASCADE;

ALTER TABLE account.system_initialization
    ADD CONSTRAINT system_initialization_state_consistent CHECK (
        (NOT is_initialized AND super_admin_user_id IS NULL AND initialized_at IS NULL)
        OR (is_initialized AND super_admin_user_id IS NOT NULL AND initialized_at IS NOT NULL)
    ),
    ADD CONSTRAINT system_initialization_super_admin_user_id_fkey
        FOREIGN KEY (super_admin_user_id) REFERENCES account.users (id) ON DELETE RESTRICT;

COMMENT ON COLUMN account.users.id IS '本地生成的 8 位大小写字母与数字随机用户主键';
COMMENT ON CONSTRAINT users_pkey ON account.users IS '保证每个本地用户具有唯一稳定主键';
COMMENT ON CONSTRAINT users_id_format ON account.users IS '保证用户 ID 固定为 8 位大小写字母或数字';

COMMENT ON COLUMN account.roles.id IS '数据库自动生成的 BIGSERIAL 角色主键';
COMMENT ON CONSTRAINT roles_pkey ON account.roles IS '保证每个角色具有唯一稳定主键';

COMMENT ON COLUMN account.permissions.id IS '数据库自动生成的 BIGSERIAL 权限主键';
COMMENT ON CONSTRAINT permissions_pkey ON account.permissions IS '保证每个权限具有唯一稳定主键';

COMMENT ON COLUMN account.user_roles.user_id IS '获得角色的 8 位本地用户 ID';
COMMENT ON COLUMN account.user_roles.role_id IS '直接授予用户的 BIGSERIAL 角色 ID';
COMMENT ON COLUMN account.user_roles.granted_by IS '执行角色授予的 8 位本地用户 ID，授权人删除后保留空值';
COMMENT ON CONSTRAINT user_roles_pkey ON account.user_roles IS '防止同一角色重复直接授予同一用户';
COMMENT ON CONSTRAINT user_roles_user_id_fkey ON account.user_roles IS '用户删除时级联清理其角色关系';
COMMENT ON CONSTRAINT user_roles_role_id_fkey ON account.user_roles IS '仍被用户使用的角色禁止删除';
COMMENT ON CONSTRAINT user_roles_granted_by_fkey ON account.user_roles IS '授权人删除时仅清空审计引用，不删除角色关系';

COMMENT ON COLUMN account.role_permissions.role_id IS '获得权限的 BIGSERIAL 角色 ID';
COMMENT ON COLUMN account.role_permissions.permission_id IS '授予角色的 BIGSERIAL 权限 ID';
COMMENT ON CONSTRAINT role_permissions_pkey ON account.role_permissions IS '防止同一权限重复授予同一角色';
COMMENT ON CONSTRAINT role_permissions_role_id_fkey ON account.role_permissions IS '角色删除时级联清理其权限关系';
COMMENT ON CONSTRAINT role_permissions_permission_id_fkey ON account.role_permissions IS '权限删除时级联清理其角色关系';

COMMENT ON COLUMN account.system_initialization.super_admin_user_id IS '初始化时选定的 8 位超级管理员本地用户 ID';
COMMENT ON CONSTRAINT system_initialization_state_consistent ON account.system_initialization IS '保证完成标记与超级管理员、完成时间同时存在或同时为空';
COMMENT ON CONSTRAINT system_initialization_super_admin_user_id_fkey ON account.system_initialization IS '防止删除完成系统初始化的超级管理员用户';

CREATE INDEX users_created_at_id_idx ON account.users (created_at DESC, id DESC);
COMMENT ON INDEX account.users_created_at_id_idx IS '支持按创建时间和用户 ID 稳定倒序分页查询用户';

CREATE INDEX role_permissions_permission_id_idx
    ON account.role_permissions (permission_id, role_id);
COMMENT ON INDEX account.role_permissions_permission_id_idx IS '支持从权限反向查询包含该权限的角色';

CREATE INDEX user_roles_role_id_idx ON account.user_roles (role_id, user_id);
COMMENT ON INDEX account.user_roles_role_id_idx IS '支持从角色反向查询直接拥有该角色的用户';

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

COMMENT ON FUNCTION account.protect_super_admin_user() IS '拒绝删除超级管理员用户，或修改其用户 ID、认证授权身份、访问状态和超级管理员标记';

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
