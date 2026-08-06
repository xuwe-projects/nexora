---
title: Introduction
order: 1
---

# Introduction

Nexora composes GPUI desktop registration, navigation, Account authentication and authorization,
and common server bootstrap into a Rust full-stack framework. It does not replace GPUI,
gpui-component, Axum, or SQLx.

## Dependency boundaries

- Applications depend on and import `gpui` and `gpui_component` directly.
- `nexora` provides Feature, Window, Application, Server, configuration, and Account contracts.
- The server creates one `PgPool`, shared by Nexora modules and application routers.
- Nexora manages framework objects through `server::migrate(&PgPool)` with history fixed at
  `nexora._sqlx_migrations`. The host then runs its application Migrator against the same pool;
  their histories remain independent.

## Public features

| Feature | Purpose |
| --- | --- |
| `desktop` | GPUI runtime and Account client capabilities |
| `server` | Axum runtime, Account, ZITADEL, and default Setup |
| `derive` | Feature, Window, Settings, and related derives |
| `cli` | The `nexora` command |

Desktop packages normally enable `desktop, derive`; generated servers enable `server, derive`.
