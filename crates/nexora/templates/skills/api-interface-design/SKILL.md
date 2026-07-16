---
name: api-interface-design
description: 用于设计、实现或审查非 SSR HTTP API，规范资源建模、REST URI、HTTP 方法、状态码、请求响应模型、snake_case wire 命名、Unix 秒时间戳、枚举格式、分页筛选、错误格式、版本管理和接口测试。服务端渲染 HTML 的页面路由不强制套用本规范；非 SSR API 默认按资源导向的 REST 方式设计。
---

# API 接口规范

## 适用边界

- 先判断目标是 SSR 页面路由还是非 SSR API。
- 返回 HTML 并由服务端完成页面渲染的 SSR 路由，遵循对应 Web 页面与表单交互规范，不要机械套用 JSON REST 结构。
- 提供给桌面端、移动端、前端 SPA、第三方客户端或服务间调用的非 SSR HTTP API，默认使用本规范。
- WebSocket、流式传输或明确采用 RPC 的接口可以使用相应协议，但普通资源管理接口仍保持 REST 语义。

## 设计流程

1. 识别业务对象及其稳定边界，把它们建模为资源。
2. 定义集合资源、单个资源和真实从属关系对应的 URI。
3. 定义集合摘要、资源详情、创建请求和更新请求的表示模型。
4. 将业务操作映射到正确的 HTTP 方法、状态码和幂等语义。
5. 补充分页、筛选、排序、错误、安全、版本和测试约束。

## 资源与 URI

- URI 使用名词表达资源，不要把动作动词放进普通资源路径。
- 集合资源使用复数名词，例如 `/projects`；单个资源使用稳定标识，例如 `/projects/{project_id}`。
- 只有存在明确从属关系时才使用子资源，例如 `/projects/{project_id}/tasks`。
- 路径层级保持简短，通常不要超过三层资源关系；复杂查询使用 query 参数，不要无限嵌套 URI。
- URI 静态路径段使用小写和连字符；path 参数占位符使用 `snake_case`，例如 `/project-members/{user_id}`。
- 禁止 `/getProjects`、`/createProject`、`/projects/delete`、`/projects?action=list` 等动作式资源路径。
- 无法自然表示为资源状态变化的命令，才使用显式 action 子资源，例如 `POST /projects/{project_id}/actions/archive`，并说明不能使用普通资源方法的原因。

## HTTP 方法

| 场景 | 方法与路径 | 成功响应 |
| --- | --- | --- |
| 查询集合 | `GET /resources` | `200 OK` |
| 查询单项 | `GET /resources/{id}` | `200 OK` |
| 创建资源 | `POST /resources` | `201 Created`，同时返回 `Location` |
| 完整替换 | `PUT /resources/{id}` | `200 OK` 或 `204 No Content` |
| 局部更新 | `PATCH /resources/{id}` | `200 OK` 或 `204 No Content` |
| 同步删除 | `DELETE /resources/{id}` | `204 No Content` 或 `200 OK` |
| 异步处理 | 对应方法 | `202 Accepted`，返回可查询的任务资源 |

- `GET` 不产生业务状态变更。
- `PUT` 和 `DELETE` 保持幂等；重复执行相同请求应得到一致的资源状态。
- `POST` 用于创建资源或确实不幂等的命令。重试可能导致重复创建时，支持并验证幂等键。
- 删除已经不存在的资源时，在整个 API 中统一采用 `204` 或 `404` 语义，不要按接口随意变化。

## 请求与响应模型

- 默认使用 `application/json`，除非文件、流或第三方协议明确需要其他媒体类型。
- 集合响应只返回列表展示需要的摘要字段；单项响应可以返回完整详情和相关资源链接。
- 同一资源出现在顶级集合和子资源集合时，字段含义保持一致，不要为不同入口创造相互冲突的表示。
- 创建请求不包含由服务端生成的 ID、时间戳或审计字段。
- 请求 DTO、响应 DTO、数据库实体和内部领域模型按职责分离，不要直接把数据库行结构暴露为公开 API。
- 多端共享的请求与响应模型放入轻量 `contracts`、`models` 或具体业务 crate，避免客户端依赖整个 API 服务 crate。
- 所有 HTTP 请求和响应时间字段使用有符号 Unix 秒时间戳，以 JSON integer 传输；禁止使用 RFC 3339 字符串或毫秒、微秒、纳秒时间戳。
- 金额和精度敏感数字使用不会丢失语义的稳定格式，不要用浮点数表达货币金额。

