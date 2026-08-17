ALTER TABLE account.users
    ALTER COLUMN identity_id DROP NOT NULL,
    ADD CONSTRAINT users_human_identity_required
        CHECK (user_type <> 'human' OR identity_id IS NOT NULL);

COMMENT ON COLUMN account.users.identity_id IS
    '认证授权服务中与用户对应的稳定唯一 ID；仅不参与认证的内部服务主体允许为空';
COMMENT ON CONSTRAINT users_identity_id_valid ON account.users IS
    '存在认证授权身份 ID 时，保证其非空白且长度不超过 255 个字符';
COMMENT ON CONSTRAINT users_human_identity_required ON account.users IS
    '人员账号必须绑定认证授权身份；identity_id 为空只表示不参与认证的内部服务主体';
