INSERT INTO account.role_permissions (role_id, permission_id)
SELECT roles.id, permissions.id
FROM account.roles AS roles
CROSS JOIN account.permissions AS permissions
WHERE roles.key = 'admin'
  AND roles.is_system = TRUE
ON CONFLICT DO NOTHING;
