---
title: Configuration
order: 1
---

# Configuration

Root configuration derives Serde and `nexora::Settings`:

```rust
#[derive(serde::Deserialize, nexora::Settings)]
struct Settings {
    api: nexora::desktop::ApiSettings,
    #[nexora(account_client)]
    account: nexora::desktop::AccountSettings,
}
```

The desktop API endpoint is a root table:

```toml
[api]
endpoint = "http://127.0.0.1:3000"
```

Server listen IP and port are separate fields:

```toml
[server]
ip = "127.0.0.1"
port = 3000
```

Account servers also require ZITADEL management settings:

```toml
[oidc]
issuer_url = "https://identity.example.com"
audience = "nexora-api"
organization_id = "zitadel-organization-id"
project_id = "zitadel-project-id"
personal_access_token = "replace-through-secret-injection"
```

`organization_id` selects where UserService v2 creates human users; `project_id` carries synchronized
system roles. Inject the service-account PAT through `OIDC__PERSONAL_ACCESS_TOKEN` or a secret manager.

Environment variables use `__` for nesting. An explicit path wins; otherwise Nexora finds
`config/<package>.toml`. Inject secrets through environment variables or a secret manager.

The Setup secret is only useful before initialization. `_sqlx_migrations` records applied versions,
so upgrades must not depend on an `initialize_empty_database` boolean switch.
