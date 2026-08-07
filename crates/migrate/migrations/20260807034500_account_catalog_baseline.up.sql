INSERT INTO account.permissions (key, name, description)
VALUES
    ('users:read', '查看用户', '查看用户列表、用户详情及其角色'),
    ('users:roles.write', '管理用户角色', '为用户授予或撤销角色'),
    ('users:status.write', '管理用户状态', '启用或停用用户访问'),
    ('users:provision', '开通用户', '把经过管理员确认的 OIDC subject 显式开通为本地用户'),
    ('roles:read', '查看角色', '查看角色及角色包含的权限'),
    ('roles:write', '管理角色', '创建、修改、删除非系统角色并配置权限'),
    ('permissions:read', '查看权限', '查看系统支持的权限目录');

INSERT INTO account.roles (key, owner, name, description, is_system)
VALUES
    ('admin', 'IMES', '系统管理员', '拥有系统管理权限；作为普通用户角色仍然完整执行权限校验', TRUE),
    ('auditor', 'IMES', '审计员', '只读查看用户、角色和权限', TRUE),
    ('member', 'IMES', '普通成员', '默认登录角色，不包含后台管理权限', TRUE),
    ('portal_admin', 'IMES', '门户管理员', '全局客户门户管理员角色；权限由宿主应用启动时同步', TRUE);

INSERT INTO account.role_permissions (role_id, permission_id)
SELECT roles.id, permissions.id
FROM account.roles AS roles
CROSS JOIN account.permissions AS permissions
WHERE roles.key = 'admin';

INSERT INTO account.role_permissions (role_id, permission_id)
SELECT roles.id, permissions.id
FROM account.roles AS roles
JOIN account.permissions AS permissions
    ON permissions.key IN ('users:read', 'roles:read', 'permissions:read')
WHERE roles.key = 'auditor';

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
WHERE source.key IN ('users:roles.write', 'users:status.write', 'users:provision', 'roles:write');
