# 迁移与连接池管理

## 连接池配置

在异步 composition root 中创建连接池，再把它传给应用状态和各 Postgres store：

```rust
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;

let pool = PgPoolOptions::new()
    .max_connections(config.max_connections)
    .acquire_timeout(Duration::from_secs(5))
    .idle_timeout(Duration::from_secs(600))
    .max_lifetime(Duration::from_secs(1_800))
    .connect(&config.url)
    .await?;
```

`PgPoolOptions::new()` 的 `max_connections` 默认值是 **10**。不要把默认值写成 5，也不要仅凭默认值就判断生产配置错误。按以下信息计算并验证连接预算：

- PostgreSQL `max_connections` 与管理、迁移、监控预留；
- 应用副本数及每个副本的最大连接数；
- 并发数据库工作量和事务持有时间；
- acquire timeout、压测结果和连接等待指标。

`PgPool` 内部已经共享连接池状态，可廉价克隆。不要在每个请求中创建新连接池，也通常无需再包一层 `Arc<PgPool>`。

### 禁止同步阻塞初始化异步连接池

不要使用 `LazyLock`、`once_cell` 或 `lazy_static!` 配合 `tokio::runtime::Handle::block_on`：

```rust
// 错误：可能在运行时内部嵌套 block_on，并隐藏启动失败。
static POOL: LazyLock<PgPool> = LazyLock::new(|| {
    tokio::runtime::Handle::current().block_on(PgPool::connect("..."))
});
```

连接失败必须从异步启动流程返回带上下文的错误。全局静态池还会让测试隔离、关闭顺序和多实例配置变得困难。

## 事务

将必须原子提交的读写放在同一事务中：

```rust
let mut tx = pool.begin().await?;

sqlx::query("INSERT INTO orders (user_id, total) VALUES ($1, $2)")
    .bind(user_id)
    .bind(total)
    .execute(&mut *tx)
    .await?;

sqlx::query("UPDATE inventory SET count = count - $1 WHERE item_id = $2")
    .bind(quantity)
    .bind(item_id)
    .execute(&mut *tx)
    .await?;

tx.commit().await?;
```

事务未提交即 drop 时 SQLx 会回滚。审查时仍需确认：

- 所有相关查询都使用 `&mut *tx`，没有误用 pool 逃出事务；
- 成功路径显式提交；
- 唯一约束、锁顺序和受影响行数能处理并发竞争；
- 错误映射保留操作上下文，但不泄露敏感值。

确有部分回滚需求时，可通过 `tx.begin()` 创建保存点。不要仅为形式引入嵌套事务。

## 迁移位置与命名

SQLx 支持顺序版本和时间戳版本，两者都有效：

```text
0001_create_users.sql
20260715103000_create_users.sql
```

Nexora 采用顺序版本，并将所有迁移集中到 `crates/migrate/migrations`。业务名称写入描述，便于识别归属：

```text
crates/migrate/migrations/
├── 0001_accounts_create_rbac.up.sql
├── 0001_accounts_create_rbac.down.sql
├── 0002_accounts_add_super_admin.up.sql
└── 0002_accounts_add_super_admin.down.sql
```

不要把迁移散落在 `modules/accounts`、`crates/database` 或 `examples/server`。`crates/migrate` 是唯一迁移执行边界。

## 可逆迁移

使用 sqlx-cli 的 reversible migration 生成同版本 `.up.sql`/`.down.sql` 文件：

```sql
-- 0003_accounts_add_email.up.sql
ALTER TABLE account.users ADD COLUMN email TEXT;
COMMENT ON COLUMN account.users.email IS '用户联系邮箱；允许为空';
```

```sql
-- 0003_accounts_add_email.down.sql
ALTER TABLE account.users DROP COLUMN email;
```

不要只在 up 文件中用注释记录 down 语句。回滚会丢失数据时，应明确风险并设计 expand/migrate/contract 或备份方案，而不是假装完全可逆。

## 迁移安全规则

1. 不修改已进入共享环境的迁移；新增后续迁移修正。
2. 评估 DDL 锁范围和执行时长，必要时分阶段部署。
3. 大表回填与 schema 变化分开，支持批处理和重启。
4. 新旧应用需要并行运行时，先扩展兼容 schema，再迁移数据，最后收缩。
5. `IF EXISTS`/`IF NOT EXISTS` 不能替代正确版本管理；只在确有幂等重试需求时使用。
6. 在测试 PostgreSQL 上验证完整 up 顺序；需要承诺回滚时也验证对应 down。

## DDL 注释与封闭枚举

审查每个新建或新增的数据库对象：

- `CREATE TABLE` 后必须存在 `COMMENT ON TABLE`，并且每一个列都有 `COMMENT ON COLUMN`。
- 应用定义的 PostgreSQL 类型、具名约束、索引、函数和触发器必须说明用途；优先使用对应的 `COMMENT ON`。
- 稳定且封闭的有限集合必须建模为模块 schema 下的 PostgreSQL ENUM；禁止使用自由 `TEXT`、整数魔法值或 `CHECK (... IN (...))` 代替。
- `COMMENT ON TYPE` 说明枚举整体业务语义，`CREATE TYPE` 中每个标签旁使用中文 SQL 注释说明具体含义。
- Rust 数据库侧使用 `enum` 与 `#[derive(sqlx::Type)]`，核对 `type_name` 和 `rename_all = "snake_case"`；如果枚举还跨 HTTP 边界，在公共契约中定义独立的 Serde 枚举并显式映射，同时核对两侧 `snake_case` 值一致。
- 可能频繁增删、需要停用、排序、本地化或运行时管理的集合使用字典表，不要使用 PostgreSQL ENUM。

缺少表、列或类型注释属于明确的可维护性问题；稳定封闭字段使用自由文本且允许写入 Rust 无法表示的值时，按数据完整性问题报告。

## 审查问题

1. 连接池是否只在 composition root 创建并通过状态/store 共享？
2. 连接预算是否考虑数据库上限、应用副本和管理预留？
3. 是否存在 `LazyLock + Handle::block_on` 或每请求建池？
4. 原子操作的全部查询是否都在同一事务中？
5. 迁移是否集中在 `crates/migrate/migrations`？
6. 顺序或时间戳命名是否保持唯一、单调且符合项目约定？
7. reversible migration 是否真的成对提供 `.up.sql`/`.down.sql`？
8. 破坏性迁移是否有锁、数据和多版本兼容方案？
9. 新表、全部字段和应用定义对象是否具有完整中文注释？
10. 稳定封闭字段是否使用 PostgreSQL ENUM，并与 Rust `enum` 保持一致？
