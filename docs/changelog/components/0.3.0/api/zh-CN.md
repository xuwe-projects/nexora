## Account 宿主集成

- `Server::account()` 在初始化后向宿主提供可克隆的 Account 句柄，自定义 Axum State 通过
  `FromRef` 即可复用 `AuthenticatedUser` 与 `Authorized<P>`。
- 新增 `create_user_with_roles`，并让 `POST /users` 支持在开通外部身份时原子建立初始角色
  关系；非空角色集合同时要求 `users:provision` 与 `users:roles.write`。
- `role_ids` 缺省时保持 JSON 兼容，但 0.3.0 会补授不带权限的内置 `member` 角色，因此
  有效权限仍为空，角色关系快照则不同于 0.2.0；本版本没有新增或修改数据库 DDL。
