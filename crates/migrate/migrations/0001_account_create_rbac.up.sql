CREATE SCHEMA IF NOT EXISTS account;

COMMENT ON SCHEMA account IS '账号、身份、角色、权限与本地授权关系使用的独立数据库命名空间';

-- 用户状态是稳定且封闭的访问控制集合，因此使用 PostgreSQL ENUM。
CREATE TYPE account.user_status AS ENUM (
    'active',   -- 正常：用户可以认证并参与授权判断。
    'suspended' -- 停用：保留用户记录，但拒绝访问受保护资源。
);

COMMENT ON TYPE account.user_status IS
    '用户访问状态：active=正常访问，suspended=保留记录但禁止访问受保护资源';

CREATE TABLE account.users (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    email TEXT,
    display_name TEXT NOT NULL,
    avatar_url TEXT,
    status account.user_status NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT users_pkey PRIMARY KEY (id),
    CONSTRAINT users_identity_unique UNIQUE (issuer, subject),
    CONSTRAINT users_issuer_valid CHECK (BTRIM(issuer) <> '' AND LENGTH(issuer) <= 2048),
    CONSTRAINT users_subject_valid CHECK (BTRIM(subject) <> '' AND LENGTH(subject) <= 255),
    CONSTRAINT users_email_length CHECK (email IS NULL OR LENGTH(email) <= 320),
    CONSTRAINT users_display_name_valid
        CHECK (BTRIM(display_name) <> '' AND LENGTH(display_name) <= 200),
    CONSTRAINT users_avatar_url_length
        CHECK (avatar_url IS NULL OR LENGTH(avatar_url) <= 2048)
);

COMMENT ON TABLE account.users IS '与外部 OIDC 身份绑定的本地用户及其访问状态';
COMMENT ON COLUMN account.users.id IS '本地生成的稳定用户 UUID 主键';
COMMENT ON COLUMN account.users.issuer IS '签发用户身份的规范 OIDC issuer URL';
COMMENT ON COLUMN account.users.subject IS 'OIDC issuer 范围内稳定且唯一的 subject';
COMMENT ON COLUMN account.users.email IS '身份提供方返回的可选用户邮箱';
COMMENT ON COLUMN account.users.display_name IS '面向管理界面展示的用户名称';
COMMENT ON COLUMN account.users.avatar_url IS '身份提供方返回的可选头像 URL';
COMMENT ON COLUMN account.users.status IS '用户访问状态，取值来自 account.user_status';
COMMENT ON COLUMN account.users.created_at IS '本地用户记录首次创建时间';
COMMENT ON COLUMN account.users.updated_at IS '本地用户资料最后更新时间';
COMMENT ON COLUMN account.users.last_login_at IS '最近一次成功认证并同步身份的时间';
COMMENT ON CONSTRAINT users_pkey ON account.users IS '保证每个本地用户具有唯一稳定主键';
COMMENT ON CONSTRAINT users_identity_unique ON account.users IS '保证同一 issuer 与 subject 只绑定一个本地用户';
COMMENT ON CONSTRAINT users_issuer_valid ON account.users IS '保证 OIDC issuer 非空且长度不超过 2048 个字符';
COMMENT ON CONSTRAINT users_subject_valid ON account.users IS '保证 OIDC subject 非空且长度不超过 255 个字符';
COMMENT ON CONSTRAINT users_email_length ON account.users IS '限制可选邮箱长度不超过 320 个字符';
COMMENT ON CONSTRAINT users_display_name_valid ON account.users IS '保证展示名称非空且长度不超过 200 个字符';
COMMENT ON CONSTRAINT users_avatar_url_length ON account.users IS '限制可选头像 URL 长度不超过 2048 个字符';

CREATE INDEX users_created_at_id_idx ON account.users (created_at DESC, id DESC);
COMMENT ON INDEX account.users_created_at_id_idx IS '支持按创建时间和用户 ID 稳定倒序分页查询用户';

CREATE INDEX users_status_idx ON account.users (status);
COMMENT ON INDEX account.users_status_idx IS '支持按用户访问状态筛选用户';

