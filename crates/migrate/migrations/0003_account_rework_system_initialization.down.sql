DROP TRIGGER IF EXISTS system_initialization_protect ON account.system_initialization;
DROP FUNCTION IF EXISTS account.protect_system_initialization();
DROP TABLE IF EXISTS account.system_initialization;

DROP TRIGGER IF EXISTS user_roles_protect_super_admin ON account.user_roles;
DROP FUNCTION IF EXISTS account.protect_super_admin_role_assignment();

DROP TRIGGER IF EXISTS users_protect_super_admin ON account.users;
DROP FUNCTION IF EXISTS account.protect_super_admin_user();

ALTER TABLE account.users
    DROP CONSTRAINT users_identity_id_unique,
    DROP CONSTRAINT users_identity_id_valid;

ALTER TABLE account.users RENAME COLUMN identity_id TO subject;
ALTER TABLE account.users
    ADD COLUMN issuer TEXT NOT NULL DEFAULT 'https://identity.invalid/';

ALTER TABLE account.users
    ADD CONSTRAINT users_identity_unique UNIQUE (issuer, subject),
    ADD CONSTRAINT users_issuer_valid
        CHECK (BTRIM(issuer) <> '' AND LENGTH(issuer) <= 2048),
    ADD CONSTRAINT users_subject_valid
        CHECK (BTRIM(subject) <> '' AND LENGTH(subject) <= 255);

COMMENT ON COLUMN account.users.issuer IS '回滚时恢复的 OIDC issuer；原始值已无法从 identity_id 重建';
COMMENT ON COLUMN account.users.subject IS 'OIDC issuer 范围内稳定且唯一的 subject';
COMMENT ON CONSTRAINT users_identity_unique ON account.users IS '保证同一 issuer 与 subject 只绑定一个本地用户';
COMMENT ON CONSTRAINT users_issuer_valid ON account.users IS '保证 OIDC issuer 非空且长度不超过 2048 个字符';
COMMENT ON CONSTRAINT users_subject_valid ON account.users IS '保证 OIDC subject 非空且长度不超过 255 个字符';
COMMENT ON COLUMN account.users.is_super_admin IS '是否为系统唯一且身份、状态和角色均不可变的内置超级管理员';

UPDATE account.roles
SET key = 'administrator',
    name = '系统管理员',
    description = '拥有全部系统权限；系统角色不可编辑或删除',
    updated_at = NOW()
WHERE key = 'admin';

INSERT INTO account.roles (key, name, description, is_system)
VALUES (
    'super-administrator',
    '超级管理员',
    '唯一内置账号使用的系统角色；本地不可编辑、删除或授予其他用户',
    TRUE
);

INSERT INTO account.role_permissions (role_id, permission_id)
SELECT roles.id, permissions.id
FROM account.roles AS roles
CROSS JOIN account.permissions AS permissions
WHERE roles.key = 'super-administrator';

INSERT INTO account.user_roles (user_id, role_id)
SELECT users.id, roles.id
FROM account.users AS users
JOIN account.roles AS roles ON roles.key IN ('member', 'super-administrator')
WHERE users.is_super_admin;

CREATE FUNCTION account.protect_super_admin_user()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.is_super_admin THEN
            RAISE EXCEPTION '内置超级管理员账号不可删除'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'users_super_admin_immutable';
        END IF;
        RETURN OLD;
    END IF;

    IF OLD.is_super_admin AND (
        NEW.id IS DISTINCT FROM OLD.id
        OR NEW.issuer IS DISTINCT FROM OLD.issuer
        OR NEW.subject IS DISTINCT FROM OLD.subject
        OR NEW.status IS DISTINCT FROM OLD.status
        OR NOT NEW.is_super_admin
    ) THEN
        RAISE EXCEPTION '内置超级管理员身份和状态不可修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'users_super_admin_immutable';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION account.protect_super_admin_user() IS '拒绝删除内置超级管理员，或修改其身份、访问状态和超级管理员标记';

CREATE TRIGGER users_protect_super_admin
BEFORE UPDATE OR DELETE ON account.users
FOR EACH ROW
EXECUTE FUNCTION account.protect_super_admin_user();

COMMENT ON TRIGGER users_protect_super_admin ON account.users IS '在更新或删除用户前保护内置超级管理员的不变属性';

CREATE FUNCTION account.protect_super_admin_role_assignment()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    super_admin_role_id UUID;
    new_role_key TEXT;
BEGIN
    SELECT id INTO super_admin_role_id
    FROM account.roles
    WHERE key = 'super-administrator';

    IF TG_OP = 'DELETE' THEN
        IF EXISTS (
            SELECT 1 FROM account.users
            WHERE id = OLD.user_id AND is_super_admin
        ) THEN
            RAISE EXCEPTION '内置超级管理员角色不可修改'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'user_roles_super_admin_immutable';
        END IF;
        RETURN OLD;
    END IF;

    IF TG_OP = 'UPDATE' AND (
        EXISTS (SELECT 1 FROM account.users WHERE id = OLD.user_id AND is_super_admin)
        OR EXISTS (SELECT 1 FROM account.users WHERE id = NEW.user_id AND is_super_admin)
    ) THEN
        RAISE EXCEPTION '内置超级管理员角色不可修改'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'user_roles_super_admin_immutable';
    END IF;

    IF TG_OP = 'INSERT' AND EXISTS (
        SELECT 1 FROM account.users
        WHERE id = NEW.user_id AND is_super_admin
    ) THEN
        SELECT key INTO new_role_key FROM account.roles WHERE id = NEW.role_id;
        IF new_role_key NOT IN ('member', 'super-administrator') THEN
            RAISE EXCEPTION '内置超级管理员角色不可修改'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'user_roles_super_admin_immutable';
        END IF;
    END IF;

    IF NEW.role_id = super_admin_role_id AND NOT EXISTS (
        SELECT 1 FROM account.users
        WHERE id = NEW.user_id AND is_super_admin
    ) THEN
        RAISE EXCEPTION '超级管理员角色仅供内置超级管理员账号使用'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'user_roles_super_admin_role_reserved';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION account.protect_super_admin_role_assignment() IS '冻结内置超级管理员的角色集合，并禁止把保留角色授予普通用户';

CREATE TRIGGER user_roles_protect_super_admin
BEFORE INSERT OR UPDATE OR DELETE ON account.user_roles
FOR EACH ROW
EXECUTE FUNCTION account.protect_super_admin_role_assignment();

COMMENT ON TRIGGER user_roles_protect_super_admin ON account.user_roles IS '在角色关系写入前保护超级管理员的固定授权关系';
