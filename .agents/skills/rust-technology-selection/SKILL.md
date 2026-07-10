---
name: rust-technology-selection
description: 用于设计、实现或审查 Rust 项目的数据库访问、数据库迁移、HTTP 服务、Axum 请求提取和异步运行时技术选型。涉及数据库交互时使用 sqlx；涉及数据库版本变更时使用 sqlx migrate 和 sqlx-cli，并将迁移 crate 与迁移文件放在 crates/migrate；涉及 HTTP 服务时使用 axum 及其 extract 能力，并由 tokio 提供异步运行时。
---

# Rust 技术选型

## 核心原则

优先复用 workspace 已选定的技术栈和依赖版本。不要为相同职责并行引入第二套框架，也不要因为样例代码更短而绕过本规范。

| 需求 | 强制选型 | 约束 |
| --- | --- | --- |
| 数据库交互 | `sqlx` | 使用连接池、查询和事务能力完成数据库访问。 |
| 数据库版本变更 | `sqlx migrate`、`sqlx-cli` | 迁移能力集中放在 `crates/migrate`。 |
| HTTP 服务 | `axum` | 使用 `Router`、extractor 和 middleware 组织服务。 |
| 异步运行时 | `tokio` | 由 Tokio 驱动 HTTP、数据库和后台异步任务。 |

## 数据库访问

- 需要连接数据库、执行 SQL、管理事务或维护连接池时，使用 `sqlx`。
- 根据实际数据库启用对应的 `sqlx` feature，只启用当前项目确实需要的能力。
- 优先使用 `sqlx::Pool` 及具体数据库对应的连接池类型，并通过应用状态共享连接池。
- 使用结构化查询结果映射；查询可静态校验时，优先采用 `query!`、`query_as!` 等宏。
- 不要自行拼接不可信输入生成 SQL；动态条件必须使用参数绑定或经过审查的查询构建方式。
- 不要引入 Diesel、SeaORM、rusqlite 或其他数据库访问框架，除非用户明确要求，或现有兼容约束无法使用 `sqlx`。

## 数据库迁移

- 所有数据库结构和版本变更使用 `sqlx migrate` 管理，并通过 `sqlx-cli` 创建、执行和检查迁移。
- 在 workspace 中使用独立的 `crates/migrate` crate 承载迁移职责。
- 将迁移 SQL 文件集中放在 `crates/migrate/migrations`，不要分散到业务 crate 或应用目录。
- 调用 `sqlx migrate` 时，将 `crates/migrate/migrations` 作为迁移 source。
- 迁移文件一旦进入共享环境，不要直接修改已执行迁移；新增后续迁移完成修正。
- 需要可逆迁移时，使用当前 `sqlx-cli` 支持的 reversible migration 方式成对维护 up/down 逻辑。
- `crates/migrate` 只负责数据库版本演进，不承载业务查询、HTTP 路由或领域服务。

## HTTP 与异步运行时

- 构建 HTTP API 或服务端入口时使用 `axum`。
- 使用 `tokio` 作为异步运行时；二进制入口可以使用 `#[tokio::main]`，库代码保持异步接口，不要私自创建嵌套 runtime。
- 使用 Axum 的 `Router`、`State`、extractor、response 和 middleware 能力表达 HTTP 语义。
- 通用中间件优先复用 Tower 生态，避免手写 Axum 或 Tower 已覆盖的基础设施。
- 数据库调用、网络请求和后台任务保持异步；阻塞操作必须放入专门的阻塞任务边界。
- 不要引入 Actix Web、Rocket、Warp 或其他 HTTP 框架，也不要直接使用 Hyper 拼装业务路由，除非用户明确要求或现有系统兼容性要求如此。

### Axum 请求提取

- handler 获取路径、查询、请求体、状态、扩展和连接信息时，尽可能使用 [`axum::extract`](https://docs.rs/axum/latest/axum/extract/index.html) 已有 extractor，不要手动拆解完整 `Request`。
- 路径参数使用 `Path<T>`，查询参数使用 `Query<T>`，JSON 正文使用 `Json<T>`，表单和上传分别使用 `Form<T>`、`Multipart`。
- 应用共享状态使用 `State<T>`；middleware 写入的请求级数据可以使用 `Extension<T>`；连接信息使用 `ConnectInfo<T>`。
- 使用类型化结构承接输入并通过 Serde 反序列化，不要在 handler 中手动解析字符串、JSON 或 query map。
- extractor 按 handler 参数从左到右执行。实现 `FromRequest`、会消费 body 的 extractor 必须放在最后，并且一个 handler 只能有一个消费 body 的 extractor。
- 只读取请求 parts 的自定义能力实现 `FromRequestParts`；确实需要消费 body 时才实现 `FromRequest`，不要为同一个普通 extractor 同时实现两者。
- 内置 extractor 无法表达认证、租户、签名或组合校验时，封装自定义 extractor，并将 rejection 转换为项目统一错误响应。
- 需要针对单个 handler 自定义提取失败时，接收 `Result<Extractor, Rejection>` 并显式映射错误；不要让不同接口产生相互冲突的错误格式。
- 使用 `Request`、`Bytes`、`String` 或原始 `HeaderMap` 前，先确认没有更具体的 extractor 可以表达需求。只有确实需要完整控制时才退回原始请求。
- 根据接口场景通过 `DefaultBodyLimit` 设置正文大小上限，不要无边界读取请求体。

## Workspace 依赖

- 优先在根 `Cargo.toml` 的 `[workspace.dependencies]` 中统一声明 `sqlx`、`axum` 和 `tokio` 的版本与公共 feature。
- 具体 crate 使用 `{ workspace = true }` 引入依赖，只在确有必要时追加 crate 特有 feature。
- 新增 `crates/migrate` 时，将其加入 workspace members，并保持它与业务 crate 的职责边界清晰。

## 执行流程

1. 先检查现有 workspace、目标 crate 和依赖配置，确认是否已经存在可复用能力。
2. 根据需求选择 `sqlx`、`sqlx migrate`、`axum` 或 `tokio`，不要扩大依赖范围。
3. 数据库版本发生变化时，同步在 `crates/migrate/migrations` 新增迁移，并检查执行顺序。
4. HTTP 服务通过 Axum 路由组合接入，运行入口交给 Tokio。
5. 完成后运行格式化、编译和相关测试；涉及迁移时，在可用测试数据库上验证迁移能够按顺序执行。

## 例外处理

如果现有代码、第三方 SDK 或部署环境与本规范冲突，不要静默选择替代技术。先说明冲突、继续使用标准选型的成本和替代方案影响，再获得用户确认。
