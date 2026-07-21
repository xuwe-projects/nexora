DELETE FROM account.role_permissions
USING account.permissions
WHERE role_permissions.permission_id = permissions.id
  AND permissions.key = 'users:avatar.write';

DELETE FROM account.permissions
WHERE key = 'users:avatar.write';
