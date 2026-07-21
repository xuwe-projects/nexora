INSERT INTO account.permissions (key, name, description)
VALUES (
    'users:avatar.write',
    '管理用户头像',
    '上传、更新或清空用户头像 URL，并同步到身份目录'
)
ON CONFLICT (key) DO UPDATE
SET name = EXCLUDED.name,
    description = EXCLUDED.description;

INSERT INTO account.role_permissions (role_id, permission_id)
SELECT roles.id, permissions.id
FROM account.roles AS roles
JOIN account.permissions AS permissions
    ON permissions.key = 'users:avatar.write'
WHERE roles.key = 'admin'
ON CONFLICT DO NOTHING;
