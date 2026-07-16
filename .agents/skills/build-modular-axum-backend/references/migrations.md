# 数据库迁移约定

## 统一职责

使用 `crates/migrate` 作为唯一数据库结构管理入口。业务模块只执行运行时查询，不创建、不嵌入、不执行迁移。

```text
crates/migrate/
├── Cargo.toml
├── migrations/
│   ├── 202607150001_account_create_accounts.sql
│   ├── 202607150002_account_add_status.sql
│   └── 202607150003_warehouse_create_warehouses.sql
├── seeds/
│   ├── account/
│   │   └── accounts.sql
│   └── warehouse/
│       └── warehouses.sql
└── src/
    ├── config.rs
    └── main.rs
```

## 迁移目录规则

SQLx 0.9 默认迁移加载器只读取指定目录的一级文件，不会递归扫描子目录。因此必须保持 `migrations/` 扁平：

```text
# 正确
migrations/202607150001_account_create_accounts.sql
migrations/202607150002_warehouse_create_warehouses.sql

# 错误：account 目录及其内容会被默认加载器忽略
migrations/account/202607150001_create_accounts.sql
```

所有模块迁移共用一套全局版本顺序和 `_sqlx_migrations` 表。通过文件名中的模块名实现归类，不要给不同模块分配独立迁移目录或独立版本空间。

## 迁移文件规则

- 使用 `<版本>_<模块>_<说明>.sql` 命名，版本采用可排序时间戳或项目既有连续编号。
- 版本号在整个项目中必须全局唯一，并按跨模块依赖顺序排列。
- 每个迁移只表达一个清晰的结构变更。
- 已应用的迁移文件禁止修改或重新排序；后续修正必须新增迁移。
- 正常迁移禁止使用 `DROP TABLE` 重置数据库。
- 每个模块默认使用与模块名一致的独立 PostgreSQL schema。
- 首个模块迁移必须先执行 `CREATE SCHEMA IF NOT EXISTS <module>`。
- 表、序列、索引和约束必须创建在对应模块 schema 中。
- 创建表时明确添加主键、唯一约束、非空约束和必要索引。
- 每个新表必须提供 `COMMENT ON TABLE`，并为每一个列提供 `COMMENT ON COLUMN`；禁止只靠字段名猜测业务语义。
- 为应用创建的 PostgreSQL 类型、具名约束、索引、函数和触发器记录用途；优先使用对应的 `COMMENT ON`，无法直接附加对象注释时使用紧邻对象的中文 SQL 注释。
- 语义稳定且取值封闭的有限集合使用 PostgreSQL ENUM，禁止退化为 `TEXT`、整数魔法值或 `CHECK (... IN (...))`。
- PostgreSQL ENUM 创建在模块 schema 中，使用 `COMMENT ON TYPE` 说明业务含义，并在 `CREATE TYPE` 中逐项使用中文行内注释解释每个枚举值。
- PostgreSQL ENUM 必须有对应的 Rust `enum` 和 `sqlx::Type` 映射；数据库标签和 Rust 映射统一使用 `snake_case`。
- 可能频繁增删、需要停用、排序、本地化或由运行时配置的值集合使用字典表，不要使用难以删除或重命名值的 PostgreSQL ENUM。
- PostgreSQL 自增主键默认使用 `BIGSERIAL`，Rust 对应 `i64`。
- 必须存在的基础业务数据可以进入版本化迁移；演示和测试数据必须进入 `seeds/<module>/`。

迁移示例：

```sql
CREATE SCHEMA IF NOT EXISTS warehouse;

-- 季节是稳定且封闭的业务集合，因此使用 PostgreSQL ENUM。
CREATE TYPE warehouse.season AS ENUM (
    'spring', -- 春季：通常指一年中的第一季度生长季。
    'summer', -- 夏季：通常指高温生产或销售季。
    'autumn', -- 秋季：通常指收获季。
    'winter'  -- 冬季：通常指低温或休整季。
);

COMMENT ON TYPE warehouse.season IS
    '季节枚举：spring=春季，summer=夏季，autumn=秋季，winter=冬季';

CREATE TABLE warehouse.warehouses (
    id         BIGSERIAL,
    name       TEXT NOT NULL,
    season     warehouse.season NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT warehouses_pkey PRIMARY KEY (id),
    CONSTRAINT warehouses_name_unique UNIQUE (name)
);

COMMENT ON TABLE warehouse.warehouses IS '仓库主数据；记录仓库名称和适用季节';
COMMENT ON COLUMN warehouse.warehouses.id IS '仓库稳定主键';
COMMENT ON COLUMN warehouse.warehouses.name IS '仓库展示名称，在系统内唯一';
COMMENT ON COLUMN warehouse.warehouses.season IS '仓库适用季节，取值来自 warehouse.season';
COMMENT ON COLUMN warehouse.warehouses.created_at IS '记录创建时间，数据库内部使用带时区时间';
COMMENT ON COLUMN warehouse.warehouses.updated_at IS '记录最后更新时间，数据库内部使用带时区时间';
COMMENT ON CONSTRAINT warehouses_pkey ON warehouse.warehouses IS '保证每个仓库具有唯一主键';
COMMENT ON CONSTRAINT warehouses_name_unique ON warehouse.warehouses IS '防止创建重名仓库';

CREATE INDEX warehouses_season_idx ON warehouse.warehouses (season);
COMMENT ON INDEX warehouse.warehouses_season_idx IS '支持按适用季节筛选仓库';
```

对应的 Rust 数据库枚举使用 SQLx 类型映射，不要把枚举列读取为 `String`：

```rust
/// 仓库业务使用的稳定季节集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "season", rename_all = "snake_case")]
pub(crate) enum Season {
    /// 春季。
    Spring,
    /// 夏季。
    Summer,
    /// 秋季。
    Autumn,
    /// 冬季。
    Winter,
}
```

运行时 SQL 中的类型、表和跨 schema 引用继续使用完整限定名；需要显式 cast 时写成
`$1::warehouse.season`，禁止依赖 `search_path`。

测试数据示例：

```sql
INSERT INTO <schema>.<modules> (...)
VALUES
    (...),
    (...),
    (...)
ON CONFLICT DO NOTHING;
```

跨模块外键使用完整限定名，例如：

```sql
REFERENCES account.accounts(id)
```

迁移版本必须保证被引用的 schema 和表先创建。所有模块仍使用同一套 `_sqlx_migrations` 表，不为不同 schema 创建独立迁移记录表。

## migrate crate

把 `migrate` 作为工作区二进制 crate：

```toml
[package]
name = "migrate"
version.workspace = true
edition.workspace = true

[dependencies]
configuration.workspace = true
serde.workspace = true
sqlx.workspace = true
tokio.workspace = true
```

迁移程序读取调用方指定的配置文件，没有参数时默认使用 `config/server.toml`，然后运行内嵌迁移：

```rust
let settings = LayeredConfigLoader::<MigrationConfig>::new()
    .with_required_file(config_path)
    .load()?;

let pool = PgPoolOptions::new()
    .connect(&settings.database.url)
    .await?;

sqlx::migrate!("./migrations").run(&pool).await?;
```

`MigrationConfig` 由 migrate crate 自己定义，不要依赖 `examples/server` 的配置类型。

## 执行和验证

只在用户明确要求迁移目标数据库时运行：

```bash
cargo run -p migrate -- config/server.toml
```

测试数据必须通过独立、显式的命令导入，不能由生产迁移程序自动执行。执行前说明目标数据库以及写入影响。

完成后运行：

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
