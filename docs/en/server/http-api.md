---
title: Complete HTTP API reference
order: 2
---

# Complete HTTP API reference

This page documents the Setup and Account routes returned by
`nexora::server::Server::routers()`, plus the host-owned `/health` example. See the
[OpenAPI 3.1 document](../../openapi.yaml) for the machine-readable contract.

## Route ownership

| Owner | Routes | Returned by `Server::routers()` |
| --- | --- | --- |
| Nexora Setup | `GET /setup`, `POST /setup`, `POST /setup/complete` | Yes |
| Nexora Account | `/me`, `/users`, `/roles`, `/permissions`, and child resources | Yes |
| Generated host | `GET /health` | No; the host's `routes::routers()` owns it |
| Host business modules | Application-defined | No |

Nexora does not add an `/api/v1` prefix, bind a listener, or own the final Axum State. The host
decides route merge order and middleware boundaries.

## Protocol conventions

- Account endpoints use `application/json` and `snake_case` fields.
- The default Setup flow is SSR HTML and accepts `application/x-www-form-urlencoded` forms.
- Timestamps are signed Unix seconds, never RFC 3339 strings or millisecond values.
- Protected endpoints require `Authorization: Bearer <access_token>`.
- Stable identity lookup uses the issuer-scoped `identity_id`. Optional `username` is mutable login
  metadata and is never an authentication key.

Authentication validates the Bearer syntax, token signature/issuer/audience/expiry, local account
registration, account status, and finally the required permission. The super administrator bypasses
permission membership; other users receive permissions through roles.

## Permission matrix

| Method and path | Required permission |
| --- | --- |
| `GET /me` | Authenticated user only |
| `GET /users`, `GET /users/{user_id}` | `users:read` |
| `POST /users` | `users:provision`; also `users:roles.write` when `role_ids` is non-empty |
| `PATCH /users/{user_id}` | `users:status.write` |
| `PUT /users/{user_id}/roles` | `users:roles.write` |
| `GET /roles`, `GET /roles/{role_id}` | `roles:read` |
| Role create/update/delete and permission replacement | `roles:write` |
| `GET /permissions` | `permissions:read` |

## Shared representations

### User

| Field | Type | Nullable | Meaning |
| --- | --- | --- | --- |
| `id` | string | No | Locally generated 8-character alphanumeric ID |
| `identity_id` | string | No | Stable subject in the configured OIDC issuer |
| `username` | string | Yes | Identity Provider login name; not a binding key |
| `email` | string | Yes | Display email |
| `display_name` | string | No | Display name, at most 200 characters |
| `avatar_url` | string | Yes | Avatar URL |
| `status` | enum | No | `active` or `suspended` |
| `is_super_admin` | boolean | No | Whether this is the unique immutable super administrator |
| `created_at` | int64 | No | Creation time in Unix seconds |
| `updated_at` | int64 | No | Profile update time in Unix seconds |
| `last_login_at` | int64 | No | Last successful authentication time in Unix seconds |

```json
{
  "id": "A1b2C3d4",
  "identity_id": "279693210507280451",
  "username": "lin.chen",
  "email": "lin@example.com",
  "display_name": "Lin Chen",
  "avatar_url": null,
  "status": "active",
  "is_super_admin": false,
  "created_at": 1784304000,
  "updated_at": 1784304000,
  "last_login_at": 1784304000
}
```

### Permission and Role

`Permission` contains `id: int64`, stable `key`, `name`, and nullable `description`. `Role` contains
`id`, stable `key`, `name`, nullable `description`, `is_system`, direct `permissions`, and
`created_at`/`updated_at` Unix seconds. System roles cannot be changed or deleted.

### AccessProfile

```json
{
  "user": { "id": "A1b2C3d4", "identity_id": "279693210507280451", "username": "lin.chen", "email": null, "display_name": "Lin Chen", "avatar_url": null, "status": "active", "is_super_admin": false, "created_at": 1784304000, "updated_at": 1784304000, "last_login_at": 1784304000 },
  "roles": [],
  "permissions": ["users:read"]
}
```

