---
title: 配置
order: 1
---

# 配置

根配置派生 serde 与 `nexora::Settings`：

```rust
#[derive(serde::Deserialize, nexora::Settings)]
struct Settings {
    api: nexora::desktop::ApiSettings,
    #[nexora(account_client)]
    account: nexora::desktop::AccountSettings,
}
```

桌面 API 使用独立 `[api]` 表：

```toml
[api]
endpoint = "http://127.0.0.1:3000"
```

服务端监听 IP 与端口分开配置：

```toml
[server]
ip = "127.0.0.1"
port = 3000
```

启用 Account 服务端时还需要 ZITADEL 管理配置：

```toml
[oidc]
issuer_url = "https://identity.example.com"
audience = "nexora-api"
organization_id = "zitadel-organization-id"
project_id = "zitadel-project-id"
personal_access_token = "replace-through-secret-injection"
```

`organization_id` 用于 UserService v2 创建人类用户；`project_id` 用于同步系统角色，两者职责
不同。PAT 必须属于有权管理目标 Organization/Project 的服务账号，生产环境应通过
`OIDC__PERSONAL_ACCESS_TOKEN` 或密钥系统注入，不要提交真实值。

环境变量以 `__` 表示嵌套字段。显式路径优先；未传路径时根据当前 package 名查找
`config/<package>.toml`。敏感值应由环境变量或密钥系统注入。

服务端 Setup secret 只在未初始化时有效。迁移记录由 `_sqlx_migrations` 管理，不需要也不
允许通过 `initialize_empty_database` 之类的人工布尔开关控制升级。
