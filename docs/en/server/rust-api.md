---
title: Complete Rust server API reference
order: 3
---

# Complete Rust server API reference

This page documents the public host-facing API available from `nexora::server` with the
`server,derive` features. See [Complete HTTP API reference](./http-api) for routes and JSON DTOs.

## Ownership boundary

`nexora::server` exposes migrations, Account/OIDC composition, composable routers, authorization
extractors, Setup extension points, and trusted host write functions. It does not create a second
pool, bind a listener, execute migrations, authorize trusted function calls, or own TLS, logging,
timeouts, limits, and graceful shutdown.

## `Server` lifecycle

### `Server::new`

```rust
pub const fn new() -> Server
```

Creates an empty, I/O-free composer. It owns no pool, listener, or Axum State.

### `Server::initialize`

```rust
pub async fn initialize<S>(
    &mut self,
    settings: &S,
    pool: &PgPool,
    setup_secret: &str,
) -> Result<(), ServerError>
```

`settings` must expose `AccountSettings` through `#[nexora(account_server)]`; `pool` is the host's
single shared PostgreSQL pool; `setup_secret` comes from secure configuration. Initialization
performs OIDC discovery, creates the verifier and ZITADEL directory, binds/verifies the deployment
issuer, reads initialization status, and synchronizes system roles for an initialized deployment.
It does not run migrations.

`ServerError` distinguishes Account dependency initialization, Account database/domain operations,
and ZITADEL directory synchronization.

### `Server::routers`

```rust
pub fn routers<S>(&self) -> Router<S>
where S: Clone + Send + Sync + 'static
```

Returns Setup and Account routers adapted to host State `S`, or an empty Router before successful
initialization. The call is synchronous and I/O-free.

### `Server::account`

```rust
pub fn account(&self) -> Option<Account>
```

Returns `None` before initialization and a cheaply cloneable facade afterwards. Clones share the
same pool and token verifier. Put it in host State and implement `FromRef<AppState> for Account` to
use `AuthenticatedUser` and `Authorized<P>` in host handlers.

### `Server::setup_url`

```rust
pub fn setup_url(&self, address: SocketAddr) -> Option<String>
```

Returns the pending `/setup` URL only before initialization. Wildcard bind addresses are rendered as
`127.0.0.1`. This only formats a URL and never owns the listener.

## Migrations

```rust
pub fn migrations() -> Vec<sqlx::migrate::Migration>
```

Returns cloned embedded migration metadata without database I/O. Merge it with application
migrations, reject cross-source version collisions, and run exactly one SQLx `Migrator` against the
host pool.

## Configuration and dependency helpers

`AccountSettings` contains `oidc: AccountOidcSettings` with:

| Field | Meaning |
| --- | --- |
| `issuer_url` | Canonical Provider issuer; HTTPS in production, loopback HTTP for development |
| `audience` | Required access-token audience |
| `organization_id` | ZITADEL Organization where `POST /users` creates human users |
| `project_id` | ZITADEL Project that carries system roles |
| `personal_access_token` | Server credential for ZITADEL UserService/ProjectService; redacted in Debug |

Advanced hosts may compose without `Server`:

```rust
pub async fn dependencies<S>(
    pool: PgPool,
    settings: &S,
) -> Result<AccountDependencies, AccountServerInitializationError>;

pub fn user_directory<S>(
    settings: &S,
) -> Result<ZitadelUserDirectory, DirectoryError>;
```

`dependencies` performs OIDC discovery and issuer binding. `user_directory` constructs the ZITADEL
gRPC clients. Neither runs migrations or starts HTTP.

`ZitadelUserDirectory` is also public for custom setup flows. `new` accepts issuer, PAT,
Organization ID, and Project ID and creates the gRPC clients;
`ensure_project_roles(&[SystemRole])` idempotently creates Project roles;
`list_active_human_users()` reads at most 10,000 enabled human users; and
`active_human_user(identity_id)` re-verifies one selection. Calls use a 15-second timeout.
It also implements `IdentityDirectory`: `identity` refreshes a profile, `create_human_identity`
creates a UserService v2 human user, sets the initial password, requests email verification, and
`delete_identity` supports compensation after a failed local transaction.
`DirectoryUser` carries identity ID, username, display name, optional email, and optional avatar;
`into_external_identity()` preserves those trusted fields. `DirectoryError` distinguishes invalid
configuration, TLS, UserService/ProjectService requests, invalid UTF-8, and the safety limit, without
exposing PAT metadata.

## `Account` facade

### Deployment and authentication

| Method | Output and behavior |
| --- | --- |
| `Account::new(AccountDependencies)` | I/O-free facade construction |
| `bind_identity_issuer(&PgPool, issuer)` | First atomic bind or equality verification |
| `initialization_status()` | `Required` or `Completed { super_admin }` |
| `is_system_initialized()` | Initialization boolean |
| `system_roles()` | Roles that must be synchronized to the Identity Provider project |
| `initialize(AccountInitialization)` | Transactional `Initialized` or idempotent `AlreadyInitialized` |
| `authenticate(token)` | Validated `AccessProfile`, including local registration/status checks |
| `authorize(token, PermissionKey)` | Authentication plus permission check |

### Queries and writes

