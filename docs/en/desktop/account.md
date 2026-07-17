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
- a default user page for provisioning, status, and role management;
- a default role page for custom-role and permission-set management;
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

## Default management capabilities

`/users` lists and refreshes users, provisions OIDC identities already confirmed by an
administrator, selects initial roles, activates or suspends regular users, and completely replaces
a regular user's direct role set. Provisioning does not create an Identity Provider account or add
a local password. An empty initial role set requires only `users:provision`; a non-empty set also
requires `users:roles.write`. Listing choices and editing a user's roles also require `roles:read`.

`/roles` lists roles and the permission catalog, creates custom roles with initial permissions,
edits names and descriptions, completely replaces permission sets, and deletes custom roles.
Creation with initial permissions, updates, permission replacement, and deletion all use
`roles:write`; listing selectable permissions requires `permissions:read`. Built-in roles remain
immutable.

The pages disable unavailable actions and explain the required permission using the current login
profile. The server still enforces super-administrator, built-in-role, and last-active-administrator
invariants. Default user management intentionally does not delete local users.

## Override defaults

Define an ordinary Feature with the same ID or path to replace `/users` or `/roles` individually.
Custom pages can call `nexora::desktop::api_session(cx)` to obtain the public user-provisioning,
status, user-role, role, and permission methods without exposing the bearer token.

Use `LoginFeature` for a complete login layout replacement. Structured failures remain available
through `login_snapshot(cx).failure`.
