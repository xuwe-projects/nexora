ALTER TABLE account.users
    ADD COLUMN username TEXT,
    ADD CONSTRAINT users_username_valid
        CHECK (
            username IS NULL
            OR (BTRIM(username) <> '' AND CHAR_LENGTH(username) <= 200)
        );

COMMENT ON COLUMN account.users.username IS '认证授权服务中的可选登录用户名；身份绑定仍以稳定 identity_id 为准';
COMMENT ON CONSTRAINT users_username_valid ON account.users IS '登录用户名为空或为不超过 200 个字符的非空文本';
