# 查询

## 编译期校验的查询

sqlx 会在编译期根据数据库 schema 验证查询。这可以在运行前发现列名拼写错误、类型不匹配和无效 SQL。

### `query!` 宏

返回一个字段与查询列匹配的匿名结构体。

```rust
// Compile-time checked — column names and types verified
let row = sqlx::query!(
    "SELECT id, name, email FROM users WHERE id = $1",
    user_id
)
.fetch_one(&pool)
.await?;

let name: String = row.name;
let email: Option<String> = row.email; // nullable column → Option
```

### `query_as!` 宏

将结果直接映射到具名结构体。可复用结果类型应优先使用它。

```rust
#[derive(Debug)]
struct User {
    id: Uuid,
    name: String,
    email: Option<String>,
    created_at: DateTime<Utc>,
}

let user = sqlx::query_as!(
    User,
    "SELECT id, name, email, created_at FROM users WHERE id = $1",
    user_id
)
.fetch_optional(&pool)
.await?;
```

### 离线模式

在 CI/CD 无法连接数据库时，SQLx 可以从 `.sqlx/` 读取查询元数据。先连接与迁移一致的开发数据库生成缓存，再把缓存提交到版本库：

```bash
# 从实时数据库生成 workspace 查询缓存
cargo sqlx prepare --workspace

# CI 强制使用缓存，不允许意外连接数据库
SQLX_OFFLINE=true cargo build --workspace
```

`sqlx.toml` 不提供 `offline = true`；离线模式由 `.sqlx/` 与 `SQLX_OFFLINE=true` 控制。
SQLx 0.9 的 `sqlx.toml` 只在启用 `sqlx-toml` feature 后用于数据库 URL 变量名、迁移和
查询宏类型映射等配置。没有这些需求时不要创建该文件。

## Fetch 方法

| 方法 | 返回值 | 适用场景 |
|--------|---------|----------|
| `.fetch_one()` | `T`（0 行时出错，多行时返回第一行） | 缺失视为错误，并且只需要第一行 |
| `.fetch_optional()` | `Option<T>` | 预期零行或一行 |
| `.fetch_all()` | `Vec<T>` | 小型、有边界的结果集 |
| `.fetch()` | `Stream<Item = Result<T>>` | 大型或无边界结果 |

`fetch_one()` 不会检查结果是否超过一行。若业务要求“至多一行”，应通过主键/唯一约束保证；若查询本身无法保证基数，则显式限制并检查结果数量。

### 常见错误：查找场景使用 `fetch_one`

```rust
// BAD - returns Err(RowNotFound) on "not found" which is an expected case
let user = sqlx::query_as!(User, "SELECT ... WHERE id = $1", id)
    .fetch_one(&pool)
    .await?; // RowNotFound error for missing users

// GOOD - "not found" is a normal case, not an error
let user = sqlx::query_as!(User, "SELECT ... WHERE id = $1", id)
    .fetch_optional(&pool)
    .await?;
match user {
    Some(user) => Ok(user),
    None => Err(Error::NotFound(id)),
}
```

### 流式处理大型结果集

```rust
use futures::TryStreamExt;

let mut stream = sqlx::query_as!(Event, "SELECT * FROM events WHERE workflow_id = $1", wf_id)
    .fetch(&pool);

while let Some(event) = stream.try_next().await? {
    process(event).await;
}
```

### Edition 2024：查询辅助函数中的 RPIT 生命周期捕获

在 edition 2024 中，`-> impl Trait` 默认捕获作用域内的所有生命周期。这会影响返回 sqlx 查询 stream 或 future 的函数。

```rust
// Edition 2021 — worked because `-> impl Stream` didn't capture 'a
fn get_events<'a>(pool: &'a PgPool, wf_id: Uuid) -> impl Stream<Item = Result<Event, sqlx::Error>> {
    sqlx::query_as!(Event, "SELECT * FROM events WHERE workflow_id = $1", wf_id)
        .fetch(pool)
}

// Edition 2024 — captures 'a by default, which is usually correct here.
// If you need to NOT capture a lifetime, use precise capture syntax:
fn get_events<'a>(pool: &'a PgPool, wf_id: Uuid) -> impl Stream<Item = Result<Event, sqlx::Error>> + use<'a> {
    sqlx::query_as!(Event, "SELECT * FROM events WHERE workflow_id = $1", wf_id)
        .fetch(pool)
}
```

大多数借用连接池的 sqlx 查询辅助函数*应当*捕获连接池的生命周期，因此 edition 2024 的默认行为通常正确。当返回类型被存入一个比借用活得更久的结构体时，应标记问题。

## 绑定参数

始终使用绑定参数（Postgres 使用 `$1`、`$2`；MySQL/SQLite 使用 `?`）。永远不要将值插入查询字符串。

```rust
// BAD - SQL injection vulnerability
let query = format!("SELECT * FROM users WHERE name = '{}'", name);
sqlx::query(&query).fetch_one(&pool).await?;

// GOOD - parameterized query
sqlx::query("SELECT * FROM users WHERE name = $1")
    .bind(&name)
    .fetch_one(&pool)
    .await?;

// BEST - compile-time checked
sqlx::query!("SELECT * FROM users WHERE name = $1", name)
    .fetch_one(&pool)
    .await?;
```

