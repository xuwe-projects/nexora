---
title: HTTP API 完整参考
order: 2
---

# HTTP API 完整参考

本页描述 `nexora::server::Server::routers()` 暴露的 Setup 与 Account HTTP 接口，以及生成的
服务端示例提供的 `/health`。机器可读契约见 [OpenAPI 3.1](../openapi.yaml)。

## 路由来源与组合边界

| 来源 | 路由 | 是否属于 `Server::routers()` |
| --- | --- | --- |
| Nexora Setup | `GET /setup`、`POST /setup`、`POST /setup/complete` | 是 |
| Nexora Account | `/me`、`/users`、`/roles`、`/permissions` 及其子资源 | 是 |
| 生成的宿主应用 | `GET /health` | 否；由应用的 `routes::routers()` 提供 |
| 宿主业务 | 应用自行声明 | 否 |

`Server::routers()` 不绑定端口、不增加 `/api/v1` 前缀，也不接管宿主 State。应用通过
`Router::merge` 决定是否公开这些路由以及中间件边界。

## 协议约定

### 基础 URL 与内容类型

- 示例基础 URL：`http://127.0.0.1:3000`；实际 scheme、host 和 port 由宿主部署决定。
- Account API 请求和响应使用 `application/json`，字段名统一为 `snake_case`。
- 默认 Setup 是 SSR HTML 流程，提交使用 `application/x-www-form-urlencoded`，响应使用
  `text/html; charset=utf-8`。
- 所有时间字段均为有符号 Unix 秒整数，不是 RFC 3339 字符串或毫秒时间戳。
- Account 路由没有路径版本前缀。

### Bearer 认证

除 Setup 与宿主健康检查外，所有接口都要求：

```http
Authorization: Bearer <access_token>
```

验证顺序固定为：Bearer 头格式、token 签名/issuer/audience/有效期、本地用户是否已开通、
用户是否停用、目标权限。身份绑定只使用当前部署 OIDC issuer 范围内稳定的
`identity_id`；`username` 仅作为可更新的登录名元数据，不参与认证查找。

超级管理员直接通过权限判断；普通用户必须通过角色拥有下表所列权限。

### 权限矩阵

| 方法与路径 | 所需权限 |
| --- | --- |
| `GET /me` | 只要求已认证 |
| `GET /users`、`GET /users/{user_id}` | `users:read` |
| `POST /users` | `users:provision`；`role_ids` 非空时还要 `users:roles.write` |
| `PATCH /users/{user_id}` | `users:status.write` |
| `PUT /users/{user_id}/roles` | `users:roles.write` |
| `GET /roles`、`GET /roles/{role_id}` | `roles:read` |
| `POST /roles`、`PATCH /roles/{role_id}`、`DELETE /roles/{role_id}` | `roles:write` |
| `PUT /roles/{role_id}/permissions` | `roles:write` |
| `GET /permissions` | `permissions:read` |

## 通用响应模型

### User

| 字段 | 类型 | 可空 | 说明 |
| --- | --- | --- | --- |
| `id` | string | 否 | Nexora 本地生成的 8 位大小写字母与数字用户 ID |
| `identity_id` | string | 否 | 当前 OIDC issuer 中稳定唯一的 subject |
| `username` | string | 是 | 身份提供方登录用户名；不会替代 `identity_id` |
| `email` | string | 是 | 展示邮箱 |
| `display_name` | string | 否 | 展示名称，最多 200 个字符 |
| `avatar_url` | string | 是 | 头像 URL |
| `status` | enum | 否 | `active` 或 `suspended` |
| `is_super_admin` | boolean | 否 | 是否为系统唯一、不可变的内置超级管理员 |
| `created_at` | int64 | 否 | 创建时间，Unix 秒 |
| `updated_at` | int64 | 否 | 资料更新时间，Unix 秒 |
| `last_login_at` | int64 | 否 | 最近认证并同步身份的时间，Unix 秒 |

