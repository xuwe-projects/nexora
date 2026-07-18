## 登录用户名绑定

- 用户开通请求与用户响应新增可选 `username`，旧 JSON 客户端省略字段时继续兼容。
- OIDC 登录会同步 `preferred_username`，稳定身份绑定仍使用 `identity_id`。
- 新增迁移 6，为 `account.users` 增加可空登录用户名列；升级时先迁移再部署 0.4.0 代码。
- 新增中英文 HTTP 与 Rust 服务端完整参考，并让 OpenAPI 3.1 覆盖默认 Setup SSR 表单、
  Account 路由权限、请求响应模型和稳定错误契约。
