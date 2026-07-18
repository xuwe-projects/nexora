ALTER TABLE account.users
    DROP CONSTRAINT users_username_valid,
    DROP COLUMN username;