| Method | Input | Output / semantics |
| --- | --- | --- |
| `register_permissions` | permission definitions | Idempotently upsert metadata and grant every registered permission to the system administrator in the same transaction |
| `permissions` | none | Complete permission catalog |
| `roles`, `role` | optional role ID | Roles with direct permissions |
| `create_role` | key, name, description, permission IDs | Transactional custom role creation |
| `update_role` | ID, optional name, three-state description | System roles are immutable |
| `delete_role` | ID | Rejects system or referenced roles |
| `replace_role_permissions` | ID and complete ID set | Atomic replacement; empty clears |
| `users` | page and page size | `Page<User>`; size clamped to 1–100 |
| `user_access` | user ID | `AccessProfile` |
| `refresh_user_from_directory` | identity ID | Synchronizes username, email, display name, avatar, and last-login time without creating unknown users |
| `update_user_status` | user ID and status | Protects the super admin and final enabled admin |
| `replace_user_roles` | user ID, complete roles, actor ID | Atomic replacement retaining `member` |
| `provision_user` | trusted `ExternalIdentity` | Creates a user without a local password |
| `provision_user_with_roles` | identity, roles, actor ID | Transactional user and initial role creation |
| `create_managed_user_with_roles` | `CreateHumanIdentity`, roles, actor ID | Creates the Provider user with `initial_password`, binds it locally, and attempts Provider deletion if the local transaction fails |
| `routers::<S>` | none | Router with private Account State injected |

Facade writes do not authorize the current caller. Built-in HTTP handlers apply `Authorized<P>`;
direct host calls must enforce equivalent authorization.

## Pool-first trusted host functions

```rust
pub async fn create_permissions(
    pool: &PgPool,
    definitions: &[PermissionDefinition],
) -> Result<Vec<Permission>, AccountError>;

pub async fn create_role(
    pool: &PgPool,
    key: &str,
    name: &str,
    description: Option<&str>,
    permission_ids: &[i64],
) -> Result<Role, AccountError>;

pub async fn replace_role_permissions(
    pool: &PgPool,
    role_id: i64,
    permission_ids: &[i64],
) -> Result<Role, AccountError>;

pub async fn create_user(
    pool: &PgPool,
    identity: ExternalIdentity,
) -> Result<User, AccountError>;

pub async fn create_user_with_roles(
    pool: &PgPool,
    identity: ExternalIdentity,
    role_ids: &[i64],
    granted_by: &str,
) -> Result<User, AccountError>;

pub async fn replace_user_roles(
    pool: &PgPool,
    user_id: &str,
    role_ids: &[i64],
    granted_by: &str,
) -> Result<AccessProfile, AccountError>;
```

All reuse the supplied pool and perform no current-request authorization. Permission definitions are
limited to 256 unique keys; role permissions to 256 IDs; user roles to 64 IDs. User/role relationship
writes are transactional. `granted_by` is an existing local actor ID used for audit records.

## Authorization extractors

`AuthenticatedUser` validates the token and local account, then exposes `profile()` and
`into_profile()`. `Authorized<P>` additionally requires the compile-time key from
`RequiredPermission`:

```rust
struct ReadFactories;

impl RequiredPermission for ReadFactories {
    const KEY: PermissionKey = PermissionKey::from_static("factories:read");
}

async fn list_factories(auth: Authorized<ReadFactories>) {
    let actor_id = auth.profile().user.id.as_str();
}
```

The matching permission still must be registered with `create_permissions` or
`Account::register_permissions`.

## Core data types

`ExternalIdentity` contains stable `identity_id`, optional `username`, optional `email`, required
`display_name`, and optional `avatar_url`. `identity_id` is the only stable binding key; username is
metadata. Outputs include `User`, `Permission`, `Role`, `SystemRole`, and `AccessProfile`.
`CreateHumanIdentity` contains `username`, `given_name`, `family_name`, `email`, optional
`display_name`, required `initial_password`, and `require_password_change`; the password is sent only
to the identity directory and is not stored in the local Account database.
`AccessProfile::allows` always permits the super administrator and otherwise checks the merged
`BTreeSet<PermissionKey>`.

## Setup extension points

```rust
pub fn setup_routes<S>(
    account: Account,
    directory: ZitadelUserDirectory,
    setup_secret: &str,
) -> Router<S>;

pub fn setup_routes_with<S, T>(
    account: Account,
    directory: ZitadelUserDirectory,
    setup_secret: &str,
    setup: T,
) -> Router<S>
where T: Setup;
```

`SetupUnlockRequest` maps a custom DTO to `setup_secret()`. `SetupCompletionRequest` maps to
`setup_token()` and `super_admin_identity_id()`. A `Setup` implementation supplies unlock,
selection, completion, error, and closed/404 responses.

Presentation is customizable, but framework invariants are not: constant-time secret validation, a
32-byte random token valid for 15 minutes, directory re-verification of the selected human user,
system-role synchronization, transactional unique-super-admin selection, and permanent Setup
closure after completion.

## Public re-export inventory

Hosts can import these stable boundaries directly from `nexora::server`:

- composition/configuration: `Server`, `ServerError`, `AccountSettings`, `AccountOidcSettings`,
  `AccountServerInitializationError`, `migrations`, `dependencies`, `user_directory`;
- Account facade/entities: `Account`, `AccountError`, `AccessProfile`, `ExternalIdentity`,
  `CreateHumanIdentity`, `IdentityDirectory`, `IdentityDirectoryError`, `User`, `Role`, `Permission`,
  `PermissionDefinition`, `PermissionKey`;
- auth: `AuthenticatedUser`, `Authorized<P>`, `RequiredPermission`;
- trusted writes: `create_permissions`, `create_role`, `create_user`, `create_user_with_roles`,
  `replace_role_permissions`, `replace_user_roles`;
- Setup/directory: `DefaultSetup`, both default request types, the three Setup traits,
  `setup_routes`, `setup_routes_with`, `DirectoryUser`, `DirectoryError`, and
  `ZitadelUserDirectory`.
