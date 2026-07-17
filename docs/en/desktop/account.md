---
title: Account
order: 3
---

# Account

After an application installs `AccountAuthenticator` in `Application::initialize`, Nexora
automatically provides:

- an OIDC Authorization Code + PKCE login gate;
- failure notifications with a request ID copy action;
- `/users` and `/roles` navigation under Access Control;
- default user, role, and permission pages;
- avatar and display name rendering;
- business Feature and Window cleanup on sign-out.

## Install the client runtime

```rust
let settings: config::Settings = nexora::config::initialize(None)?;
let config = nexora::desktop::client_config(&settings, &settings.api)?;
let authenticator = nexora::desktop::AccountAuthenticator::new(&config)?;

nexora::desktop::install_authenticator(authenticator, cx);
```

There is no separate `account_enabled` switch. A regular desktop application that does not install
an authenticator gets neither the login gate nor the default `/users` and `/roles` pages.

## Override defaults

Define an ordinary Feature with the same ID or path to replace `/users` or `/roles` individually.
Custom pages can call `nexora::desktop::api_session(cx)` to obtain the public user, role,
and permission methods without exposing the bearer token.

Use `LoginFeature` for a complete login layout replacement. Structured failures remain available
through `login_snapshot(cx).failure`.
