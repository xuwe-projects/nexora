DROP TRIGGER system_initialization_protect ON account.system_initialization;
DROP FUNCTION account.protect_system_initialization();

DROP TRIGGER user_roles_protect_super_admin ON account.user_roles;
DROP FUNCTION account.protect_super_admin_role_assignment();

DROP TRIGGER users_protect_super_admin ON account.users;
DROP FUNCTION account.protect_super_admin_user();