```json
{
  "id": "A1b2C3d4",
  "identity_id": "279693210507280451",
  "username": "lin.chen",
  "email": "lin@example.com",
  "display_name": "陈林",
  "avatar_url": "https://id.example.com/avatar/279693210507280451",
  "status": "active",
  "is_super_admin": false,
  "created_at": 1784304000,
  "updated_at": 1784304000,
  "last_login_at": 1784304000
}
```

### Permission

| 字段 | 类型 | 可空 | 说明 |
| --- | --- | --- | --- |
| `id` | int64 | 否 | PostgreSQL BIGSERIAL ID |
| `key` | string | 否 | 稳定权限键，例如 `users:read` |
| `name` | string | 否 | 展示名称 |
| `description` | string | 是 | 权限说明 |

### Role

| 字段 | 类型 | 可空 | 说明 |
| --- | --- | --- | --- |
| `id` | int64 | 否 | PostgreSQL BIGSERIAL ID |
| `key` | string | 否 | 稳定角色键 |
| `name` | string | 否 | 展示名称，1 至 100 个字符 |
| `description` | string | 是 | 说明，最多 1000 个字符 |
| `is_system` | boolean | 否 | 系统角色为 `true`，不能修改或删除 |
| `permissions` | Permission[] | 否 | 角色直接包含的权限 |
| `created_at` | int64 | 否 | 创建时间，Unix 秒 |
| `updated_at` | int64 | 否 | 更新时间，Unix 秒 |

### AccessProfile

`AccessProfile` 是用户当前授权快照：

```json
{
  "user": { "id": "A1b2C3d4", "identity_id": "279693210507280451", "username": "lin.chen", "email": null, "display_name": "陈林", "avatar_url": null, "status": "active", "is_super_admin": false, "created_at": 1784304000, "updated_at": 1784304000, "last_login_at": 1784304000 },
  "roles": [
    { "id": 2, "key": "member", "name": "成员", "description": null, "is_system": true, "permissions": [], "created_at": 1784304000, "updated_at": 1784304000 }
  ],
  "permissions": ["users:read"]
}
```

`roles` 只包含直接角色；`permissions` 是角色合并、去重后的稳定权限键。超级管理员可能没有
角色或权限记录，但授权判断仍会通过。

### 分页与集合

用户集合使用页码分页：

```json
{
  "items": [],
  "page": { "number": 1, "size": 25, "total": 0 }
}
```

`page` 默认 `1` 且必须大于零；`page_size` 默认 `25`。服务端把有效的 `page_size` 限制在
`1..=100`，大于 100 时实际返回的 `page.size` 为 100。角色和权限集合不分页，统一返回
`{"items": [...]}`。

## 通用错误模型

所有 JSON API 错误使用同一个 envelope：

```json
{
  "error": {
    "code": "validation_failed",
    "message": "角色名称必须为 1 到 100 个字符",
    "details": { "field": "name" },
    "request_id": "req_01JZ7V4M1K8F9Q2T6Y3B5C7D8E"
  }
}
```

