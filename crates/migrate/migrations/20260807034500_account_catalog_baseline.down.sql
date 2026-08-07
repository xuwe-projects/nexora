DELETE FROM account.role_permissions
USING account.roles
WHERE account.role_permissions.role_id = account.roles.id
  AND account.roles.key IN ('admin', 'auditor', 'member', 'portal_admin');

DELETE FROM account.permission_implications
USING account.permissions AS source_permission
WHERE account.permission_implications.permission_id = source_permission.id
  AND source_permission.key IN (
      'users:read',
      'users:roles.write',
      'users:status.write',
      'users:provision',
      'roles:read',
      'roles:write',
      'permissions:read'
  );

DELETE FROM account.roles
WHERE key IN ('admin', 'auditor', 'member', 'portal_admin');

DELETE FROM account.permissions
WHERE key IN (
    'users:read',
    'users:roles.write',
    'users:status.write',
    'users:provision',
    'roles:read',
    'roles:write',
    'permissions:read'
);
