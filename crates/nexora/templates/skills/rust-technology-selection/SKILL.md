---
name: rust-technology-selection
description: 用于设计、实现或审查 Nexora workspace 的数据库访问、数据库迁移、HTTP 服务、Axum 请求提取和异步运行时技术选型。数据库统一使用 SQLx 与 PostgreSQL，迁移集中在 crates/migrate，HTTP 使用 Axum 0.8，异步运行时使用 Tokio。
---

# Nexora Rust 技术选型

## 固定技术栈

优先复用 workspace 已锁定的版本和 features，不为相同职责并行引入第二套框架。

| 需求 | 选型 | 项目边界 |
| --- | --- | --- |
| 数据库访问 | `sqlx` + PostgreSQL | `crates/database` 与业务 `store/postgres.rs` |
| 数据库版本变更 | `sqlx migrate`、`sqlx-cli` | `crates/migrate` |
| HTTP 服务 | Axum 0.8 + Tower | `crates/api` |
| 异步运行时 | Tokio | 宿主入口与异步库接口 |

除非用户明确要求或有无法兼容的既有系统，不引入 Diesel、SeaORM、rusqlite、Actix Web、Rocket、Warp 或另一套 runtime。

## SQLx 与 PostgreSQL

### 连接池

- 在 `examples/server` composition root 中异步创建一次连接池，通过 `Database`、`AppState` 和 Postgres store 共享。
- `PgPool` 可廉价克隆，通常无需 `Arc<PgPool>`；不要每个请求创建连接池。
- 根据 PostgreSQL 总连接预算、应用副本数和压测结果配置 `max_connections`、`acquire_timeout` 等参数。
- 不使用静态 `LazyLock` 配合 `Handle::block_on` 初始化异步连接池，也不创建嵌套 runtime。

### 查询策略

- 静态 SQL 在数据库元数据可用时优先考虑 `query!`、`query_as!`、`query_scalar!`，以获得编译期校验。
- CI 需要离线校验时运行 `cargo sqlx prepare`、提交 `.sqlx/` 并设置 `SQLX_OFFLINE=true`。
- `query`、`query_as::<_, T>` 是动态 SQL、可复用 `FromRow` 类型或无法提供编译期元数据时的有效策略；不要仅因运行时查询而重写代码。
- 参数始终绑定。动态条件使用 `QueryBuilder` 和 `.push_bind(...)`；批量插入可使用 `.push_values(...)`，避免循环逐条往返。
- `.fetch_one()` 在零行时错误，多行时返回第一行；它不校验唯一性。
- 零或一行使用 `.fetch_optional()`；小型有界集合使用 `.fetch_all()`；大型结果使用分页或 `.fetch()`。
- 可空列映射为 `Option<T>`；结构化数据库类型映射到对应 Rust 类型。
- `FromRow`、`Type` 是同步 trait。schema、SELECT 列和 Rust 映射必须同步维护。

### 事务与错误

- 原子多语句操作使用 `pool.begin()`，所有查询通过 `&mut *tx` 执行，成功路径显式 `commit()`。
- 事务 drop 会回滚，但仍要保留可诊断的错误 source 和操作上下文。
- store 将约束冲突、未找到和数据库不可用等情况映射为稳定业务/存储错误；HTTP 层再统一映射为 `404`、`409` 或 `5xx`，不要让 SQLx 错误直接成为公开响应。
- 需要数据库集成测试时优先使用 `#[sqlx::test]` 或项目现有测试数据库工具，隔离数据库并运行指定迁移。

## 数据库迁移

- 所有结构和版本变更由 `sqlx migrate` 管理，SQL 文件集中在 `crates/migrate/migrations`。
- Nexora 使用顺序版本；SQLx 同时接受顺序和时间戳版本，不能把另一种格式视为无效。
- 可逆迁移用同版本 `.up.sql`/`.down.sql` 成对维护。
- 已进入共享环境的迁移不直接修改，通过后续迁移修正。
- `crates/migrate` 只负责版本演进，不承载业务查询、HTTP 路由或 application。
- 迁移由宿主启动流程在接收流量前执行；Nexora 框架迁移与应用迁移必须先合并并检查版本
  冲突，再由唯一 SQLx `Migrator` 执行一次。业务 crate、handler 和 `Server::initialize`
  不运行迁移。

## Axum 与 Tokio

- 使用 Axum 0.8 的 `Router`、extractor、response 与 middleware；路径参数写作 `{id}`。
- 项目默认直接暴露资源路径，不添加 `/api/v1` 前缀。
- 路径、查询、JSON、共享状态分别使用 `Path<T>`、`Query<T>`、`Json<T>`、`State<T>`。
- extractor 从左到右执行；消费 body 的 extractor 放最后，每个 handler 只有一个 body consumer。
- 只读取 request parts 的自定义能力实现原生 async `FromRequestParts`；需要 body 时才实现 `FromRequest`。
- 认证、租户和签名 extractor 的 rejection 统一转换为项目错误契约。
- 通用中间件优先复用 Tower 生态；阻塞操作放入 `spawn_blocking`。
- 二进制入口可使用 `#[tokio::main]`；库代码提供异步接口，不自行创建 runtime。
- 进程入口监听 Ctrl+C 与 Unix `SIGTERM`，通过 Axum graceful shutdown 停止接收新请求。

## Workspace 依赖

- 在根 `Cargo.toml` 的 `[workspace.dependencies]` 统一声明公共版本和 features。
- crate 使用 `{ workspace = true }` 引入依赖，只在确有必要时增加局部 feature。
- 只启用当前数据库、runtime、TLS 和类型映射实际需要的 SQLx features。

## 执行流程

1. 检查根 `Cargo.toml`、目标 crate、现有模块边界和已锁定版本。
2. 在既有技术栈中实现最小改动，不扩大依赖范围。
3. 数据库版本变化时在 `crates/migrate/migrations` 新增迁移并验证顺序和回滚承诺。
4. 数据库 I/O 放在 Postgres store，HTTP 通过 Router/handler 调用 application。
5. 运行格式化、workspace 编译、Clippy 和相关测试；涉及迁移时在隔离 PostgreSQL 上验证。

## 例外处理

如果现有代码、第三方 SDK 或部署环境与本规范冲突，说明冲突、继续采用标准选型的成本与替代方案影响，再请求用户决定；不要静默引入第二套框架。
