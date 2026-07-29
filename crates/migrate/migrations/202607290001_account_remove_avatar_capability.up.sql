DELETE FROM account.role_permissions
USING account.permissions
WHERE account.role_permissions.permission_id = account.permissions.id
  AND account.permissions.key = 'users:avatar.write';

DELETE FROM account.permission_implications
USING account.permissions AS source_permission
WHERE account.permission_implications.permission_id = source_permission.id
  AND source_permission.key = 'users:avatar.write';

DELETE FROM account.permission_implications
USING account.permissions AS implied_permission
WHERE account.permission_implications.implied_permission_id = implied_permission.id
  AND implied_permission.key = 'users:avatar.write';

DELETE FROM account.permissions
WHERE key = 'users:avatar.write';

ALTER TABLE account.users
    DROP CONSTRAINT IF EXISTS users_avatar_url_length,
    DROP COLUMN IF EXISTS avatar_url;