| HTTP | `error.code` | 含义 |
| --- | --- | --- |
| 400 | `invalid_json_body`、`invalid_path_parameter`、`invalid_query_parameter` | JSON 语法、path 或 query 无法解析 |
| 401 | `missing_access_token` | 缺少 Authorization 头 |
| 401 | `invalid_access_token` | Bearer 格式错误，或 token 无效/过期 |
| 401 | `invalid_identity`、`invalid_identity_issuer` | token 身份字段不完整，或 issuer 不属于当前部署 |
| 403 | `account_not_registered` | token 有效，但本地账号尚未开通 |
| 403 | `account_suspended` | 本地账号已停用 |
| 403 | `permission_denied` | 已认证但缺少接口权限 |
| 404 | `resource_not_found` | 用户、角色或权限关联不存在 |
| 404 | `route_not_found` | 路由不存在；需要宿主应用安装统一 fallback |
| 405 | `method_not_allowed` | 路径存在但方法不支持；需要宿主应用安装统一 fallback |
| 409 | `user_already_provisioned` | `identity_id` 已开通 |
| 409 | `role_key_exists` | 角色键已存在 |
| 409 | `role_in_use` | 删除的角色仍被用户引用 |
| 409 | `system_role_immutable` | 尝试修改或删除系统角色 |
| 409 | `last_administrator` | 操作会移除最后一个启用管理员 |
| 409 | `super_administrator_immutable` | 尝试修改超级管理员状态或角色 |
| 422 | `validation_failed` | 业务字段校验失败，`details.field` 指向字段 |
| 500 | `internal_error` | 数据库或内部操作失败；不会返回 SQL 或堆栈 |
| 503 | `identity_issuer_not_bound` | 部署尚未完成 issuer 绑定 |
| 503 | `identity_provider_unavailable` | OIDC Provider/JWKS 暂时不可用 |

`401` 响应包含 `WWW-Authenticate: Bearer`。如果宿主安装 Nexora 的统一 HTTP 中间件，响应还
会包含 `x-request-id`，并优先沿用格式有效的请求头值；错误正文的 `request_id` 与该值一致。

## 当前用户

### `GET /me`

验证 Bearer token、本地账号和状态，然后通过配置的 ZITADEL UserService v2 gRPC 按
`identity_id` 读取最新人类用户，原子同步 `username`、邮箱、展示名、头像与最近登录时间，
最后返回 `AccessProfile`。该接口不要求额外权限，也不会为陌生身份自动创建用户；目录中
不存在该身份时返回 `404 identity_not_found`，目录暂时不可用时返回
`503 identity_provider_unavailable`。

```bash
curl -H "Authorization: Bearer $ACCESS_TOKEN" http://127.0.0.1:3000/me
```

成功：`200 application/json`，正文为 `AccessProfile`。常见失败：`401`、
`403 account_not_registered`、`403 account_suspended`、`503`。

## 用户接口

### `GET /users`

权限：`users:read`。

| query | 类型 | 默认值 | 约束 |
| --- | --- | --- | --- |
| `page` | u32 | `1` | 从 1 开始；0 返回 `422 validation_failed` |
| `page_size` | u32 | `25` | 0 按 1 处理，大于 100 按 100 处理 |

```bash
curl -H "Authorization: Bearer $ACCESS_TOKEN" \
  "http://127.0.0.1:3000/users?page=1&page_size=25"
```

成功：`200`，正文为用户分页。未知 query 字段由 `deny_unknown_fields` 拒绝，并返回
`400 invalid_query_parameter`。

`User` 响应包含 `user_type`：`human` 表示人员用户，`service_account` 表示服务账号。服务账号
用于系统集成、任务或服务间调用，默认管理界面会把它们标记为不可操作。

### `POST /users`

在服务端后台调用 ZITADEL UserService v2 gRPC 创建人类用户，并把返回的 identity ID 绑定到
Nexora Account。权限：`users:provision`；`role_ids` 非空时额外要求 `users:roles.write`。
Provider 创建成功后，本地用户、内置 `member` 角色和初始角色关联在同一数据库事务中完成；
本地事务失败时服务端会尽力删除刚创建的 Provider 用户作为补偿。

| body 字段 | 类型 | 必填 | 约束 |
| --- | --- | --- | --- |
| `username` | string | 是 | trim 后 1 至 200 个字符；在 ZITADEL Organization 中唯一 |
| `given_name` | string | 是 | trim 后 1 至 200 个字符 |
| `family_name` | string | 是 | trim 后 1 至 200 个字符 |
| `email` | string | 是 | 无空白、包含有效 `@` 与域名，最多 200 个字符；创建时请求 ZITADEL 发送验证邮件 |
| `display_name` | string/null | 否 | trim 后最多 200 个字符；省略时由名字与姓氏生成 |
| `role_ids` | int64[] | 否 | 默认 `[]`，最多 64 项；服务端去重 |

