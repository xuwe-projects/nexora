## 用户类型响应字段

- `User` / `UserResponse` 新增 `user_type`，取值为 `human` 或 `service_account`。
- 服务账号用于系统集成、任务或服务间调用；默认 Account API 现在拒绝修改服务账号状态和直接
  角色，返回 `409 service_account_immutable`。
- HTTP API 路径、数据库 schema 与迁移历史未变化；严格 DTO 客户端需要同步新增字段。
