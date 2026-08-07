CREATE TABLE account.roles (
    id BIGSERIAL NOT NULL,
    key TEXT NOT NULL,
    owner TEXT NOT NULL DEFAULT 'IMES',
    name TEXT NOT NULL,
    description TEXT,
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT roles_pkey PRIMARY KEY (id),
    CONSTRAINT roles_key_unique UNIQUE (key),
    CONSTRAINT roles_key_format CHECK (key ~ '^[a-z][a-z0-9._-]{1,63}$'),
    CONSTRAINT roles_owner_valid CHECK (BTRIM(owner) <> '' AND LENGTH(owner) <= 200),
    CONSTRAINT roles_name_valid CHECK (BTRIM(name) <> '' AND LENGTH(name) <= 100),
    CONSTRAINT roles_description_length
        CHECK (description IS NULL OR LENGTH(description) <= 1000)
);

COMMENT ON TABLE account.roles IS '可授予用户的角色目录，包含系统角色和运行时创建的自定义角色';
COMMENT ON COLUMN account.roles.id IS '数据库自动生成的 BIGSERIAL 角色主键';
COMMENT ON COLUMN account.roles.key IS '授权规则使用的稳定角色键';
COMMENT ON COLUMN account.roles.owner IS '角色所属范围；IMES 表示后台系统角色和后台自定义角色，其他值由宿主作为客户或业务范围 ID';
COMMENT ON COLUMN account.roles.name IS '面向管理界面展示的角色名称';
COMMENT ON COLUMN account.roles.description IS '角色用途的可选说明';
COMMENT ON COLUMN account.roles.is_system IS '是否为不可编辑和删除的系统预置角色';
COMMENT ON COLUMN account.roles.created_at IS '角色创建时间';
COMMENT ON COLUMN account.roles.updated_at IS '角色元数据或权限集合最后更新时间';
COMMENT ON CONSTRAINT roles_pkey ON account.roles IS '保证每个角色具有唯一稳定主键';
COMMENT ON CONSTRAINT roles_key_unique ON account.roles IS '保证角色键在账号模块内唯一';
COMMENT ON CONSTRAINT roles_key_format ON account.roles IS '限制角色键使用稳定的小写授权标识格式';
COMMENT ON CONSTRAINT roles_owner_valid ON account.roles IS '保证角色所属范围非空且长度不超过 200 个字符';
COMMENT ON CONSTRAINT roles_name_valid ON account.roles IS '保证角色展示名称非空且长度不超过 100 个字符';
COMMENT ON CONSTRAINT roles_description_length ON account.roles IS '限制可选角色说明长度不超过 1000 个字符';
COMMENT ON SEQUENCE account.roles_id_seq IS '为 account.roles 生成 BIGSERIAL 角色主键';
COMMENT ON INDEX account.roles_pkey IS '支撑角色稳定主键约束的唯一索引';
COMMENT ON INDEX account.roles_key_unique IS '支撑角色键全局唯一约束的索引';

CREATE INDEX roles_owner_key_idx ON account.roles (owner, key);
COMMENT ON INDEX account.roles_owner_key_idx IS '支持按角色所属范围查询角色目录并按全局唯一角色键稳定排序';

