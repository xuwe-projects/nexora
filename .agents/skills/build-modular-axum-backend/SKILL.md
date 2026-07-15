---
name: build-modular-axum-backend
description: 使用 Cargo 工作区、Axum 0.8、SQLx/PostgreSQL、模块独立 State 与 schema、共享 PgPool、公共 API 契约、config-rs 和 crates/migrate 构建模块化 Rust 后台；规范完整 DDL COMMENT、PostgreSQL ENUM/Rust enum 映射，以及 HTTP snake_case 和 Unix 秒时间戳。当需要初始化后台架构、新增 account 或 warehouse 业务模块、复用 entities/store/handler/router/error 分层、组合不同 State、设计公共 DTO，或新增数据库接口与表结构时使用。
---

# 构建模块化 Axum 后台

## 目标

按照既定的架构构建后台：服务端应用只负责启动和组合路由，各业务模块使用独立 crate 管理自身状态、HTTP 层和数据库层；每个模块拥有独立 PostgreSQL schema，同时共享服务端创建的 `PgPool` 句柄。

## 读取相关参考资料

- 修改目录结构或 State 组合方式前，必须阅读 [架构约定](references/architecture.md)。
- 创建或扩展业务模块时，必须阅读 [业务模块模板](references/module-template.md)。
- 新增或修改 HTTP 请求、成功响应、错误响应或 SDK 公共类型时，必须阅读 [API 契约约定](references/contracts.md)。
- 创建工作区、修改配置加载或服务端启动逻辑时，必须阅读 [配置加载约定](references/configuration.md)。
- 新增或修改表结构、索引、约束和初始化数据时，必须阅读 [数据库迁移约定](references/migrations.md)。

## 工作流程

1. 编辑前检查工作区。
   - 阅读根目录 `Cargo.toml`、服务端入口和路由、`crates/configuration`、`crates/migrate`，以及一个完整的现有业务模块。
   - 使用 `rg --files` 查找文件，并保留脏工作区中的用户修改。
   - 仓库存在 `AGENTS.md` 时遵守其中要求。

2. 根据上下文确认范围。
   - 新增模块时，确定单数 Rust 类型名、复数路由和表名、PostgreSQL schema 名、业务字段以及所需接口。
   - 用户未指定字段时，选择小而实用的读取模型，并说明所做假设。
   - 用户仅要求评审或解释时，只检查和报告，不修改文件。

3. 添加工作区依赖，同时保持单向依赖。
   - 让 `apps/<app>` 下的后端应用依赖业务模块。
   - 让提供 HTTP 接口的业务模块依赖 `crates/contracts`；SDK 也直接依赖该 crate，共用同一组公开类型。
   - 禁止业务模块依赖任何 `apps/<app>` 或其中的 `AppState`。
   - 统一使用工作区管理的 `axum`、`serde` 和 `sqlx` 依赖。
   - `apps/` 下每个后端应用的目录名和 Cargo 包名必须一致；未指定配置路径时，默认加载 `config/<应用名>.toml`。

4. 实现模块边界。
   - 将业务模块创建为库 crate。
   - 按业务模块模板创建 entities、stores、handlers、routers、errors 和模块 State 文件。
   - 把对外请求 DTO、成功响应 DTO 和通用错误响应放在 `crates/contracts`，禁止在 handler 中重复定义公共请求结构。
   - HTTP JSON、query、path 与 form 参数统一使用 `snake_case`；HTTP 枚举值使用 `snake_case`，时间字段使用 Unix 秒时间戳整数。
   - 数据库 Entity 只负责 SQLx 行映射并保持业务模块私有，禁止把 Entity 当作 HTTP 响应或 SDK 类型。
   - 在模块 State 中直接保存 `PgPool`。克隆 `PgPool` 句柄，不要额外包装 `Arc`。
   - 在 `routers<S>()` 内绑定模块 State，返回能与服务端待注入 State 兼容的路由。

