## ZITADEL 联系手机号与已验证创建

- `CreateHumanIdentity::with_contact_phone` 支持把登录手机号同步写入 ZITADEL human phone/mobile 联系信息。
- ZITADEL 创建 human user 时 email 和 contact phone 默认标记为已验证，不再发送邮箱或短信验证码。
- Account 本地库不新增 phone 字段，未提供手机号时保持旧创建行为。
