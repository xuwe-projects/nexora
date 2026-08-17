---
title: API 0.38.2
---

- 修复 `UserListQuery` 的扁平 URL query 反序列化，正常的 `page` 与 `page_size` 不再触发
  `invalid_query_parameter`。
- 保留现有 Rust 字段结构、HTTP wire 参数、筛选枚举、默认值和未知字段拒绝行为。
