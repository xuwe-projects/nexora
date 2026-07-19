## ZITADEL 客户门户开通

- 新增 `ZitadelProvisioningClient`，服务端可为客户动态创建或绑定 ZITADEL Organization、
  portal Project Grant、人类用户和 Project authorization。
- 新增独立 OIDC resource server verifier，portal/openapi 可以校验自己的 audience，并从
  `VerifiedIdentity.organization` 映射业务租户。
- 下游仍只使用 Nexora 的 `server` / `desktop` feature；默认 Account 用户管理不会混入 portal 用户。