`roles` are direct assignments. `permissions` are the deduplicated merged permission keys. A super
administrator may have neither roles nor permission records and still passes authorization.

### Collections and pagination

Users use page-number pagination:

```json
{ "items": [], "page": { "number": 1, "size": 25, "total": 0 } }
```

`page` defaults to 1 and must be positive. `page_size` defaults to 25 and is clamped to `1..=100`.
Roles and permissions use the unpaged `{"items": [...]}` envelope.

## Shared error contract

```json
{
  "error": {
    "code": "validation_failed",
    "message": "Role name must contain 1 to 100 characters",
    "details": { "field": "name" },
    "request_id": "req_01JZ7V4M1K8F9Q2T6Y3B5C7D8E"
  }
}
```

| HTTP | Stable codes |
| --- | --- |
| 400 | `invalid_json_body`, `invalid_path_parameter`, `invalid_query_parameter` |
| 401 | `missing_access_token`, `invalid_access_token`, `invalid_identity`, `invalid_identity_issuer` |
| 403 | `account_not_registered`, `account_suspended`, `permission_denied` |
| 404 | `resource_not_found`, `route_not_found` |
| 405 | `method_not_allowed` |
| 409 | `user_already_provisioned`, `role_key_exists`, `role_in_use`, `system_role_immutable`, `role_not_modified`, `last_administrator`, `super_administrator_immutable` |
| 422 | `validation_failed`; `details.field` identifies the field |
| 500 | `internal_error`; SQL, paths, and stack traces are never exposed |
| 503 | `identity_issuer_not_bound`, `identity_provider_unavailable` |

`401` includes `WWW-Authenticate: Bearer`. When the host installs Nexora's common HTTP middleware,
responses also include `x-request-id`, matching the error body's `request_id`.

## Current user

### `GET /me`

Requires authentication but no extra permission. It reads the latest human profile from ZITADEL
UserService v2 by identity ID, synchronizes username, email, display name, avatar, and last-login
time, then returns `200 AccessProfile`. It never provisions an unknown identity. Common failures are
invalid token `401`, unregistered or suspended account `403`, missing directory identity `404`, and
Provider `503`.

## Users

### `GET /users`

Requires `users:read`. Accepts `page` and `page_size`; unknown query keys are rejected. Returns
`200` with the user page envelope.

### `POST /users`

Requires `users:provision` and, for a non-empty `role_ids`, `users:roles.write`. The server creates a
human user with an initial password through ZITADEL gRPC and binds the returned identity ID locally.
`initial_password` is never stored in the local database, logs, or error details.

| Body field | Required | Constraints |
| --- | --- | --- |
| `username` | Yes | Trimmed 1–200 characters; unique in the configured Organization |
| `given_name` | Yes | Trimmed 1–200 characters |
| `family_name` | Yes | Trimmed 1–200 characters |
| `email` | Yes | Valid email without whitespace, up to 200 characters |
| `display_name` | No | Nullable, up to 200 characters; names are used when omitted |
| `initial_password` | Yes | 1–200 characters; written only to ZITADEL |
| `require_password_change` | No | Defaults to `false`; whether the user must change the password after first login |
| `role_ids` | No | Defaults to `[]`, at most 64 IDs; deduplicated by the server |

```json
{
  "username": "lin.chen",
  "given_name": "Lin",
  "family_name": "Chen",
  "email": "lin@example.com",
  "display_name": "Lin Chen",
  "initial_password": "imes13800000000.",
  "require_password_change": false,
  "role_ids": [3, 5]
}
```

Returns `201 Created`, `Location: /users/{id}`, and the created `User`. Local user creation, the
built-in `member` assignment, and requested roles are transactional. A local failure triggers a
best-effort deletion of the newly created Provider user. Duplicate username/email returns
`409 identity_already_exists`; invalid fields return `422`; missing roles return `404`; an unavailable
Provider returns `503 identity_provider_unavailable`.

### `GET /users/{user_id}`

Requires `users:read`. `user_id` is the 8-character local ID. Returns `200 AccessProfile` or
`404 resource_not_found`.

