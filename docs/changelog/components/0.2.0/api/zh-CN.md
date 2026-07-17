## Account 宿主管理

- 新增 pool-first 用户、权限、角色创建，以及角色权限和用户角色完整关联替换能力。
- 所有写入复用 Account 校验、Store、事务和宿主唯一 `PgPool`，不在 API 内隐式授权。

## 统一迁移

- 新增 `nexora::server::migrations()` 导出框架迁移清单。
- `Server::initialize` 不再执行迁移；应用必须拒绝跨来源版本冲突，并使用唯一 SQLx
  `Migrator` 先迁移、后初始化。
- 删除 `Server::migrate` 与 `nexora::server::migrate`，升级前请阅读 0.2.0 完整回滚说明。
