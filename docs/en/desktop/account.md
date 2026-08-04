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
- circular initial/default Avatar rendering and display name rendering;
- business Feature and Window cleanup on sign-out.

## Install the client runtime

```rust
let settings: config::Settings = nexora::config::initialize(None)?;
let config = nexora::desktop::client_config(&settings, &settings.api)?;
let authenticator = nexora::desktop::AccountAuthenticator::new(&config)?;

nexora::desktop::install_authenticator(authenticator, cx);
```

`ApiSettings` accepts HTTPS and loopback HTTP by default. To connect to an intranet plain-HTTP
service, set `allow_insecure_http = true` under `[api]`. Plain HTTP sends Bearer tokens in clear
text, so use it only in controlled environments where that risk is accepted.

There is no separate `account_enabled` switch. A regular desktop application that does not install
an authenticator gets neither the login gate nor the default `/users` and `/roles` pages.

## Session persistence and refresh

The desktop Account configuration guarantees that the OIDC scope contains `offline_access` without
duplicating a scope already supplied by the application. On Windows and macOS the default login gate
shows “Keep me signed in” selected on first use. The checkbox changes only non-sensitive preferences
in `workspace.toml`. On Linux it is always unchecked and disabled: Nexora does not use Secret Service
and never writes tokens to a file.

When selected, a successful login first stores a versioned record containing only the refresh token,
OIDC subject, minimal profile, and record version in macOS Keychain or Windows Credential Manager.
Only after that succeeds is `recovery_allowed` committed. On restart, silent recovery runs only when
the preference and security-store marker allow it, and it calls Account `/me` again before opening
the business shell. Recovery never opens a browser. Refresh-token rotation disables the old recovery
marker before saving the new record. A temporary security-store failure does not sign out the current
process; later refreshes retry the save.

Access tokens are refreshed in the background roughly 60 seconds before expiry. Network, provider 5xx,
and Account 5xx failures retain a potentially valid refresh token and use bounded backoff. `invalid_grant`,
subject mismatch, `account_suspended`, and `account_not_registered` disable recovery, remove local
credentials, and return to the login gate. Generation checks discard results from replaced or signed-out
tasks, while a same-account profile/permission refresh preserves business Features and Windows.

Interactive login uses the typed `prompt=select_account` parameter, so users can choose another
browser account. The gate exposes “Retry recovery” and “Use another account”; the latter is local and
does not perform provider-wide logout.

## Sign-out and revocation

`sign_out()` immediately clears the in-memory session and business gate, then asynchronously writes
`recovery_allowed=false`, deletes the secure credential, and best-effort revokes the refresh token at
the provider's `revocation_endpoint`. The request contains only `token`,
`token_type_hint=refresh_token`, and `client_id`; no client secret is sent. Missing or failed revocation
does not block local sign-out. Nexora does not call `end_session_endpoint` or clear browser cookies,
so the ZITADEL SSO session remains available.

## Default management capabilities

`/users` uses a card-styled, content-height DataTable with circular initial/default Avatar markers, login usernames, compact status
tags, movable columns, resizable widths, and bottom-triggered continuous loading. The server creates
the human user through ZITADEL gRPC and binds the returned stable identity ID; the UI never asks for
that internal ID and no local password is introduced. `GET /me` refreshes username, email, and display
name from ZITADEL. The page also selects initial roles, changes access status, and replaces
direct roles. An empty initial role set requires only `users:provision`; a non-empty
set also requires `users:roles.write`. Listing choices and editing roles also require `roles:read`.

`/roles` lists roles and the permission catalog, creates custom roles with initial permissions,
edits names and descriptions, completely replaces permission sets, and deletes custom roles.
Create and edit use panel-scoped FormDialog instances. The system-administrator role is marked
separately and automatically receives newly registered permissions.
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
through `login_snapshot(cx).failure`. The same non-sensitive snapshot exposes `busy`, `restoring`,
`remember_login`, `secure_storage_supported`, and `can_retry_recovery`. Custom login pages can call
`set_remember_login`, `retry_recovery`, and `login_with_other_account` without accessing Keychain or
any token.
