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
allow_insecure_http = false
```

HTTPS endpoints are always accepted, and `localhost` plus IPv4/IPv6 loopback HTTP remain available
for local development. Non-loopback HTTP requires `allow_insecure_http = true`. Plain HTTP transmits
Bearer tokens in clear text, so enable it only for trusted intranet or otherwise controlled
environments where that risk is accepted.

Production deployments usually use HTTPS:

```toml
[api]
endpoint = "https://api.example.com"
allow_insecure_http = false
```

Controlled intranet HTTP must be explicitly allowed after accepting the risk:

```toml
[api]
endpoint = "http://10.0.0.20:3000"
allow_insecure_http = true
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

Environment variables use `__` for nesting. `nexora::config::initialize(None)` resolves an explicit
`initialize(Some(path))` path first, then a valid command-line TOML path, the standard macOS bundle
resource `Contents/Resources/config/<package>.toml`, and finally local `config/<package>.toml`.
Updater health arguments are not treated as config paths, and bundle detection does not scan
arbitrary ancestors. Inject secrets through environment variables or a secret manager.

For channel builds, runtime config resolution is: explicit channel `runtime_config`,
`config/<package>-<channel>.toml`, then `config/<package>.toml`. A missing explicit file never falls
back. Paths must be safe workspace-relative files whose canonical locations remain inside the
workspace; absolute paths, `..`, Windows prefixes, and symlink escapes are rejected. The selected
file is copied into the stable bundle resource path before signing. Runtime config ships publicly
inside the app bundle and must not contain database passwords, PATs, private keys, or other secrets.

The Setup secret is only useful before initialization. `_sqlx_migrations` records applied versions,
so upgrades must not depend on an `initialize_empty_database` boolean switch.

## Updater Publish Configuration

Desktop updater build and publish configuration lives in the repository-root `nexora.toml`; the full
file is not bundled into clients. Publish targets support S3-compatible object storage:

```toml
[publish.targets.rustfs]
provider = "s3"
endpoint = "http://127.0.0.1:9000"
bucket = "desktop-releases"
region = "us-east-1"
force_path_style = true
public_base_url = "http://127.0.0.1:9000/desktop-releases"
allow_insecure_http = true
```

`endpoint` is the signed S3 API URL; `public_base_url` is the anonymous client read URL. Local RustFS
over HTTP must explicitly enable `allow_insecure_http`. Publish credentials come from
`AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` or `RUSTFS_ACCESS_KEY_ID` /
`RUSTFS_SECRET_ACCESS_KEY` and must not be written to configuration files. Each E2E run should use a
unique `object_prefix`.

Each desktop app declares its stable app key, bundle identifier, branding, and platform icons in the
same app registration. Both single-package and workspace layouts resolve resources from the workspace
root and do not read or mutate `[package.metadata.bundle]`:

```toml
[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "Desktop Example"
publish_target = "rustfs"
object_prefix = "products"

[apps.desktop.branding]
application_logo = "assets/logos/desktop/logo-icon-128.png"
icon_source = "assets/logos/desktop/logo-icon-source.png"
managed = true

[apps.desktop.release]
default_channel = "nightly"
version = "${CARGO_PKG_VERSION}"
build_number = "${BUILD_DATETIME}"
minimum_supported_version = "0.1.0"

[apps.desktop.release.channels.nightly]
runtime_config = "config/desktop-nightly.toml"

[apps.desktop.release.channels.beta]
runtime_config = "config/desktop-beta.toml"

[apps.desktop.release.channels.stable]
runtime_config = "config/desktop-stable.toml"

[apps.desktop.updater]
enabled = true
channels = ["nightly", "beta", "stable"]

[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "ad_hoc"
notarize = false

[apps.desktop.platforms.windows]
icon = "assets/logos/desktop/logo-icon.ico"

[apps.desktop.platforms.linux]
icons = ["assets/logos/desktop/logo-icon-16.png", "assets/logos/desktop/logo-icon-128.png"]
```

Paths must be workspace-relative and remain inside the workspace. The production packaging path in
this phase covers macOS `.app`, DMG, and ICNS. Windows and Linux icons participate in configuration,
scaffolding, and generation, but full installers and automatic replacement are not implemented yet.

Root `release` fields are defaults for every channel; a channel can override version, build number,
minimum supported version, and runtime config. `default_channel` must exist, and every release
channel must belong to enabled updater channels. New `release.channels` configuration cannot be
combined with legacy `release.channel` or a static `updater.feed_url`; legacy single-channel config
remains compatible. Multi-channel updater feeds are derived from the publish target, public base
URL, object prefix, app key, and channel.
