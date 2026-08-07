---
title: API 0.31.0
---

- 修复框架 Account 基线在 PostgreSQL 17 空库执行命名 `NOT NULL` 约束注释时失败的问题；
  非空约束、列注释和最终 schema 语义保持不变。
- v0.29.0 与 v0.30.x 数据库必须清库重建后依次运行 Nexora 与应用迁移，不得恢复旧迁移历史、
  旧 DDL 或 checksum。
