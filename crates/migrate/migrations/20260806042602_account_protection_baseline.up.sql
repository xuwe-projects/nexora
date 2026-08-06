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

COMMENT ON FUNCTION account.protect_super_admin_user() IS
    '拒绝删除超级管理员用户，或修改其用户 ID、认证授权身份、访问状态和超级管理员标记';

CREATE TRIGGER users_protect_super_admin
BEFORE UPDATE OR DELETE ON account.users
FOR EACH ROW
EXECUTE FUNCTION account.protect_super_admin_user();

COMMENT ON TRIGGER users_protect_super_admin ON account.users IS
    '在更新或删除用户前保护超级管理员的不变属性';

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

COMMENT ON FUNCTION account.protect_super_admin_role_assignment() IS
    '拒绝为超级管理员增加、替换或删除角色关系，保证该身份始终不挂载角色';

CREATE TRIGGER user_roles_protect_super_admin
BEFORE INSERT OR UPDATE OR DELETE ON account.user_roles
FOR EACH ROW
EXECUTE FUNCTION account.protect_super_admin_role_assignment();

COMMENT ON TRIGGER user_roles_protect_super_admin ON account.user_roles IS
    '在角色关系写入前保证超级管理员不挂载任何角色';

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

CREATE TRIGGER system_initialization_protect
BEFORE UPDATE OR DELETE ON account.system_initialization
FOR EACH ROW
EXECUTE FUNCTION account.protect_system_initialization();

COMMENT ON TRIGGER system_initialization_protect ON account.system_initialization IS
    '保证部署 issuer 首次绑定后不可替换，并保证一次性系统初始化完成后永久不可回退';
