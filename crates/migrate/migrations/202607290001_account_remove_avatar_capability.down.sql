ALTER TABLE account.users
    ADD COLUMN IF NOT EXISTS avatar_url TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'users_avatar_url_length'
          AND conrelid = 'account.users'::regclass
    ) THEN
        ALTER TABLE account.users
            ADD CONSTRAINT users_avatar_url_length
            CHECK (avatar_url IS NULL OR LENGTH(avatar_url) <= 2048);
    END IF;
END
$$;

COMMENT ON COLUMN account.users.avatar_url IS '身份提供方返回的可选头像 URL';
COMMENT ON CONSTRAINT users_avatar_url_length ON account.users IS '限制可选头像 URL 长度不超过 2048 个字符';

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
