ALTER TABLE account.roles
    ADD COLUMN owner TEXT NOT NULL DEFAULT 'IMES',
    ADD CONSTRAINT roles_owner_valid CHECK (BTRIM(owner) <> '' AND LENGTH(owner) <= 200);

COMMENT ON COLUMN account.roles.owner IS '角色所属范围；IMES 表示后台系统角色和后台自定义角色，其他值由宿主作为客户或业务范围 ID';
COMMENT ON CONSTRAINT roles_owner_valid ON account.roles IS '保证角色所属范围非空且长度不超过 200 个字符';

CREATE INDEX roles_owner_key_idx ON account.roles (owner, key);
COMMENT ON INDEX account.roles_owner_key_idx IS '支持按角色所属范围查询角色目录并按全局唯一角色键稳定排序';

INSERT INTO account.roles (key, owner, name, description, is_system)
VALUES (
    'portal_admin',
    'IMES',
    '门户管理员',
    '全局客户门户管理员角色；权限由宿主应用启动时同步',
    TRUE
)
ON CONFLICT (key) DO UPDATE
SET owner = 'IMES',
    name = EXCLUDED.name,
    description = EXCLUDED.description,
    is_system = TRUE,
    updated_at = NOW();
