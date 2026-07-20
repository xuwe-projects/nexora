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

CREATE INDEX permission_implications_implied_permission_id_idx
    ON account.permission_implications (implied_permission_id, permission_id);
COMMENT ON INDEX account.permission_implications_implied_permission_id_idx IS '支持从被蕴含权限反向查询上游权限';

INSERT INTO account.permission_implications (permission_id, implied_permission_id)
SELECT source.id, implied.id
FROM account.permissions AS source
JOIN account.permissions AS implied
    ON implied.key = CASE source.key
        WHEN 'users:roles.write' THEN 'users:read'
        WHEN 'users:status.write' THEN 'users:read'
        WHEN 'users:provision' THEN 'users:read'
        WHEN 'roles:write' THEN 'roles:read'
        ELSE NULL
    END
WHERE source.key IN ('users:roles.write', 'users:status.write', 'users:provision', 'roles:write')
ON CONFLICT DO NOTHING;

WITH RECURSIVE expanded(role_id, permission_id, path) AS (
    SELECT role_id, permission_id, ARRAY[permission_id]
    FROM account.role_permissions

    UNION

    SELECT expanded.role_id,
           implication.implied_permission_id,
           expanded.path || implication.implied_permission_id
    FROM expanded
    JOIN account.permission_implications AS implication
        ON implication.permission_id = expanded.permission_id
    WHERE NOT implication.implied_permission_id = ANY(expanded.path)
)
INSERT INTO account.role_permissions (role_id, permission_id)
SELECT DISTINCT role_id, permission_id
FROM expanded
ON CONFLICT DO NOTHING;
