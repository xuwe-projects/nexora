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

Environment variables use `__` for nesting. An explicit path wins; otherwise Nexora finds
`config/<package>.toml`. Inject secrets through environment variables or a secret manager.

The Setup secret is only useful before initialization. `_sqlx_migrations` records applied versions,
so upgrades must not depend on an `initialize_empty_database` boolean switch.
