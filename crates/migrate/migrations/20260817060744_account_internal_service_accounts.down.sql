DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM account.users WHERE identity_id IS NULL) THEN
        RAISE EXCEPTION '回滚前必须先移除或重新绑定全部内部服务主体'
            USING ERRCODE = '23502';
    END IF;
END
$$;

ALTER TABLE account.users
    DROP CONSTRAINT users_human_identity_required,
    ALTER COLUMN identity_id SET NOT NULL;

COMMENT ON COLUMN account.users.identity_id IS
    '认证授权服务中与用户对应的稳定唯一 ID';
COMMENT ON CONSTRAINT users_identity_id_valid ON account.users IS
    '保证认证授权身份 ID 非空且长度不超过 255 个字符';
