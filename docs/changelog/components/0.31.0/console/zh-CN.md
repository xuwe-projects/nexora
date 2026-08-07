---
title: Console 0.31.0
---

- 本版本不改变 Console 窗口、Account 管理交互、Updater 协议或发布配置。
- 使用 Account 服务端能力的下游必须随 v0.31.0 清库重建 PostgreSQL 17 数据库；回滚时同时
  恢复 v0.30.1 数据库快照、二进制和依赖。