## 运行时查询也是有效策略

`query()`、`query_as::<_, T>()` 与 `QueryBuilder` 不是查询宏的降级错误。它们适用于动态 SQL、可复用 `FromRow` 类型，以及构建环境无法提供编译期数据库元数据的项目。

```rust
#[derive(Debug, sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
}

let user = sqlx::query_as::<_, UserRow>(
    "SELECT id, email FROM users WHERE id = $1",
)
.bind(user_id)
.fetch_optional(&pool)
.await?;
```

审查运行时查询时，核对 SQL 语法、参数绑定、测试、schema 约束和字段映射。`FromRow` 与 `Type` 是同步 trait；schema 或 SELECT 列变化时必须同步更新 Rust 映射。

## 类型映射

### Rust ↔ PostgreSQL

| Rust 类型 | PostgreSQL 类型 |
|-----------|-----------------|
| `i32` | `INT4` / `INTEGER` |
| `i64` | `INT8` / `BIGINT` |
| `f64` | `FLOAT8` / `DOUBLE PRECISION` |
| `Decimal` | `NUMERIC` / `DECIMAL` |
| `String` | `TEXT` / `VARCHAR` |
| `bool` | `BOOL` |
| `Uuid` | `UUID` |
| `DateTime<Utc>` | `TIMESTAMPTZ` |
| `NaiveDateTime` | `TIMESTAMP` |
| `serde_json::Value` | `JSONB` / `JSON` |
| `Vec<u8>` | `BYTEA` |
| `Option<T>` | 可空列 |

### 自定义枚举类型

稳定且封闭的取值集合先在模块 schema 中创建 PostgreSQL ENUM，并逐项说明含义：

```sql
CREATE TYPE workflow.workflow_status AS ENUM (
    'pending',     -- 等待处理。
    'in_progress', -- 正在处理。
    'complete',    -- 已成功完成。
    'failed'       -- 处理失败。
);

COMMENT ON TYPE workflow.workflow_status IS
    '工作流状态：pending=等待，in_progress=处理中，complete=完成，failed=失败';
```

Rust 数据库侧使用 `enum` 和 SQLx 类型映射，不要为了 HTTP 直接给数据库类型派生 Serde：

```rust
#[derive(Debug, Clone, sqlx::Type)]
#[sqlx(type_name = "workflow_status", rename_all = "snake_case")]
pub(crate) enum WorkflowStatus {
    /// 等待处理。
    Pending,
    /// 正在处理。
    InProgress,
    /// 已成功完成。
    Complete,
    /// 处理失败。
    Failed,
}
```

枚举需要跨 HTTP 边界时，在 `crates/contracts` 中定义独立的公开 Serde 枚举，使用
`#[serde(rename_all = "snake_case")]`，并在 handler 边界与数据库/领域枚举显式转换。审查时
确保 PostgreSQL 标签、`sqlx::Type` 和公共契约 wire value 一致。

### Edition 2024：保留关键字 `gen`

在 edition 2024 中，`gen` 是保留关键字。任何名为 `gen` 的 sqlx 枚举变体或结构体字段都无法编译。使用 `r#gen` 作为 Rust 标识符，并使用 `#[sqlx(rename)]` 保留数据库列名。

```rust
// BAD — fails to compile on edition 2024
#[derive(sqlx::Type)]
#[sqlx(type_name = "generation_type", rename_all = "snake_case")]
pub enum GenerationType {
    Manual,
    Gen, // compile error: `gen` is a reserved keyword
}

// GOOD — compiles on edition 2024, database value unchanged
#[derive(sqlx::Type)]
#[sqlx(type_name = "generation_type", rename_all = "snake_case")]
pub enum GenerationType {
    Manual,
    #[sqlx(rename = "gen")]
    r#Gen,
}
```

### Edition 2024：使用 `#[expect]` 抑制 lint

对仅用于 sqlx 映射、但未被直接读取的结构体字段，优先使用 `#[expect(unused)]` 而非 `#[allow(unused)]`。当不再需要抑制时，`#[expect]` 属性会发出警告，从而使 lint 覆写能够自清理。

```rust
// BAD — silent if the field starts being used elsewhere
#[allow(dead_code)]
struct AuditRow {
    id: i64,
    raw_payload: serde_json::Value,
}

// GOOD — warns when suppression is no longer needed
#[expect(dead_code)]
struct AuditRow {
    id: i64,
    raw_payload: serde_json::Value,
}
```

## 审查问题

1. 查询策略是否适合当前约束；静态宏或运行时查询是否都有相应校验与测试？
2. 可能不返回任何行的查找是否使用 `.fetch_optional()`？
3. 是否使用绑定参数（没有字符串插值）？
4. 大型结果集是否使用 `.fetch()` 进行流式处理？
5. Rust 类型是否与 PostgreSQL 列类型一致？
6. sqlx 和 serde 的枚举表示是否一致？
7. （Edition 2024）是否有枚举变体或字段在不使用 `r#gen` 的情况下将 `gen` 作为标识符？
8. （Edition 2024）返回 `-> impl Stream`/`-> impl Future` 的函数是否考虑了 RPIT 生命周期捕获变化？
