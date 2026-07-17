---
name: sqlx-database-code-review
description: 审查 Nexora workspace 中使用 SQLx 与 PostgreSQL 的数据库代码。检查查询与绑定、FromRow/Type 映射、PostgreSQL ENUM 与 Rust enum 一致性、DDL 对象注释、连接池、事务、迁移、错误上下文和数据库职责边界，并区分编译期查询宏与可审查的运行时查询策略。
---

# Nexora SQLx 数据库代码审查

## 先确定范围和事实

按顺序完成以下门禁，再给出严重程度：

1. 列出实际检查的 `Cargo.toml`、Rust 文件、迁移目录和 SQLx 配置。
2. 确认 workspace 使用的 SQLx 版本、features、PostgreSQL 类型支持和 Rust edition。
3. 检查相关 schema/迁移后再判断列、约束、索引、可空性和类型映射。
4. 为每个发现提供 `[FILE:LINE]`，并按 [审查验证协议](references/review-verification-protocol.md) 验证触发条件和影响。

不要根据通用偏好报告问题。缺少 schema、部署容量或运行数据时，明确证据边界并降低结论强度。

## 遵守项目数据库边界

- `crates/database` 负责连接池建立、健康检查和通用数据库错误上下文。
- `crates/migrate` 负责执行迁移，SQL 文件集中放在 `crates/migrate/migrations`。
- `modules/<business>/src/store.rs` 定义持久化端口，`store/postgres.rs` 及其子模块执行 SQLx 查询和事务。
- `crates/api` 的 Router/handler 不执行 SQL；业务 application 不持有具体 `PgPool`。
- 宿主在 `examples/server` composition root 中异步创建连接池、运行迁移并构造 Postgres store。
- 不建议新增 `Repository`/`Service` 命名；本项目使用 `Application`/`Store`。

## 审查查询

编译期宏和运行时查询都是有效策略，按约束选择：

- `query!`/`query_as!` 适合静态 SQL，能在准备好数据库元数据或离线缓存时校验列与类型。
- `query`/`query_as::<_, T>` 适合动态 SQL、`QueryBuilder`、可复用 `FromRow` 行类型，或构建环境无法提供宏校验元数据的场景。
- 不要仅因为使用运行时查询就报告 Major；继续审查 SQL、绑定参数、测试、`FromRow` 映射和数据库约束。
- 动态筛选使用 `QueryBuilder` 的 `.push_bind(...)`；绝不把不可信输入插入 SQL 字符串。
- PostgreSQL 参数使用 `$1`、`$2`，并核对绑定顺序、Rust 类型、SQL cast 和可空性。
- `.fetch_one()` 在零行时返回 `RowNotFound`，多行时返回第一行；它不会因多行报错。
- 预期“零或一行”时使用 `.fetch_optional()`；必须保证唯一时依赖主键/唯一约束或显式基数检查，而不是误以为 `fetch_one()` 会检查多行。
- 对有明确上限的小结果集使用 `.fetch_all()`；大结果集使用分页或 `.fetch()` 流式处理。

详细规则见 [查询参考](references/queries.md)。

## 审查行与类型映射

- `#[derive(sqlx::FromRow)]` 和 `query_as::<_, T>` 是可复用行类型的有效组合。
- `FromRow` 与 `Type` 是同步映射 trait；不要要求原生 `async fn` 或 `#[async_trait]`。
- schema、SELECT 列、列别名、Rust 字段、`FromRow` 和 `Type` 必须同步变化。
- 可空列使用 `Option<T>`；UUID、时间、金额和 JSON 使用对应结构化类型，不以字符串逃避类型映射。
- 稳定且封闭的有限取值字段使用 PostgreSQL ENUM 与 Rust `enum`，禁止用 `TEXT`、整数魔法值或 `CHECK (... IN (...))` 代替领域枚举。
- 自定义枚举同时核对 PostgreSQL ENUM 标签、`sqlx::Type` 和公共契约中对应 Serde 枚举的命名，并统一使用 `snake_case`；数据库枚举与 HTTP DTO 保持分离并显式映射。
- 可能频繁增删、需要停用或由业务配置的集合使用字典表；不要误把开放集合强制改成 PostgreSQL ENUM。
- edition 2024 中注意保留关键字 `gen`；保留数据库名称时使用 raw identifier 和明确 rename。