CREATE TABLE account.roles (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    key TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT roles_pkey PRIMARY KEY (id),
    CONSTRAINT roles_key_unique UNIQUE (key),
    CONSTRAINT roles_key_format CHECK (key ~ '^[a-z][a-z0-9._-]{1,63}$'),
    CONSTRAINT roles_name_valid CHECK (BTRIM(name) <> '' AND LENGTH(name) <= 100),
    CONSTRAINT roles_description_length
        CHECK (description IS NULL OR LENGTH(description) <= 1000)
);

COMMENT ON TABLE account.roles IS '可授予用户的角色目录，包含系统角色和运行时创建的自定义角色';
COMMENT ON COLUMN account.roles.id IS '角色稳定 UUID 主键';
COMMENT ON COLUMN account.roles.key IS '授权规则使用的稳定角色键';
COMMENT ON COLUMN account.roles.name IS '面向管理界面展示的角色名称';
COMMENT ON COLUMN account.roles.description IS '角色用途的可选说明';
COMMENT ON COLUMN account.roles.is_system IS '是否为不可编辑和删除的系统预置角色';
COMMENT ON COLUMN account.roles.created_at IS '角色创建时间';
COMMENT ON COLUMN account.roles.updated_at IS '角色元数据或权限集合最后更新时间';
COMMENT ON CONSTRAINT roles_pkey ON account.roles IS '保证每个角色具有唯一稳定主键';
COMMENT ON CONSTRAINT roles_key_unique ON account.roles IS '保证角色键在账号模块内唯一';
COMMENT ON CONSTRAINT roles_key_format ON account.roles IS '限制角色键使用稳定的小写授权标识格式';
COMMENT ON CONSTRAINT roles_name_valid ON account.roles IS '保证角色展示名称非空且长度不超过 100 个字符';
COMMENT ON CONSTRAINT roles_description_length ON account.roles IS '限制可选角色说明长度不超过 1000 个字符';

CREATE TABLE account.permissions (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    key TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT permissions_pkey PRIMARY KEY (id),
    CONSTRAINT permissions_key_unique UNIQUE (key),
    CONSTRAINT permissions_key_format
        CHECK (key ~ '^[a-z][a-z0-9._-]{1,63}:[a-z][a-z0-9._-]{1,63}$'),
    CONSTRAINT permissions_name_valid CHECK (BTRIM(name) <> '' AND LENGTH(name) <= 100),
    CONSTRAINT permissions_description_length
        CHECK (description IS NULL OR LENGTH(description) <= 1000)
);

COMMENT ON TABLE account.permissions IS '系统支持的细粒度授权权限目录';
COMMENT ON COLUMN account.permissions.id IS '权限稳定 UUID 主键';
COMMENT ON COLUMN account.permissions.key IS '授权判断使用的资源与操作组合键';
COMMENT ON COLUMN account.permissions.name IS '面向管理界面展示的权限名称';
COMMENT ON COLUMN account.permissions.description IS '权限用途的可选说明';
COMMENT ON COLUMN account.permissions.created_at IS '权限首次进入目录的时间';
COMMENT ON CONSTRAINT permissions_pkey ON account.permissions IS '保证每个权限具有唯一稳定主键';
COMMENT ON CONSTRAINT permissions_key_unique ON account.permissions IS '保证权限键在账号模块内唯一';
COMMENT ON CONSTRAINT permissions_key_format ON account.permissions IS '保证权限键符合 resource:action 的小写稳定格式';
COMMENT ON CONSTRAINT permissions_name_valid ON account.permissions IS '保证权限展示名称非空且长度不超过 100 个字符';
COMMENT ON CONSTRAINT permissions_description_length ON account.permissions IS '限制可选权限说明长度不超过 1000 个字符';

CREATE TABLE account.role_permissions (
    role_id UUID NOT NULL,
    permission_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT role_permissions_pkey PRIMARY KEY (role_id, permission_id),
    CONSTRAINT role_permissions_role_id_fkey
        FOREIGN KEY (role_id) REFERENCES account.roles (id) ON DELETE CASCADE,
    CONSTRAINT role_permissions_permission_id_fkey
        FOREIGN KEY (permission_id) REFERENCES account.permissions (id) ON DELETE CASCADE
);

