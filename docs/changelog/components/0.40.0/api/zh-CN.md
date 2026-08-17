---
title: API 0.40.0
---

- 服务账号支持按 username 创建、确认复用或导入既有 ZITADEL machine user。
- 系统角色定义和用户完整角色集合同步到 ZITADEL Project；首次分配时自动补齐既有本地角色定义。
- 删除服务账号凭据 HTTP/Rust API、权限和本地元数据表；保留 PAT introspection 鉴权配置。