## 审查连接池

- `PgPoolOptions::new()` 的 `max_connections` 默认值是 10，不是 5。
- 是否需要覆盖默认值取决于 PostgreSQL 总连接预算、应用副本数、并发查询、管理预留和压测证据。
- 检查 acquire timeout、idle timeout、max lifetime 与连接建立错误的上下文。
- 在 composition root 中异步创建一次连接池，通过 `Database`/`AppState`/Postgres store 共享；不要每个请求创建连接池。
- `PgPool` 本身可廉价克隆，通常不需要 `Arc<PgPool>`。
- 禁止使用 `LazyLock`、`once_cell` 或其他静态容器配合 `tokio::runtime::Handle::block_on` 初始化连接池；这会制造嵌套运行时、panic 和启动顺序风险。

## 审查事务

- 需要原子性的多语句写入使用同一事务，并将 `&mut *tx` 传给所有相关查询。
- 成功路径显式 `commit()`；错误路径 drop 会回滚，但错误映射仍应保留操作上下文。
- 检查读改写竞争、锁顺序、唯一约束、受影响行数和幂等语义，而不只检查是否调用 `begin()`。
- 仅在确有局部回滚语义时使用保存点。

## 审查迁移

- SQLx 同时接受顺序版本和时间戳版本；两者都有效，不要把其中一种报告为错误。
- Nexora 统一使用顺序版本并集中到 `crates/migrate/migrations`，例如 `0003_accounts_add_email.up.sql`。
- 可逆迁移使用同版本成对的 `.up.sql`/`.down.sql`，不要把 down SQL 只写成 up 文件里的注释。
- 不修改已进入共享环境的迁移；通过后续迁移修正。
- 每个新表必须有 `COMMENT ON TABLE`，每个列必须有 `COMMENT ON COLUMN`；类型、具名约束、索引、函数和触发器也应记录用途。
- PostgreSQL ENUM 使用 `COMMENT ON TYPE` 描述类型，并在 `CREATE TYPE` 中逐项注明每个标签的中文含义。
- 检查扩展、约束、索引、锁影响、数据回填、回滚数据损失和多版本应用兼容窗口。
- `crates/migrate` 将框架与应用迁移合并、检查跨来源版本冲突后，由唯一 SQLx `Migrator`
  执行一次；业务 crate、HTTP handler 和框架初始化不自行运行迁移。

详细规则见 [迁移与连接池参考](references/migrations.md)。

## 严重程度

### Critical

- 不可信输入直接拼接 SQL，存在可利用的 SQL 注入。
- 必须原子的写入缺少事务，且失败会留下不可恢复的部分状态。
- 每个请求创建连接池，足以造成连接耗尽。

### Major

- Rust/SQL 类型或可空性不一致并会导致运行失败或数据解释错误。
- 新建的稳定封闭字段仍使用自由文本或整数，导致数据库可以持久化 Rust 枚举无法表示的值。
- 无边界读取大表并有明确内存或延迟风险。
- 事务遗漏查询、锁或约束导致已证实的竞争和一致性问题。
- 迁移存在明确数据丢失、长时间阻塞或无法部署风险。

### Minor / Informational

- 连接池参数缺少部署依据、查询列过宽、错误上下文不足或可维护性较差。
- 在已有离线校验基础设施时，可建议静态 SQL 使用查询宏，但不要把运行时查询本身判为缺陷。

## 不要误报

- `query_as::<_, T>` 配合 `FromRow`。
- 使用绑定参数的运行时静态查询。
- 经 `.push_bind(...)` 构建的动态查询。
- `.fetch_one()` 对唯一键查询返回第一行。
- 顺序版本或时间戳版本迁移。
- 外部或遗留 schema 已明确豁免，且当前变更不能安全迁移为 PostgreSQL ENUM 的文本枚举。
- 由外部系统管理迁移且本项目只读数据库，只要这是明确架构约束。

## 输出格式

先列发现，按严重程度排序；没有发现时明确写“未发现可报告问题”。每条使用：

```text
[FILE:LINE] 问题标题
Severity: Critical | Major | Minor | Informational
说明触发条件、实际影响、证据和最小修复方向。
```

最后列出已检查范围、验证命令和因环境限制未完成的检查。