COMMENT ON TABLE account.role_permissions IS '角色与权限之间的多对多直接授权关系';
COMMENT ON COLUMN account.role_permissions.role_id IS '获得权限的角色 ID';
COMMENT ON COLUMN account.role_permissions.permission_id IS '授予角色的权限 ID';
COMMENT ON COLUMN account.role_permissions.created_at IS '角色首次获得该权限的时间';
COMMENT ON CONSTRAINT role_permissions_pkey ON account.role_permissions IS '防止同一权限重复授予同一角色';
COMMENT ON CONSTRAINT role_permissions_role_id_fkey ON account.role_permissions IS '角色删除时级联清理其权限关系';
COMMENT ON CONSTRAINT role_permissions_permission_id_fkey ON account.role_permissions IS '权限删除时级联清理其角色关系';

CREATE INDEX role_permissions_permission_id_idx
    ON account.role_permissions (permission_id, role_id);
COMMENT ON INDEX account.role_permissions_permission_id_idx IS '支持从权限反向查询包含该权限的角色';

CREATE TABLE account.user_roles (
    user_id UUID NOT NULL,
    role_id UUID NOT NULL,
    granted_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT user_roles_pkey PRIMARY KEY (user_id, role_id),
    CONSTRAINT user_roles_user_id_fkey
        FOREIGN KEY (user_id) REFERENCES account.users (id) ON DELETE CASCADE,
    CONSTRAINT user_roles_role_id_fkey
        FOREIGN KEY (role_id) REFERENCES account.roles (id) ON DELETE RESTRICT,
    CONSTRAINT user_roles_granted_by_fkey
        FOREIGN KEY (granted_by) REFERENCES account.users (id) ON DELETE SET NULL
);

COMMENT ON TABLE account.user_roles IS '本地用户与直接授予角色之间的多对多关系';
COMMENT ON COLUMN account.user_roles.user_id IS '获得角色的用户 ID';
COMMENT ON COLUMN account.user_roles.role_id IS '直接授予用户的角色 ID';
COMMENT ON COLUMN account.user_roles.granted_by IS '执行角色授予的本地用户 ID，授权人删除后保留空值';
COMMENT ON COLUMN account.user_roles.created_at IS '角色首次直接授予用户的时间';
COMMENT ON CONSTRAINT user_roles_pkey ON account.user_roles IS '防止同一角色重复直接授予同一用户';
COMMENT ON CONSTRAINT user_roles_user_id_fkey ON account.user_roles IS '用户删除时级联清理其角色关系';
COMMENT ON CONSTRAINT user_roles_role_id_fkey ON account.user_roles IS '仍被用户使用的角色禁止删除';
COMMENT ON CONSTRAINT user_roles_granted_by_fkey ON account.user_roles IS '授权人删除时仅清空审计引用，不删除角色关系';

CREATE INDEX user_roles_role_id_idx ON account.user_roles (role_id, user_id);
COMMENT ON INDEX account.user_roles_role_id_idx IS '支持从角色反向查询直接拥有该角色的用户';

INSERT INTO account.permissions (key, name, description) VALUES
    ('users:read', '查看用户', '查看用户列表、用户详情及其角色'),
    ('users:roles.write', '管理用户角色', '为用户授予或撤销角色'),
    ('users:status.write', '管理用户状态', '启用或停用用户访问'),
    ('roles:read', '查看角色', '查看角色及角色包含的权限'),
    ('roles:write', '管理角色', '创建、修改、删除非系统角色并配置权限'),
    ('permissions:read', '查看权限', '查看系统支持的权限目录');

INSERT INTO account.roles (key, name, description, is_system) VALUES
    ('administrator', '系统管理员', '拥有全部系统权限；系统角色不可编辑或删除', TRUE),
    ('auditor', '审计员', '只读查看用户、角色和权限', TRUE),
    ('member', '普通成员', '默认登录角色，不包含后台管理权限', TRUE);

INSERT INTO account.role_permissions (role_id, permission_id)
SELECT roles.id, permissions.id
FROM account.roles AS roles
CROSS JOIN account.permissions AS permissions
WHERE roles.key = 'administrator';

INSERT INTO account.role_permissions (role_id, permission_id)
SELECT roles.id, permissions.id
FROM account.roles AS roles
JOIN account.permissions AS permissions
    ON permissions.key IN ('users:read', 'roles:read', 'permissions:read')
WHERE roles.key = 'auditor';
