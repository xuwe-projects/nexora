---
title: API 0.39.0
---

- 支持 `identity_id = null` 的内部服务主体，同时强制人员账号必须绑定 Provider 身份。
- 普通用户集合排除内部主体；Provider 资料与凭据接口对其返回
  `409 internal_service_account`，不会访问 ZITADEL。
- `User` 和 `UserResponse` 的公开 Rust `identity_id` 字段改为 `Option<String>`。