### `PATCH /users/{user_id}`

Requires `users:status.write` and accepts only:

```json
{ "status": "suspended" }
```

Returns `200 User`. The super administrator cannot be changed, and the last enabled administrator
cannot be suspended (`409 super_administrator_immutable` or `409 last_administrator`).

### `PUT /users/{user_id}/roles`

Requires `users:roles.write`:

```json
{ "role_ids": [3, 5] }
```

This replaces the complete business-role set rather than appending. The server deduplicates up to
64 IDs and retains the built-in `member` role. Returns `200 AccessProfile`. The super administrator
cannot receive roles, and the operation cannot remove the final administrator.

## Roles

### `GET /roles`

Requires `roles:read`. Returns `200 {"items": Role[]}`.

### `POST /roles`

Requires `roles:write`.

| Body field | Required | Constraints |
| --- | --- | --- |
| `key` | Yes | 2–64 characters; lowercase letter first, then lowercase letters, digits, `.`, `_`, `-` |
| `name` | Yes | Trimmed 1–100 characters |
| `description` | No | May be omitted or null, at most 1000 characters |
| `permission_ids` | No | Defaults to `[]`, at most 256 IDs |

Returns `201`, `Location: /roles/{id}`, and `Role`. Duplicate key returns `409 role_key_exists`;
missing permission returns `404`; invalid fields return `422`.

### `GET /roles/{role_id}`

Requires `roles:read`. `role_id` is a positive int64. Returns `200 Role`, `404`, or
`400 invalid_path_parameter`.

### `PATCH /roles/{role_id}`

Requires `roles:write`. At least one field must be supplied:

```json
{ "name": "Senior support", "description": null }
```

Missing `description` keeps the value; explicit `null` clears it. The stable `key` cannot be changed.
Returns `200 Role`; an empty body returns `422`; a system role returns
`409 system_role_immutable`.

### `DELETE /roles/{role_id}`

Requires `roles:write`. Returns `204` with no body. A system role cannot be deleted and a role still
assigned to users returns `409 role_in_use`.

### `PUT /roles/{role_id}/permissions`

Requires `roles:write`:

```json
{ "permission_ids": [1, 2, 7] }
```

Replaces the complete direct permission set. Up to 256 IDs are deduplicated; an empty array clears
all direct permissions. Returns `200 Role`.

## Permissions

### `GET /permissions`

Requires `permissions:read`. Returns the complete unpaged catalog as
`200 {"items": Permission[]}`. HTTP is read-only; trusted hosts register permissions through
`create_permissions` or `Account::register_permissions`. Registration also grants each new
permission to the system-administrator role in the same transaction.

## Default Setup SSR flow

Setup exists only until initialization completes. Afterwards all three operations return `404`.
Default responses include `Cache-Control: no-store`, a restrictive CSP, and
`X-Content-Type-Options: nosniff`.

### `GET /setup`

No input. Returns the secret form as `200 text/html`, `404` after initialization, or `500` when the
initialization state cannot be read.

### `POST /setup`

Accepts form field `secret`. A wrong secret returns `401`. Success lists enabled human ZITADEL users,
creates a 32-byte random one-time token valid for 15 minutes, and returns the selection page. No
eligible user returns `503`; directory/database errors return `500`.

### `POST /setup/complete`

Accepts form fields `setup_token` and `identity_id`. The framework re-reads the selected user from
the directory. Invalid/expired token returns `401`; a missing, disabled, or non-human user returns
`422`; success synchronizes system roles, transactionally selects the unique super administrator,
clears the session, and returns `200 text/html`.

A custom `Setup` may replace DTOs and presentation, but cannot bypass secret validation, the timed
token, directory verification, role synchronization, or the super-administrator transaction.

## Host health endpoint

`GET /health` is not returned by `Server::routers()`. The repository's complete example returns
`{"status":"ok"}` after a successful PostgreSQL `SELECT 1`, or `503 database_unavailable`. The
minimal CLI-generated host only guarantees a `200`/`503` status and no JSON body. The OpenAPI entry
describes the repository's complete example contract.
