---
title: API 0.29.0
---

- 服务端迁移入口改为 `nexora::server::migrate(&PgPool)`；框架历史固定记录在
  `nexora._sqlx_migrations`，宿主随后使用同一个连接池运行独立的应用 Migrator。
- `nexora::server::migrations()`、MigrationPlan 和跨来源版本/checksum 协调已删除；旧数据库
  不支持原地升级，必须清库重建后依次运行两套迁移。
