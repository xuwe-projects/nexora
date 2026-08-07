CREATE SCHEMA account;

COMMENT ON SCHEMA account IS
    '账号、身份、角色、权限与本地授权关系使用的独立数据库命名空间';

-- 用户状态是稳定且封闭的访问控制集合，因此使用 PostgreSQL ENUM。
CREATE TYPE account.user_status AS ENUM (
    'active',   -- 正常：用户可以认证并参与授权判断。
    'suspended' -- 停用：保留用户记录，但拒绝访问受保护资源。
);

COMMENT ON TYPE account.user_status IS
    '用户访问状态：active=正常访问，suspended=保留记录但禁止访问受保护资源';

CREATE TABLE account.users (
    id VARCHAR(8) NOT NULL,
    identity_id TEXT NOT NULL,
    username TEXT,
    email TEXT,
    display_name TEXT NOT NULL,
    status account.user_status NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    is_super_admin BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT users_pkey PRIMARY KEY (id),
    CONSTRAINT users_id_format CHECK (id ~ '^[A-Za-z0-9]{8}$'),
    CONSTRAINT users_identity_id_unique UNIQUE (identity_id),
    CONSTRAINT users_identity_id_valid
        CHECK (BTRIM(identity_id) <> '' AND LENGTH(identity_id) <= 255),
    CONSTRAINT users_username_valid
        CHECK (username IS NULL OR (BTRIM(username) <> '' AND CHAR_LENGTH(username) <= 200)),
    CONSTRAINT users_email_length CHECK (email IS NULL OR LENGTH(email) <= 320),
    CONSTRAINT users_display_name_valid
        CHECK (BTRIM(display_name) <> '' AND LENGTH(display_name) <= 200)
);

COMMENT ON TABLE account.users IS '与外部 OIDC 身份绑定的本地用户及其访问状态';
COMMENT ON COLUMN account.users.id IS '本地生成的 8 位大小写字母与数字随机用户主键';
COMMENT ON COLUMN account.users.identity_id IS '认证授权服务中与用户对应的稳定唯一 ID';
COMMENT ON COLUMN account.users.username IS '认证授权服务中的可选登录用户名；身份绑定仍以稳定 identity_id 为准';
COMMENT ON COLUMN account.users.email IS '身份提供方返回的可选用户邮箱';
COMMENT ON COLUMN account.users.display_name IS '面向管理界面展示的用户名称';
COMMENT ON COLUMN account.users.status IS '用户访问状态，取值来自 account.user_status';
COMMENT ON COLUMN account.users.created_at IS '本地用户记录首次创建时间';
COMMENT ON COLUMN account.users.updated_at IS '本地用户资料最后更新时间';
COMMENT ON COLUMN account.users.last_login_at IS '最近一次成功认证并同步身份的时间';
COMMENT ON COLUMN account.users.is_super_admin IS '是否为系统唯一超级管理员；该身份不绑定角色或权限并直接绕过权限校验';
COMMENT ON CONSTRAINT users_pkey ON account.users IS '保证每个本地用户具有唯一稳定主键';
COMMENT ON CONSTRAINT users_id_format ON account.users IS '保证用户 ID 固定为 8 位大小写字母或数字';
COMMENT ON CONSTRAINT users_identity_id_unique ON account.users IS '保证一个认证授权身份只对应一个本地用户';
COMMENT ON CONSTRAINT users_identity_id_valid ON account.users IS '保证认证授权身份 ID 非空且长度不超过 255 个字符';
COMMENT ON CONSTRAINT users_username_valid ON account.users IS '登录用户名为空或为不超过 200 个字符的非空文本';
COMMENT ON CONSTRAINT users_email_length ON account.users IS '限制可选邮箱长度不超过 320 个字符';
COMMENT ON CONSTRAINT users_display_name_valid ON account.users IS '保证展示名称非空且长度不超过 200 个字符';
COMMENT ON INDEX account.users_pkey IS '支撑用户稳定主键约束的唯一索引';
COMMENT ON INDEX account.users_identity_id_unique IS '支撑认证授权身份全局唯一约束的索引';

CREATE INDEX users_created_at_id_idx ON account.users (created_at DESC, id DESC);
COMMENT ON INDEX account.users_created_at_id_idx IS '支持按创建时间和用户 ID 稳定倒序分页查询用户';

CREATE INDEX users_status_idx ON account.users (status);
COMMENT ON INDEX account.users_status_idx IS '支持按用户访问状态筛选用户';

CREATE UNIQUE INDEX users_single_super_admin_idx
    ON account.users (is_super_admin)
    WHERE is_super_admin;
COMMENT ON INDEX account.users_single_super_admin_idx IS '保证整个系统最多存在一个内置超级管理员用户';

CREATE TABLE account.system_initialization (
    id SMALLINT NOT NULL DEFAULT 1,
    is_initialized BOOLEAN NOT NULL DEFAULT FALSE,
    initialized_at TIMESTAMPTZ,
    super_admin_user_id VARCHAR(8),
    identity_issuer TEXT,
    CONSTRAINT system_initialization_pkey PRIMARY KEY (id),
    CONSTRAINT system_initialization_singleton CHECK (id = 1),
    CONSTRAINT system_initialization_state_consistent CHECK (
        (NOT is_initialized AND super_admin_user_id IS NULL AND initialized_at IS NULL)
        OR (is_initialized AND super_admin_user_id IS NOT NULL AND initialized_at IS NOT NULL)
    ),
    CONSTRAINT system_initialization_identity_issuer_valid CHECK (
        identity_issuer IS NULL
        OR (BTRIM(identity_issuer) <> '' AND LENGTH(identity_issuer) <= 2048)
    ),
    CONSTRAINT system_initialization_super_admin_user_id_fkey
        FOREIGN KEY (super_admin_user_id) REFERENCES account.users (id) ON DELETE RESTRICT
);

COMMENT ON TABLE account.system_initialization IS '系统一次性初始化状态；单例记录完成后禁止再次进入 setup 流程';
COMMENT ON COLUMN account.system_initialization.id IS '固定为 1 的单例主键';
COMMENT ON COLUMN account.system_initialization.is_initialized IS '系统是否已完成所有当前初始化步骤';
COMMENT ON COLUMN account.system_initialization.initialized_at IS '系统完成初始化的数据库时间';
COMMENT ON COLUMN account.system_initialization.super_admin_user_id IS '初始化时选定的 8 位超级管理员本地用户 ID';
COMMENT ON COLUMN account.system_initialization.identity_issuer IS '当前部署唯一允许使用的规范 OIDC issuer URL；首次启动绑定后永久不可更换';
COMMENT ON CONSTRAINT system_initialization_pkey ON account.system_initialization IS '保证初始化状态单例记录具有稳定主键';
COMMENT ON CONSTRAINT system_initialization_singleton ON account.system_initialization IS '保证初始化状态表只允许固定单例记录';
COMMENT ON CONSTRAINT system_initialization_state_consistent ON account.system_initialization IS '保证完成标记与超级管理员、完成时间同时存在或同时为空';
COMMENT ON CONSTRAINT system_initialization_identity_issuer_valid ON account.system_initialization IS '允许首次绑定前为空；绑定值必须非空且长度不超过 2048 个字符';
COMMENT ON CONSTRAINT system_initialization_super_admin_user_id_fkey ON account.system_initialization IS '防止删除完成系统初始化的超级管理员用户';
COMMENT ON INDEX account.system_initialization_pkey IS '支撑系统初始化单例主键约束的唯一索引';

INSERT INTO account.system_initialization (id, is_initialized)
VALUES (1, FALSE);
