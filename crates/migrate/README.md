# 数据库迁移

本 crate 是 PostgreSQL 结构变更的唯一执行入口。它读取服务端同形状的数据库配置，但定义
自己的 `MigrationConfig`，不依赖 `apps/server`。

## 文件组织

```text
crates/migrate/
├── migrations/                 # 所有模块共用的扁平迁移序列
├── seeds/<module>/             # 可选的本地测试数据
└── src/main.rs                 # 独立迁移程序
```

SQLx 迁移加载器只扫描 `migrations/` 一级目录，因此禁止按模块建立迁移子目录。文件使用
`<全局版本>_<模块>_<说明>.sql`，或项目现有的同版本 `.up.sql`/`.down.sql` 对。版本必须在整个
workspace 中唯一，已进入共享环境的迁移不得修改；后续修正必须新增更高版本。

每个业务模块使用与模块名一致的 PostgreSQL schema。当前账号模块的表、索引、函数和触发器
都位于 `account` schema，运行时 SQL 必须使用 `account.<table>` 完整限定名，不能依赖
共享连接的 `search_path`。

演示和测试数据只能进入 `seeds/<module>/`，并通过独立命令显式导入，不得混入生产迁移。

## 执行

从 workspace 根目录运行：

```bash
cargo run -p migrate -- config/server.toml
```

未传路径时默认读取 `config/server.toml`。环境变量最后加载并覆盖文件，例如：

```bash
DATABASE__URL=postgres://postgres:postgres@127.0.0.1:5432/xuwe \
  cargo run -p migrate -- config/server.toml
```

迁移会修改目标数据库结构；执行前应确认配置指向的数据库。测试数据若存在，可使用 PostgreSQL
客户端按需导入对应文件，例如：

```bash
psql "$DATABASE_URL" -f crates/migrate/seeds/<module>/<seed-file>.sql
```
