DELETE FROM account.user_roles
USING account.roles
WHERE user_roles.role_id = roles.id
  AND roles.key = 'portal_admin'
  AND roles.owner = 'IMES'
  AND roles.is_system = TRUE;

DELETE FROM account.roles
WHERE key = 'portal_admin'
  AND owner = 'IMES'
  AND is_system = TRUE;

DROP INDEX account.roles_owner_key_idx;

ALTER TABLE account.roles
    DROP CONSTRAINT roles_owner_valid,
    DROP COLUMN owner;