CREATE TABLE account.permissions (
    id BIGSERIAL NOT NULL,
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
COMMENT ON COLUMN account.permissions.id IS '数据库自动生成的 BIGSERIAL 权限主键';
COMMENT ON COLUMN account.permissions.key IS '授权判断使用的资源与操作组合键';
COMMENT ON COLUMN account.permissions.name IS '面向管理界面展示的权限名称';
COMMENT ON COLUMN account.permissions.description IS '权限用途的可选说明';
COMMENT ON COLUMN account.permissions.created_at IS '权限首次进入目录的时间';
COMMENT ON CONSTRAINT permissions_pkey ON account.permissions IS '保证每个权限具有唯一稳定主键';
COMMENT ON CONSTRAINT permissions_key_unique ON account.permissions IS '保证权限键在账号模块内唯一';
COMMENT ON CONSTRAINT permissions_key_format ON account.permissions IS '保证权限键符合 resource:action 的小写稳定格式';
COMMENT ON CONSTRAINT permissions_name_valid ON account.permissions IS '保证权限展示名称非空且长度不超过 100 个字符';
COMMENT ON CONSTRAINT permissions_description_length ON account.permissions IS '限制可选权限说明长度不超过 1000 个字符';
COMMENT ON SEQUENCE account.permissions_id_seq IS '为 account.permissions 生成 BIGSERIAL 权限主键';
COMMENT ON INDEX account.permissions_pkey IS '支撑权限稳定主键约束的唯一索引';
COMMENT ON INDEX account.permissions_key_unique IS '支撑权限键全局唯一约束的索引';

CREATE TABLE account.user_roles (
    user_id VARCHAR(8) NOT NULL,
    role_id BIGINT NOT NULL,
    granted_by VARCHAR(8),
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
COMMENT ON COLUMN account.user_roles.user_id IS '获得角色的 8 位本地用户 ID';
COMMENT ON COLUMN account.user_roles.role_id IS '直接授予用户的 BIGSERIAL 角色 ID';
COMMENT ON COLUMN account.user_roles.granted_by IS '执行角色授予的 8 位本地用户 ID，授权人删除后保留空值';
COMMENT ON COLUMN account.user_roles.created_at IS '角色首次直接授予用户的时间';
COMMENT ON CONSTRAINT user_roles_pkey ON account.user_roles IS '防止同一角色重复直接授予同一用户';
COMMENT ON CONSTRAINT user_roles_user_id_fkey ON account.user_roles IS '用户删除时级联清理其角色关系';
COMMENT ON CONSTRAINT user_roles_role_id_fkey ON account.user_roles IS '仍被用户使用的角色禁止删除';
COMMENT ON CONSTRAINT user_roles_granted_by_fkey ON account.user_roles IS '授权人删除时仅清空审计引用，不删除角色关系';
COMMENT ON INDEX account.user_roles_pkey IS '支撑用户与角色关系复合主键约束的唯一索引';

CREATE INDEX user_roles_role_id_idx ON account.user_roles (role_id, user_id);
COMMENT ON INDEX account.user_roles_role_id_idx IS '支持从角色反向查询直接拥有该角色的用户';

CREATE TABLE account.role_permissions (
    role_id BIGINT NOT NULL,
    permission_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT role_permissions_pkey PRIMARY KEY (role_id, permission_id),
    CONSTRAINT role_permissions_role_id_fkey
        FOREIGN KEY (role_id) REFERENCES account.roles (id) ON DELETE CASCADE,
    CONSTRAINT role_permissions_permission_id_fkey
        FOREIGN KEY (permission_id) REFERENCES account.permissions (id) ON DELETE CASCADE
);

COMMENT ON TABLE account.role_permissions IS '角色与权限之间的多对多直接授权关系';
COMMENT ON COLUMN account.role_permissions.role_id IS '获得权限的 BIGSERIAL 角色 ID';
COMMENT ON COLUMN account.role_permissions.permission_id IS '授予角色的 BIGSERIAL 权限 ID';
COMMENT ON COLUMN account.role_permissions.created_at IS '角色首次获得该权限的时间';
COMMENT ON CONSTRAINT role_permissions_pkey ON account.role_permissions IS '防止同一权限重复授予同一角色';
COMMENT ON CONSTRAINT role_permissions_role_id_fkey ON account.role_permissions IS '角色删除时级联清理其权限关系';
COMMENT ON CONSTRAINT role_permissions_permission_id_fkey ON account.role_permissions IS '权限删除时级联清理其角色关系';
COMMENT ON INDEX account.role_permissions_pkey IS '支撑角色与权限关系复合主键约束的唯一索引';

CREATE INDEX role_permissions_permission_id_idx
    ON account.role_permissions (permission_id, role_id);
COMMENT ON INDEX account.role_permissions_permission_id_idx IS '支持从权限反向查询包含该权限的角色';

CREATE TABLE account.permission_implications (
    permission_id BIGINT NOT NULL,
    implied_permission_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT permission_implications_pkey PRIMARY KEY (permission_id, implied_permission_id),
    CONSTRAINT permission_implications_not_self CHECK (permission_id <> implied_permission_id),
    CONSTRAINT permission_implications_permission_id_fkey
        FOREIGN KEY (permission_id) REFERENCES account.permissions (id) ON DELETE CASCADE,
    CONSTRAINT permission_implications_implied_permission_id_fkey
        FOREIGN KEY (implied_permission_id) REFERENCES account.permissions (id) ON DELETE CASCADE
);

COMMENT ON TABLE account.permission_implications IS '权限目录中由一个权限自动蕴含另一个权限的静态依赖关系';
COMMENT ON COLUMN account.permission_implications.permission_id IS '声明蕴含关系的上游权限 ID';
COMMENT ON COLUMN account.permission_implications.implied_permission_id IS '被上游权限自动补入角色授权集合的权限 ID';
COMMENT ON COLUMN account.permission_implications.created_at IS '权限蕴含关系首次写入的时间';
COMMENT ON CONSTRAINT permission_implications_pkey ON account.permission_implications IS '防止同一权限蕴含关系重复写入';
COMMENT ON CONSTRAINT permission_implications_not_self ON account.permission_implications IS '防止权限直接蕴含自身';
COMMENT ON CONSTRAINT permission_implications_permission_id_fkey ON account.permission_implications IS '权限删除时级联清理其作为上游的蕴含关系';
COMMENT ON CONSTRAINT permission_implications_implied_permission_id_fkey ON account.permission_implications IS '权限删除时级联清理其作为下游的蕴含关系';
COMMENT ON INDEX account.permission_implications_pkey IS '支撑权限蕴含关系复合主键约束的唯一索引';

CREATE INDEX permission_implications_implied_permission_id_idx
    ON account.permission_implications (implied_permission_id, permission_id);
COMMENT ON INDEX account.permission_implications_implied_permission_id_idx IS '支持从被蕴含权限反向查询上游权限';