请求 DTO 拒绝未知字段。示例：

```json
{
  "username": "lin.chen",
  "given_name": "林",
  "family_name": "陈",
  "email": "lin@example.com",
  "display_name": "陈林",
  "role_ids": [3, 5]
}
```

成功：`201 Created`，`Location: /users/{id}`，正文为包含 ZITADEL identity ID 的新 `User`。
由该接口创建的用户固定为 `user_type = "human"`。
不存在的角色或授权人返回 `404`；ZITADEL 中用户名或邮箱冲突返回
`409 identity_already_exists`；字段错误返回 `422`；Provider 不可用返回
`503 identity_provider_unavailable`。本地事务失败时不会留下部分用户或角色关系，并触发
Provider 删除补偿；补偿本身失败只记录脱敏服务端日志，不会覆盖原始错误。

### `GET /users/{user_id}`

权限：`users:read`。`user_id` 是 8 位本地用户 ID。成功返回 `200 AccessProfile`；不存在时
返回 `404 resource_not_found`。

### `PATCH /users/{user_id}`

权限：`users:status.write`。只允许修改访问状态：

```json
{ "status": "suspended" }
```

`status` 只能为 `active` 或 `suspended`，未知字段被拒绝。成功返回 `200 User`。不能修改
超级管理员或服务账号；也不能停用会导致系统失去最后一个启用管理员的用户，对应返回
`409 super_administrator_immutable`、`409 service_account_immutable` 或 `409 last_administrator`。

### `PUT /users/{user_id}/roles`

权限：`users:roles.write`。`role_ids` 表示替换后的完整业务角色集合，不是增量追加：

```json
{ "role_ids": [3, 5] }
```

最多 64 项，服务端去重；空数组会移除业务角色，但保留内置 `member`。成功返回更新后的
`200 AccessProfile`。目标用户、角色或授权人不存在返回 `404`；超级管理员和服务账号不可挂载
角色，且不能通过此操作移除最后一个管理员。

## 角色接口

### `GET /roles`

权限：`roles:read`。成功返回：

```json
{ "items": [{ "id": 2, "key": "member", "name": "成员", "description": null, "is_system": true, "permissions": [], "created_at": 1784304000, "updated_at": 1784304000 }] }
```

### `POST /roles`

权限：`roles:write`。

| body 字段 | 类型 | 必填 | 约束 |
| --- | --- | --- | --- |
| `key` | string | 是 | 2 至 64 位；小写字母开头，其余为小写字母、数字、`.`、`_`、`-` |
| `name` | string | 是 | trim 后 1 至 100 个字符 |
| `description` | string/null | 否 | 最多 1000 个字符，可省略或为 `null` |
| `permission_ids` | int64[] | 否 | 默认 `[]`，最多 256 项，服务端去重 |

```json
{
  "key": "support_agent",
  "name": "客服",
  "description": "处理用户支持请求",
  "permission_ids": [1, 2]
}
```

成功：`201 Created`，`Location: /roles/{id}`，正文为 `Role`。重复 key 返回
`409 role_key_exists`；权限不存在返回 `404`；字段不合法返回 `422`。

### `GET /roles/{role_id}`

权限：`roles:read`。`role_id` 是大于零的 int64。成功返回 `200 Role`；不存在返回 `404`；
无法解析的 path 返回 `400 invalid_path_parameter`。

### `PATCH /roles/{role_id}`

权限：`roles:write`。至少提供一个字段：

```json
{ "name": "高级客服", "description": null }
```

- 缺少 `name` 表示保持原名称；`name` 不接受 `null`。
- 缺少 `description` 表示保持原说明；显式 `null` 表示清空；字符串表示替换。
- `key` 不可修改，未知字段被拒绝。

