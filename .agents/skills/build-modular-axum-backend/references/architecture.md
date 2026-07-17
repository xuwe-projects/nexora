# 标准架构

## 工作区目录

使用以下职责划分：

```text
.
├── apps/
│   └── server/
│       ├── Cargo.toml
│       └── src/
│           ├── config.rs
│           ├── main.rs
│           └── routers.rs
├── config/
│   ├── example.server.toml
│   ├── example.admin.toml
│   ├── server.toml              # server 本地配置，不提交
│   └── admin.toml               # admin 本地配置，不提交
├── crates/
│   ├── configuration/
│   ├── contracts/               # 服务端与 SDK 共用的公开 API 契约
│   └── migrate/
│       ├── migrations/          # 迁移文件扁平存放
│       ├── seeds/               # 测试数据按模块分组
│       └── src/
├── modules/
│   ├── account/
│   └── <业务模块>/
```

服务端负责进程启动、应用连接池、顶层配置类型和路由组合。业务模块负责自己的 HTTP 行为和数据库行为，并且不得依赖服务端 crate。

## 依赖方向

```text
examples/server ──> modules/account
            ├─> modules/<业务模块>
            └─> crates/configuration

crates/migrate ──> crates/configuration + sqlx

SDK ──────────> crates/contracts

modules/* ────> crates/contracts + axum + sqlx
```

把每个本地 crate 添加到 `[workspace.dependencies]`，使用时声明 `.workspace = true`。

## State 所有权

在服务端只创建一个 SQLx 连接池：

```rust
#[derive(Clone)]
pub struct AppState {
    pool: PgPool,
}
```

`PgPool::clone()` 只会创建指向同一个底层连接池的新句柄。禁止使用 `Arc<PgPool>`。

每个模块拥有私有 State：

```rust
#[derive(Clone)]
pub struct ModuleState {
    pool: PgPool,
}
```

模块先构建 `Router<ModuleState>`，再通过 `with_state` 注入 `ModuleState`，最后以泛型形式返回 `Router<S>`。这样服务端可以把内部 State 不同的模块合并为 `Router<AppState>`。

## 路由组合

使用实例方法调用 `merge`：

```rust
pub(crate) fn initialize(state: &AppState) -> Router<AppState> {
    let account_module = account::Account::new(state.pool.clone());
    let warehouse_module = warehouse::Warehouse::new(state.pool.clone());

    Router::new()
        .merge(account_module.routers::<AppState>())
        .merge(warehouse_module.routers::<AppState>())
}
```

启动时注入最后缺少的应用 State：

```rust
let app = routers::initialize(&state).with_state(state);
axum::serve(listener, app).await?;
```

只有 `Router<()>` 可以直接提供服务。已经注入某个 State 后仍返回 `Router<State>`，等于错误地声明路由仍然缺少该 State。

## API 契约与数据库实体边界

`crates/contracts` 保存跨进程边界公开的请求 DTO、成功响应 DTO 和错误响应 DTO。服务端 handler、客户端 SDK 和接口测试直接复用这些类型。

业务模块的 `entities` 只描述数据库查询结果，保持 `pub(crate)`，并只派生 SQLx 所需能力。禁止直接返回 Entity：数据库增加内部列、调整查询模型或拆表时，不应被迫改变公共 API。

数据流保持为：

```text
公共请求 DTO -> handler -> store -> 私有 Entity -> handler 显式映射 -> 公共响应 DTO
```

公共契约不得依赖 SQLx，不得包含数据库表名、内部审计字段或仅供存储层使用的实现细节。只有确实属于接口协议的字段才能进入 `crates/contracts`。

## 数据库和 HTTP 约定

- 每个业务模块默认拥有与模块名一致的 PostgreSQL schema，例如 `account` 和 `warehouse`。
- 表始终通过完整限定名访问，例如 `account.accounts` 和 `warehouse.warehouses`。
- PostgreSQL 表名和资源路由默认使用复数形式。
- 标识符默认使用 `BIGSERIAL`，Rust 类型使用 `i64`。
- 使用参数绑定，禁止拼接 SQL 参数。
- 查询单条记录时使用 `fetch_optional`，区分“记录不存在”和“数据库失败”。
- 数据库读取模型只派生 `sqlx::FromRow`；对外 JSON 使用 `crates/contracts` 中派生 serde 能力的响应 DTO。
- 禁止在客户端 JSON 中暴露 SQLx 或数据库错误详情。
- 第一个查询接口优先使用 `GET /资源复数/{id}`。

## PostgreSQL schema 隔离

所有业务模块共享同一个数据库和 `PgPool`，但通过独立 schema 隔离表、序列、索引和约束：

```text
account 模块   ──> account.accounts
warehouse 模块 ──> warehouse.warehouses
```

共享连接池不会妨碍 schema 隔离。同一个 PostgreSQL 连接可以访问多个 schema，只要应用数据库角色拥有相应权限。

运行时 SQL 必须使用完整限定名：

```sql
SELECT id, username, nickname
FROM account.accounts
WHERE id = $1;
```

禁止在共享连接池上使用会话级 schema 切换：

```sql
SET search_path TO account;
```

连接会被连接池复用，会话级 `search_path` 可能影响随后取得同一连接的其他模块。只有在明确使用独立连接池，或在受控事务中使用 `SET LOCAL` 时才能考虑 `search_path`；本架构默认始终使用 `schema.table`。

允许使用跨 schema 外键：

```sql
REFERENCES account.accounts(id)
```

但这会形成数据库层模块依赖，必须在全局迁移版本中先创建被依赖的 schema 和表。

## 数据库迁移职责

- 由 `crates/migrate` 统一负责所有表结构、索引、约束和必要基础数据变更。
- 每个模块的首个迁移负责 `CREATE SCHEMA IF NOT EXISTS <module>`，后续对象全部创建在该 schema 中。
- 版本化迁移文件统一平铺在 `crates/migrate/migrations/` 一级目录，并在文件名中包含模块名。
- SQLx 默认迁移加载器只读取一级目录，禁止在 `migrations/` 下按模块创建子目录。
- 所有应用模块与外部框架迁移共用全局唯一的迁移版本号和 `_sqlx_migrations` 记录表；宿主
  在运行前合并两类迁移并拒绝跨来源版本冲突。
- 本地测试数据放在 `crates/migrate/seeds/<module>/`，可以按模块分组，但不得随生产迁移自动写入。
- 业务模块只负责运行时查询，不拥有迁移文件。
- 仓库根目录不得创建 `sql/`，也不得在各业务模块下散落建表脚本。
- 已应用的迁移文件不得修改；后续变更必须新增更高版本的迁移。
- 迁移只由宿主构造的唯一 SQLx `Migrator` 执行一次；禁止框架初始化和应用迁移分别运行。

## 配置与本地敏感信息

- 跟踪 `config/example.*` 文件。
- 递归忽略 `config/` 下其他文件。
- `apps/` 下后端应用的目录名和 Cargo 包名必须一致。
- 由调用方选择配置文件；没有命令行参数时，使用 `env!("CARGO_PKG_NAME")` 推导 `config/<应用名>.toml`。
- 例如 `examples/server` 默认使用 `config/server.toml`，`apps/admin` 默认使用 `config/admin.toml`。
- 禁止提交真实数据库凭据。