## Wire 命名、时间与枚举

- JSON 字段、query 参数、path 参数和 form 字段统一使用 `snake_case`，例如 `user_id`、`page_size`、`created_at`；禁止使用 `userId`、`UserID` 或 `user-id` 作为参数名。
- 标准 HTTP header 名称遵循协议既有写法，不强制改成 `snake_case`。
- HTTP 枚举值统一使用小写 `snake_case`，例如 `pending_review`、`in_progress`；禁止直接暴露 Rust 的 `PendingReview` 或使用数字魔法值。
- 时间戳统一使用 `i64` 语义，单位固定为秒；服务端接收时间戳时校验合法范围，响应时从内部时间类型显式转换，禁止根据数值大小猜测单位。
- 时间字段按业务语义命名为 `created_at`、`updated_at`、`expires_at`、`scheduled_at`；不使用 `created_at_ms`，也不使用含糊的 `time`。

请求与响应示例：

```json
{
  "user_id": "019f6046-8e3b-73d2-86c8-c56155c84259",
  "status": "pending_review",
  "expires_at": 1784044800
}
```

集合响应示例：

```json
{
  "items": [],
  "page": {
    "next_cursor": null,
    "has_more": false
  }
}
```

## 分页、筛选与排序

- 大型或持续变化的集合优先使用 `cursor` + `limit`；稳定的后台管理列表可以使用 `page` + `page_size`。
- 同一资源集合只采用一种分页模型，不要同时混用 offset、page 和 cursor。
- 筛选、搜索和排序使用 query 参数，例如 `?status=active&sort=-created_at`。
- 服务端限制最大 `limit` 或 `page_size`，并对未知筛选字段和非法排序字段返回明确的客户端错误。
- 响应包含继续分页所需的信息，不要求客户端根据当前结果数量猜测是否还有下一页。

## 状态码与错误

- 使用 HTTP 状态码表达结果类别，不要对失败请求返回 `200 OK` 再依赖业务字段判断成功与否。
- 常用客户端错误保持一致：`400` 请求格式错误、`401` 未认证、`403` 无权限、`404` 资源不存在、`409` 状态冲突、`422` 字段校验失败。
- 服务端错误使用 `5xx`，响应中不要泄露堆栈、SQL、内部路径、密钥或实现细节。
- 错误响应至少包含稳定机器错误码、可读消息、可选详情和请求追踪 ID。

```json
{
  "error": {
    "code": "project_not_found",
    "message": "项目不存在",
    "details": {},
    "request_id": "req_01"
  }
}
```

## 版本与兼容

- 本项目默认不使用路径版本前缀，资源直接从 `/projects`、`/accounts` 等根路径提供；不要自行添加 `/api/v1`。
- 只有出现真实的公开兼容需求、迁移方案和旧版本下线计划后，才引入统一版本策略。若未来选择路径版本，必须整体设计和迁移，不能只给个别接口临时加前缀。
- 新增可选字段通常保持向后兼容；删除字段、修改字段类型、改变状态码或收紧已有输入属于破坏性变更。
- 破坏性变更进入新版本，并明确迁移方式和旧版本下线计划。
- 不要把内部 crate 版本、数据库版本或部署版本直接当作公开 API 版本。

## 安全与可观察性

- 认证信息放在标准请求头或安全 Cookie 中，不要放入 URI 和日志可见的 query 参数。
- 对输入长度、集合分页大小、上传体积和复杂查询设置边界。
- 为请求生成或传递 request ID，并记录方法、匹配路由、状态码和耗时；日志不得记录密码、令牌和敏感正文。
- 跨域、限流、超时和重试策略按调用方与部署边界配置，不要由单个 handler 临时决定。

## 契约与验证

- 为每个接口记录方法、路径、鉴权、请求模型、响应模型、错误码和示例。
- 同步维护机器可读接口文档，例如 OpenAPI；实现变更时一并更新契约。
- 测试至少覆盖成功、认证失败、权限不足、参数校验、资源不存在、状态冲突和分页边界。
- 契约测试必须断言时间字段是 Unix 秒整数，并覆盖多单词枚举值与 `user_id` 一类参数的 `snake_case` 序列化结果。
- 审查接口时先检查资源与方法语义，再检查字段命名和实现代码，避免从现有 handler 反推不合理契约。

## 参考

- REST 资源建模、URI、表示和 HTTP 方法设计：[REST API Design Tutorial with Example](https://restfulapi.net/rest-api-design-tutorial-with-example/)
