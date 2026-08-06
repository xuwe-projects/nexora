---
name: manage-sqlx-migrations
description: 管理 Nexora 与下游项目的 PostgreSQL DDL、SQLx migration、Migrator、sqlx.toml 和数据库测试。凡涉及创建、修改、审查或执行迁移，设计表、列、类型、约束、索引、函数或触发器，调整迁移历史表或启动顺序，以及新增、修改或运行 #[sqlx::test] 时必须使用。
---

# 管理 SQLx 迁移

## 先确认边界

1. 读取仓库 `AGENTS.md`、根 `Cargo.toml`、`sqlx.toml`、迁移目录、宿主启动入口和数据库测试。
2. 确认当前工作树、已提交迁移、迁移是否执行过、目标 PostgreSQL 数据源和用户授权范围。
3. 保留宿主创建的唯一 `PgPool`；禁止由迁移库、业务模块或测试辅助逻辑创建第二套长期连接池。
4. 使用 SQLx 0.9 原生 `Migrator`；不要自建迁移版本、checksum 或历史协调协议。

## 创建与维护迁移

- 只用 `sqlx migrate add <module>_<description>` 创建新迁移。通过 `sqlx.toml` 选择目录、可逆类型和 timestamp 版本；禁止手写文件名、版本号或运行不存在的 `sqlx migrate init`。
- 默认禁止修改、删除或重命名已提交迁移。历史错误通过新的后续迁移修复。
- 只编辑当前任务中由 SQLx CLI 新建、尚未提交且尚未在任何数据库执行的迁移。
- 只有用户明确授权受控基线重构，并确认无需兼容既有数据库历史时，才允许删除旧迁移并重建基线；记录授权、范围、旧数据库处置和下游清库要求。
- 迁移平铺在配置的 migrations 目录。按模块拆分清晰、可逆的迁移，避免一份巨型迁移。
- up 文件只描述目标结构和必要基础数据；down 文件按依赖逆序删除本迁移拥有的对象。

## 编写 DDL

- 使用模块 schema 和完整限定名，不依赖共享连接池的 `search_path`。
- 为每个 TABLE、COLUMN、应用定义的 TYPE、具名约束、索引、函数和触发器写完整中文 `COMMENT`。
- 稳定封闭集合使用 PostgreSQL ENUM，并让 Rust `sqlx::Type` 映射与数据库标签保持 `snake_case` 一致；开放集合使用字典表。
- 明确主键、外键、唯一性、非空、检查约束、级联行为、索引目的和回滚影响。
- 不把演示或临时测试数据写入生产迁移。

## 配置与启动顺序

- 在根 `sqlx.toml` 中维护 `[migrate]` 的 migrations 目录、需自动创建的 schema、迁移历史表，以及 `[migrate.defaults]` 的可逆类型和 timestamp 版本。
- Nexora 框架对象由 `nexora::server::migrate(&PgPool)` 先迁移，并使用框架独立历史表；应用随后用自己的 `Migrator` 和应用历史表迁移业务对象。
- 不合并 Nexora 与应用迁移，不协调跨来源版本，不复制 Nexora migration，不创建第二连接池。
- 迁移全部成功后再初始化 Account、构造 Router 和接收流量。

## 数据库测试

- 保留全部数据库测试和 `#[sqlx::test]`；禁止通过删除、忽略或改成无数据库测试来绕过失败。
- 在启动 Cargo/SQLx 测试的当前进程中，从项目标准 `config/server.toml` 解析唯一基础数据库 URL，再设置 `DATABASE_URL`。禁止读取 `.env`，也禁止私自选择另一套主机、端口、账号、密码或 TLS 配置。
- 允许 SQLx 基于该 URL 自动创建和清理隔离测试数据库；禁止手工创建长期存在的 `test`、`e2e` 或 `codex` 数据库。
- 测试失败后按本次运行产生的精确数据库名称清理遗留项；禁止用宽泛名称匹配删除数据库。
- 不在命令输出、日志、提交、Release Notes 或错误消息中打印数据库 URL、密码、令牌或其他秘密。

## 验证

1. 在隔离数据库验证完整 `run`、重复 `run` 幂等、`info`、`revert` latest 后重新 `run`。
2. 查询实际 migration history schema/table，并核对表、列、类型、约束、索引、函数、触发器及其注释。
3. 运行全部 `#[sqlx::test]` 和数据库集成测试，失败后精确检查并清理本次遗留数据库。
4. 运行仓库要求的格式化、测试、Clippy、lint、脚手架消费者和文档门禁。
5. 如实记录每条已执行命令和结果；不得把未执行或失败的检查写成通过。
