# Nexora 框架数据库迁移

本 crate 只拥有 Nexora 框架数据库对象。公开入口为：

```rust
nexora::server::migrate(pool: &sqlx::PgPool)
```

入口借用宿主创建的唯一连接池，使用 SQLx 0.9 原生 `Migrator`，自动创建 `nexora` schema，
并把框架迁移历史固定记录在 `nexora._sqlx_migrations`。它不返回迁移列表，也不与应用迁移
合并、重编号或协调 checksum。

## 文件与配置

迁移平铺在 `crates/migrate/migrations`。根 `sqlx.toml` 固定以下规则：

- migrations 目录为 `crates/migrate/migrations`；
- 自动创建 `nexora` schema；
- 历史表为 `nexora._sqlx_migrations`；
- 新迁移默认为 reversible timestamp。

只能使用 SQLx CLI 创建新迁移：

```bash
sqlx migrate add <module>_<description>
```

禁止手写文件名或版本号，也不要使用不存在的 `sqlx migrate init`。已提交迁移不得修改、删除
或重命名；历史错误通过新增迁移修复。只有用户明确授权无需兼容旧数据库的受控基线重构时，
才允许重建迁移历史。

当前 v0.31.0 基线以 PostgreSQL 17 为最低受支持数据库版本。列级 `NOT NULL` 直接写在列定义
中，不为其声明 PostgreSQL 18 才能作为独立 `pg_constraint` 注释的约束名；列职责继续通过
`COMMENT ON COLUMN` 记录。v0.29.0 与 v0.30.x 的框架迁移历史不能原地升级到该基线，必须先
保护需要保留的数据，再清库重建，并且不得恢复旧 DDL、旧 migration history 或旧 checksum。

## 宿主启动顺序

```rust
let pool = sqlx::postgres::PgPoolOptions::new()
    .connect(settings.database.url.as_str())
    .await?;

nexora::server::migrate(&pool).await?;
application_migrate::migrate(&pool).await?;
server.initialize(&settings, &pool, setup_secret).await?;
```

Nexora 与应用 Migrator 共享同一个 `PgPool`，但维护独立历史表。所有迁移成功后才能初始化
Account、构造 Router 和接收流量。

## 数据库测试

保留全部 `#[sqlx::test]`。使用项目标准 `config/server.toml` 为当前测试进程派生
`DATABASE_URL`，不读取 `.env`，也不选择另一套数据库数据源：

```bash
bash scripts/run-sqlx-tests.sh /path/to/config/server.toml \
  cargo test -p migrate --all-features --test data_preservation
```

包装脚本允许 SQLx 创建并清理隔离测试数据库；测试失败时只清理本次新增的精确数据库名。
