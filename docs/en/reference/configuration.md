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

Environment variables use `__` for nesting. An explicit path wins; otherwise Nexora finds
`config/<package>.toml`. Inject secrets through environment variables or a secret manager.

The Setup secret is only useful before initialization. Nexora records framework history in
`nexora._sqlx_migrations`, while the application owns an independent history. Both borrow the same
`PgPool`; upgrades must not depend on an `initialize_empty_database` boolean switch.

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

[publish.targets.rustfs.channels.nightly]
endpoint = "http://192.168.0.250:9000"
public_base_url = "http://192.168.0.250:9000/desktop-releases"
allow_insecure_http = true
```

`endpoint` is the signed S3 API URL; `public_base_url` is the anonymous client read URL. Local RustFS
over HTTP must explicitly enable `allow_insecure_http`. Channel tables override the base target by
field; omitted fields inherit, and the merged endpoint, public URL, and HTTP policy are revalidated.

Publish resolves every credential field independently for the current channel. It checks
`NEXORA_PUBLISH_<CHANNEL>_<FIELD>`, then `NEXORA_PUBLISH_<FIELD>`, and finally `AWS_<FIELD>`. For
example, beta may use `NEXORA_PUBLISH_BETA_ACCESS_KEY_ID` together with
`NEXORA_PUBLISH_SECRET_ACCESS_KEY`. Empty values continue fallback, access and secret are required,
and the session token is optional. `RUSTFS_*` is not read. `object_prefix = ""` places the stable app
key directly at the bucket root without empty path segments or doubled slashes.

Each desktop app declares its stable app key, bundle identifier, branding, and platform icons in the
same app registration. Both single-package and workspace layouts resolve resources from the workspace
root and do not read or mutate `[package.metadata.bundle]`:

```toml
[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "Desktop Example"
publish_target = "rustfs"
object_prefix = ""

[apps.desktop.branding]
application_logo = "assets/logos/desktop/logo-icon-128.png"
icon_source = "assets/logos/desktop/logo-icon-source.png"
managed = true

[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "ad_hoc"
notarize = false

[apps.desktop.platforms.windows]
icon = "assets/logos/desktop/logo-icon.ico"
publisher = "Example Publisher"
signing = "none"
start_menu_shortcut_default = true
launch_after_install_default = true
minimum_windows_build = 15063

[apps.desktop.platforms.linux]
icons = ["assets/logos/desktop/logo-icon-16.png", "assets/logos/desktop/logo-icon-128.png"]
```

Paths must be workspace-relative and remain inside the workspace. `targets.required` is optional;
build defaults to the host from `rustc -vV`, while `nexora build --target <triple>` overrides it.
The production packaging path covers macOS `.app`/DMG and Windows x86_64/ARM64 Simplified Chinese
Inno Setup EXE and update ZIP artifacts.

Windows `publisher` remains required installer metadata in every signing mode. `signing = "none"`
retains Ed25519 manifest verification, artifact SHA-256, ZIP safety, and PE architecture checks, but
must not include `signing_thumbprint`, `expected_publisher`, or `timestamp_url`. With
`signing = "authenticode"`, configure a certificate thumbprint (or
`WINDOWS_SIGN_CERTIFICATE_SHA1`) and an RFC 3161 `timestamp_url`. `expected_publisher` defaults to
`publisher`; the updater verifies both the main executable and sidecar certificate identities.
