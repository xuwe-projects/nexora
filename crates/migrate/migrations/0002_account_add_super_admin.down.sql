DROP TRIGGER IF EXISTS user_roles_protect_super_admin ON account.user_roles;
DROP FUNCTION IF EXISTS account.protect_super_admin_role_assignment();

DROP TRIGGER IF EXISTS users_protect_super_admin ON account.users;
DROP FUNCTION IF EXISTS account.protect_super_admin_user();

DELETE FROM account.user_roles AS user_roles
USING account.roles AS roles
WHERE user_roles.role_id = roles.id
  AND roles.key = 'super-administrator';

DELETE FROM account.role_permissions AS role_permissions
USING account.roles AS roles
WHERE role_permissions.role_id = roles.id
  AND roles.key = 'super-administrator';

DELETE FROM account.roles WHERE key = 'super-administrator';

DROP INDEX IF EXISTS account.users_single_super_admin_idx;

ALTER TABLE account.users DROP COLUMN IF EXISTS is_super_admin;
