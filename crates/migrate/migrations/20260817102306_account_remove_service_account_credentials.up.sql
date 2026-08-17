-- Add up migration script here
DELETE FROM account.permissions
WHERE key IN (
    'service_accounts:credentials.read',
    'service_accounts:credentials.write'
);

DROP TABLE account.service_account_credentials;
DROP TYPE account.service_account_credential_source;
DROP TYPE account.service_account_credential_status;
DROP TYPE account.service_account_credential_type;
