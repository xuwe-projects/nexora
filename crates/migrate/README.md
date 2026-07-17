# 数据库迁移

本 crate 是 PostgreSQL 结构变更的唯一所有者。它同时提供独立 `migrate` 命令和可由
Nexora Account composition root 调用的 library API；两种入口共享同一套 fail-closed 检查，
且都不依赖 `examples/server`。

## 文件组织

```text
crates/migrate/
├── migrations/                 # 所有模块共用的扁平迁移序列
├── seeds/<module>/             # 可选的本地测试数据
└── src/
    ├── lib.rs                  # 可组合的 prepare/plan.run API
    └── main.rs                 # 复用 library API 的独立迁移程序
```

SQLx 迁移加载器只扫描 `migrations/` 一级目录，因此禁止按模块建立迁移子目录。文件使用
`<全局版本>_<模块>_<说明>.sql`，或项目现有的同版本 `.up.sql`/`.down.sql` 对。版本必须在整个
workspace 中唯一，已进入共享环境的迁移不得修改；后续修正必须新增更高版本。

每个业务模块使用与模块名一致的 PostgreSQL schema。当前账号模块的表、索引、函数和触发器
都位于 `account` schema，运行时 SQL 必须使用 `account.<table>` 完整限定名，不能依赖
共享连接的 `search_path`。

演示和测试数据只能进入 `seeds/<module>/`，并通过独立命令显式导入，不得混入生产迁移。

## 执行

首次安装与已有数据库的日常升级使用同一个幂等命令：

```bash
cargo run -p migrate -- config/server.toml
```

空数据库会执行全部迁移，已有数据库只执行 `_sqlx_migrations` 尚未记录的向前版本。已有
account schema 却没有迁移历史、存在失败记录或账号核心表缺失时仍会拒绝继续，避免把
schema 丢失或存储故障伪装成正常升级。迁移 runner 不提供自动回滚或清库入口。

未传路径时默认读取 `config/server.toml`。环境变量最后加载并覆盖文件，例如：

```bash
DATABASE__URL=postgres://postgres:postgres@127.0.0.1:5432/nexora \
  cargo run -p migrate -- config/server.toml
```

迁移会先打印不含凭据的数据库名、服务器地址、已应用数量和待应用版本。执行前仍应确认目标并
完成备份。测试数据若存在，可使用 PostgreSQL 客户端按需导入对应文件，例如：

```bash
psql "$DATABASE_URL" -f crates/migrate/seeds/<module>/<seed-file>.sql
```

框架使用方不需要启动另一个进程，也可以在服务端创建共享 `PgPool` 后显式组合：

```rust
nexora::server::migrate(&pool).await?;
```
