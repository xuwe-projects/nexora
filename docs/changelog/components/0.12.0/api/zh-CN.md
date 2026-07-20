## Account 初始密码创建用户

- Account 创建人类用户时支持携带 `initial_password` 和 `require_password_change`。
- `ProvisionUserRequest` 新增必填初始密码字段，HTTP 请求体与 OpenAPI 契约同步更新。
- ZITADEL 创建用户路径会写入初始密码，Nexora 本地数据库、日志和错误详情不保存明文密码。