5. 实现数据库访问和 HTTP 行为。
   - 除非仓库已经维护 SQLx 离线元数据，否则使用运行时 `sqlx::query_as`。
   - 明确列出查询字段，禁止使用 `SELECT *`。
   - 每个业务模块默认使用与模块名一致的独立 PostgreSQL schema，例如 `account.accounts` 和 `warehouse.warehouses`。
   - 所有运行时 SQL 必须使用完整的 `schema.table` 限定名，禁止依赖默认 schema。
   - 多个模块共享同一个连接池时，禁止通过会话级 `SET search_path` 切换模块 schema。
   - PostgreSQL 的 `BIGSERIAL/BIGINT` 标识符对应 Rust `i64` 和 `Path<i64>`。
   - handler 接收和返回 `crates/contracts` 中的公共 DTO，并显式完成 Entity 到响应 DTO 的转换。
   - 数据库或领域层可以使用 `DateTime<Utc>`，但跨 HTTP 边界时必须显式转换为 `i64` Unix 秒时间戳。
   - 稳定且封闭的有限取值字段使用 PostgreSQL ENUM 与 Rust `enum`，数据库标签、SQLx 映射和 HTTP 枚举值保持一致的 `snake_case`。
   - 禁止给数据库 Entity 派生 `serde::Serialize` 只是为了直接返回 JSON；数据库结构和 API 契约必须能够独立演进。
   - 成功时返回 JSON，记录不存在时返回 404，数据库失败时返回通用的 500 响应。
   - 禁止在客户端响应中暴露数据库错误详情。

6. 在服务端组合模块。
   - 克隆服务端连接池句柄并传入模块构造函数。
   - 将 `module.routers::<AppState>()` 合并进根路由。
   - 只在服务端边界调用最终的 `.with_state(app_state)`。

7. 通过 `crates/migrate` 管理数据库变更。
   - 把表结构、索引和约束变更添加到 `crates/migrate/migrations/` 一级目录，文件名包含模块名。
   - 首个模块迁移必须创建对应 schema，并在该 schema 中创建模块表、序列和索引。
   - 每个新表必须使用 `COMMENT ON TABLE` 和 `COMMENT ON COLUMN` 完整记录表与全部字段语义；类型、约束、索引、函数和触发器也要记录用途。
   - PostgreSQL ENUM 必须创建在模块 schema 中，使用 `COMMENT ON TYPE` 描述类型，并在 DDL 中逐项注明每个枚举值的中文含义。
   - 迁移版本号必须全局唯一；禁止在 `migrations/` 下按模块建立子目录，因为 SQLx 默认不会递归扫描。
   - 把仅用于本地验证的测试数据放在 `crates/migrate/seeds/<module>/`，禁止混入生产迁移。
   - 业务模块和仓库根目录不得保存零散建表 SQL，也不得创建根目录 `sql/`。
   - 正常迁移只允许向前追加，禁止使用 `DROP TABLE` 重置已有结构。

8. 完成验证。
   - 检查每个 `CREATE TABLE` 都有表注释和全部列注释，每个 `CREATE TYPE ... AS ENUM` 都有类型注释和值含义，并存在对应 Rust `enum`。
   - 用契约测试断言时间字段序列化为 Unix 秒整数，字段名、query/path 参数和枚举值均为 `snake_case`。
   - 运行 `cargo fmt --all`。
   - 运行 `cargo test --workspace`。
   - 运行 `cargo clippy --workspace --all-targets -- -D warnings`。
   - 已存在有效本地配置和测试数据时，启动服务端并请求新增接口，随后停止进程。

## 后端应用默认配置规则

由 `apps/` 下当前应用的 Cargo 包名推导默认配置文件，禁止在通用启动模板中固定写死 `server.toml`：

```text
apps/server  ──> config/server.toml
apps/admin   ──> config/admin.toml
```

使用 `env!("CARGO_PKG_NAME")` 获取当前应用名；第一个命令行参数始终可以覆盖默认路径。

## State 模型硬性约束

把 `Router<S>` 理解为“仍缺少 State `S` 的路由”，而不是“已经持有 `S` 的路由”。每个模块必须先注入自身 State：

```rust
pub fn routers<S>(self) -> Router<S> {
    routers::initialize().with_state::<S>(self.state)
}
```

服务端合并模块时再选择 `S = AppState`。禁止仅为满足 Axum 路由类型而让业务模块依赖服务端的具体 `AppState`。

## 团队语言约定

说明文字、代码注释、SQL 注释、错误说明和最终交付摘要默认使用简体中文。Skill 名、YAML 固定字段、Rust 标识符、库名、协议名和命令等技术标识保留原文。

## 交付要求

总结已创建的模块、接口和迁移；提供可点击文件链接；列出已完成的验证；给出运行迁移、按需载入测试数据和测试接口的命令。存在数据库凭据或运行验证阻塞时，必须明确说明。