成功返回 `200 Role`。空对象返回 `422`；系统角色返回
`409 system_role_immutable`；并发状态不再允许修改时可能返回 `409 role_not_modified`。

### `DELETE /roles/{role_id}`

权限：`roles:write`。成功返回 `204 No Content`，无正文。系统角色不可删除；仍被用户引用的
自定义角色返回 `409 role_in_use`；不存在返回 `404`。

### `PUT /roles/{role_id}/permissions`

权限：`roles:write`。`permission_ids` 是替换后的完整直接权限集合：

```json
{ "permission_ids": [1, 2, 7] }
```

最多 256 项，服务端去重；空数组清空自定义角色的直接权限。成功返回 `200 Role`。系统角色
不可修改；角色或权限不存在返回 `404`。

## 权限目录

### `GET /permissions`

权限：`permissions:read`。返回内置与宿主注册权限的完整、不分页目录。宿主每次通过
`create_permissions` 或 `Account::register_permissions` 注册新权限时，Account 都会在同一事务
中把这些权限补入系统管理员角色；升级迁移也会回填已有权限，避免管理员随着权限目录扩展而
失去管理能力：

```json
{
  "items": [
    { "id": 1, "key": "users:read", "name": "查看用户", "description": "查看用户及授权详情" }
  ]
}
```

HTTP API 只读。宿主通过 `nexora::server::create_permissions` 或
`Account::register_permissions` 幂等注册应用权限。

## 默认 Setup SSR 接口

Setup 路由只在系统尚未初始化时可用。初始化完成后，三个入口都永久返回 `404`。默认页面
响应包含 `Cache-Control: no-store`、严格 CSP 与 `X-Content-Type-Options: nosniff`。

### `GET /setup`

无入参。未初始化时返回 `200 text/html` 的 secret 输入页；已初始化返回 `404 text/html`；
读取初始化状态失败返回 `500 text/html`。

### `POST /setup`

Content-Type：`application/x-www-form-urlencoded`。

| form 字段 | 必填 | 说明 |
| --- | --- | --- |
| `secret` | 是 | 服务端配置的 setup secret；默认 HTML 输入限制 1024 字符 |

secret 错误返回 `401` 并重新显示表单。成功后，服务端通过 ZITADEL UserService 读取启用的
人类用户，创建有效期 15 分钟的一次性 setup token，并返回 `200` 用户选择页。没有可选用户
返回 `503`；目录或数据库失败返回 `500`。

### `POST /setup/complete`

Content-Type：`application/x-www-form-urlencoded`。

| form 字段 | 必填 | 说明 |
| --- | --- | --- |
| `setup_token` | 是 | 解锁步骤签发的 64 位十六进制一次性 token，15 分钟有效 |
| `identity_id` | 是 | 选择的 ZITADEL 人类用户 ID |

框架会再次从目录核对用户，不信任浏览器提交的展示资料。token 无效或过期返回 `401`；用户
不存在、已停用或不是人类用户返回 `422`；完成后返回 `200 text/html`，清除会话并把该身份
设为唯一超级管理员。系统角色同步、数据库事务或目录调用失败返回 `500`。

自定义 `Setup` 可以替换 form DTO 与响应表现层，但框架仍固定执行 secret、短期 token、
目录二次核对、系统角色同步和超级管理员事务；详见 [Rust 服务端 API](./rust-api)。

## 宿主健康检查

### `GET /health`

`/health` 不属于 `Server::routers()`。仓库中的完整示例在 PostgreSQL `SELECT 1` 成功时返回：

```json
{ "status": "ok" }
```

失败返回 `503 database_unavailable`。CLI 生成的最小服务端模板只保证 `200` 或 `503` 状态，
不会承诺 JSON 正文；宿主可以按部署探针规范替换它。OpenAPI 描述的是仓库完整示例的 JSON
契约。
