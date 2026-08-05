# 业务模块模板

统一替换以下占位符：

- `<module>`：单数蛇形命名的 crate 和模块名，例如 `warehouse`
- `<Module>`：单数大驼峰命名的初始化类型，例如 `Warehouse`
- `<modules>`：复数蛇形命名的资源和表名，例如 `warehouses`
- `<schema>`：PostgreSQL schema 名，默认与 `<module>` 相同，例如 `warehouse`

根据业务调整实体字段和 SQL，禁止盲目复制示例字段。

## 文件结构

```text
modules/<module>/
├── Cargo.toml
└── src/
    ├── <module>.rs
    ├── entities.rs
    ├── entities/<module>.rs
    ├── errors.rs
    ├── handlers.rs
    ├── handlers/<modules>.rs
    ├── routers.rs
    ├── routers/<modules>.rs
    ├── stores.rs
    └── stores/<modules>.rs
```

## Cargo 库目标

```toml
[package]
name = "<module>"
version.workspace = true
edition.workspace = true

[lib]
path = "src/<module>.rs"

[dependencies]
axum.workspace = true
contracts.workspace = true
sqlx.workspace = true
```

删除遗留的 `src/main.rs`；业务模块是库，不是独立进程。

## 模块入口

```rust
use axum::Router;
use sqlx::PgPool;

pub(crate) mod entities;
pub(crate) mod errors;
pub(crate) mod handlers;
mod routers;
pub(crate) mod stores;

pub struct <Module> {
    state: <Module>State,
}

#[derive(Clone)]
pub struct <Module>State {
    pool: PgPool,
}

impl <Module> {
    pub fn new(pool: PgPool) -> Self {
        Self {
            state: <Module>State { pool },
        }
    }

    pub fn routers<S>(self) -> Router<S> {
        routers::initialize().with_state::<S>(self.state)
    }
}
```

## 公共 API 契约

在 `crates/contracts/src/<module>.rs` 中定义可供服务端和 SDK 共同使用的公开类型，并在 `crates/contracts` 的 crate 根模块中导出该模块：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Create<Module>Request {
    // 只添加客户端需要提交的字段。
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct <Module>Response {
    pub id: i64,
    // 只添加公共接口承诺返回的字段。
}
```

请求 DTO、成功响应 DTO 和错误响应 DTO 属于 API 契约，不得定义在 handler 或 `entities` 中。公共契约禁止依赖 SQLx，也不得加入仅供数据库使用的内部字段。
公共契约中的字段、query/path 参数和枚举值使用 `snake_case`；时间字段使用 `i64` Unix 秒时间戳，禁止直接序列化 `DateTime<Utc>`。

## 数据库实体

在 `entities.rs` 中声明：

```rust
pub(crate) mod <module>;
```

在 `entities/<module>.rs` 中定义：

```rust
use sqlx::FromRow;

#[derive(Debug, FromRow)]
pub(crate) struct <Module> {
    pub id: i64,
    // 添加与 SELECT 字段完全一致的业务字段。
}
```

## 数据存储层

在 `stores.rs` 中声明：

```rust
pub(crate) mod <modules>;
```

在 `stores/<modules>.rs` 中定义：

```rust
use sqlx::PgPool;

use crate::entities::<module>::<Module>;

pub(crate) async fn query_by_id(
    id: i64,
    pool: &PgPool,
) -> Result<Option<<Module>>, sqlx::Error> {
    sqlx::query_as::<_, <Module>>(
        r#"
        SELECT
            id
            -- 在这里明确添加业务字段。
        FROM <schema>.<modules>
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}
```

## 错误响应

创建模块专属的内部错误枚举，至少包含 `NotFound` 和 `Database(sqlx::Error)`。实现 `From<sqlx::Error>` 和 `IntoResponse`，但响应体使用 `crates/contracts` 中公开的通用错误 DTO。响应结构示例：

```json
{"code":"<module>_not_found","message":"未找到对应数据"}
```

数据库失败时记录内部错误，向客户端返回状态码 500 和通用中文说明。

## 请求处理器

在 `handlers.rs` 中声明：

```rust
pub(crate) mod <modules>;
```

在 `handlers/<modules>.rs` 中定义：

```rust
use axum::{
    Json,
    extract::{Path, State},
};
use contracts::<module>::<Module>Response;

use crate::{
    <Module>State,
    entities::<module>::<Module>,
    errors::<Module>Error,
    stores::<modules>::query_by_id,
};

pub(crate) async fn by_id(
    Path(id): Path<i64>,
    State(state): State<<Module>State>,
) -> Result<Json<<Module>Response>, <Module>Error> {
    let entity = query_by_id(id, &state.pool)
        .await?
        .ok_or(<Module>Error::NotFound)?;

    Ok(Json(<Module>Response {
        id: entity.id,
        // 显式映射公共字段。
    }))
}
```

handler 禁止返回 `Json<Entity>`。显式映射虽然多几行代码，但能够让数据库模型和 API/SDK 契约分别演进。

## 路由

在 `routers.rs` 中定义：

```rust
use axum::Router;

use crate::<Module>State;

mod <modules>;

pub fn initialize() -> Router<<Module>State> {
    Router::new().merge(<modules>::initialize())
}
```

在 `routers/<modules>.rs` 中定义：

```rust
use axum::{Router, routing::get};

use crate::{<Module>State, handlers};

pub(crate) fn initialize() -> Router<<Module>State> {
    Router::new().route("/<modules>/{id}", get(handlers::<modules>::by_id))
}
```

## 数据库变更

业务模块目录只保存运行时代码，不保存建表或测试数据 SQL。新增模块需要数据库结构时：

1. 在 `crates/migrate/migrations/` 一级目录新增版本化迁移，文件名包含 `<module>`；
2. 首个迁移创建 `<schema>`，并把模块表、序列和索引创建在该 schema 中；
3. 为新表和全部字段添加 `COMMENT ON`；稳定封闭的有限集合使用带类型和值说明的 PostgreSQL ENUM，并提供对应 Rust `enum`；
4. 在 `crates/migrate/seeds/<module>/` 按需新增本地测试数据，SQL 使用 `<schema>.<modules>`；
5. 在工作区中验证 migrate crate；
6. 未获得用户明确授权时，不运行迁移或写入测试数据。

禁止在 `crates/migrate/migrations/` 下按模块建立子目录；禁止在根目录创建 `sql/`，也禁止在 `modules/<module>/` 下创建 migrations 或零散 SQL 文件。运行时查询必须使用完整的 `<schema>.<modules>` 表名，禁止依赖 `search_path`。
