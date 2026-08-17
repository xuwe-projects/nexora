---
title: Console 0.40.0
---

- 服务账号创建页移除初始凭据选项；同 username 的 machine user 使用确认弹窗复用。
- 用户列表删除“账号与凭据”入口，凭据改由开发人员直接在 ZITADEL 管理。
- 用户角色增删通过统一 Account 流程同步本地 `account.user_roles` 与 ZITADEL authorization。
