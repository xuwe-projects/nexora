---
title: API 0.38.0
---

- Account 新增显式服务账号类型、machine user 创建、资料管理以及 Client Credentials/PAT 凭据闭环。
- JWT 继续本地校验，PAT/opaque token 改为每次请求实时 introspection；暂停账号会立即触发本地拒绝。
- Secret 与 PAT 只交付一次且不落库，凭据列表仅返回可协调的非敏感元数据。
