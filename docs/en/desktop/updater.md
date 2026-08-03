# Desktop updater

Nexora uses a signed `latest.json`, anonymous `.app.zip` downloads, and an independent sidecar for in-app updates. First installation uses a DMG. Signing keys and S3/RustFS credentials remain in the developer's publish environment; end users configure no updater environment variables.

The current production installer and self-update path is macOS-only. Windows and Linux icon fields
belong to the unified app registration, but Nexora does not yet package or auto-install those
platforms.

## First-time prerequisites

```bash
xcode-select --install
rustup target add aarch64-apple-darwin
cargo install cargo-bundle
brew install create-dmg
cargo install --git https://github.com/xuwe-projects/nexora \
  --tag v0.24.0 nexora --locked --force \
  --no-default-features --features cli --bin nexora
nexora doctor
# Installs cargo-bundle/create-dmg if either is missing:
nexora doctor --fix
```

Rust, Xcode Command Line Tools, and Homebrew themselves must be installed before `doctor --fix`.
The complete field-by-field reference, including required status, sources, defaults, secret status,
examples, and failure behavior, is maintained in the
[Chinese updater reference](/desktop/updater). The code defaults are `force_path_style = false`,
`allow_insecure_http = false`, `branding.managed = false`, `minimum_supported_version = "0.0.0"`,
`check_on_launch = false`, `check_interval = "15m"`, `check_jitter = "1m"`,
`offline_grace_period = "24h"`, `mandatory_restart_delay = "15m"`, and
`health_timeout = "2m"`. `expected_team_id` is unset, and updater `app_id` inherits the app
identifier when omitted.

## Configuration

The repository-root `nexora.toml` is the only build and publish project configuration. Each app declares its technical `package`, user-facing `display_name`, release channel/version/build number, updater trust, required targets, and macOS signing policy. New projects use:

```toml
[apps.desktop.release]
channel = "stable"
version = "${CARGO_PKG_VERSION}"
build_number = "${BUILD_DATETIME}"
minimum_supported_version = "0.0.0"
```

The supported release identity parameters are:

| Parameter | Supported values | Result |
| --- | --- | --- |
| `release.version` | Exact `${CARGO_PKG_VERSION}` or a literal SemVer such as `"1.2.3"` | A validated SemVer |
| `release.build_number` | Exact `${BUILD_DATETIME}` or a literal positive integer such as `42` | A positive `u64` |

`package` controls Cargo, the cargo-bundle source path, and technical artifact names. `display_name` controls Info.plist, the DMG volume, and the app name users see after installation. `${CARGO_PKG_VERSION}` reads the selected app package through `cargo metadata --no-deps --format-version 1`, including packages that use `version.workspace = true`; it is not the workspace root name or Nexora CLI version. A literal SemVer remains supported.

`${BUILD_DATETIME}` is the complete UTC `yyMMddHHmmss` value. A build in the same second or after clock rollback uses `max(current UTC value, previous local build number + 1)`. A literal positive integer remains supported. Expressions must occupy the entire field; unknown expressions, fragments, arbitrary environment variables, zero, and overflow are rejected.

The signing key is read from `release.signing_key_file`, resolved relative to `nexora.toml`. Only an absent or empty file setting falls back to the environment variable named by `signing_key_env`. A configured missing file is an error and never falls back.

Create the key with `nexora updater keygen --app <key> --private-key-file <ignored-path>`. Rotate by
first shipping a client that trusts both old and new public keys, then changing the signing private
key, and only later removing the old public key. RustFS credentials authorize uploads, Ed25519 signs
the update manifest, and Apple Developer ID signs macOS code; these credentials are independent.

Applications install `UpdateConfig::from_current_bundle()` once with
`nexora::desktop::install_updater`. Standalone sidecars call
`nexora::desktop::run_sidecar_from_env_args`; applications must not depend directly on the internal
updater crate. Successful installation enables the shared action, default login, account menu,
Settings, and native macOS menu entries. A failed or missing installation exposes no half-working
entry and updater traffic never uses Account tokens or business permissions.

After installation, the Sidebar Footer item, native macOS menu, and default shortcut all dispatch
the same `CheckForUpdates` action. The shortcut is `Cmd+Shift+U` on macOS and `Ctrl+Shift+U` on
Windows and Linux.

## Commands

```bash
nexora build
nexora publish --dry-run
nexora publish
```

A single app is selected automatically. Multiple apps use an interactive `display_name (app key / package)` menu; non-interactive use requires `--app`, while publishing every app requires explicit `--all`. A real non-interactive publish also requires `--yes`.

Before building any target, build atomically freezes the resolved identity in `dist/<app>/<channel>/release.json`. The receipt contains its schema, app key, package, channel, final version/build number and their sources, signed-integer Unix creation second, and planned targets. Every target in that build shares the identity. An incomplete retry reuses a matching receipt; once all target artifacts are complete, another explicit dynamic build creates a strictly higher number without deleting old versioned artifacts. A corrupt or unsupported receipt fails before target builds and is never reconstructed from directory names.

Build never accesses object storage. It builds required host-compatible targets, the main binary and `<executable>-updater` sidecar, writes version, build number, the configured ICNS, updater configuration, and sidecar before signing the completed bundle. The same signed `.app` produces both the technical `.app.zip` and DMG plus multi-artifact `artifact.json`. The ICNS is used only by the `.app` through `CFBundleIconFile`; the DMG file and mounted volume keep their system-default appearance, with no separate DMG icon setting.

Publish, dry-run, and yank read version/build number only from the receipt and never recompute UTC time or run build. Publish validates the receipt against the app, package, channel, current Cargo package version, configuration, and required targets, then validates each `artifact.json`, file size, and SHA-256. Dry-run performs the same local and remote checks without local or remote writes. An available release must be strictly greater than the remote `(version, build_number)`.

The DMG is the first-install medium; `.app.zip` is the self-update payload; `release.json` freezes
the local build identity; `artifact.json` indexes
local hashes; `latest.json` is the signed remote release decision; and the sidecar independently
re-verifies, stages, replaces, restarts, confirms health, and rolls back.

When an application uses `nexora::config::initialize(None)`, Nexora ignores the internal
`--nexora-updater-health-*` arguments injected by the sidecar and continues with the default TOML.
Explicit configuration paths and ordinary first positional arguments retain their precedence. This
lets the replacement process initialize configuration, create its first window, and report health
instead of treating a health-session flag as a configuration filename.

## Sequence and remote objects

Developers do not maintain `manifest_sequence`. A missing remote `latest.json` (HTTP 404 only) yields sequence 1; otherwise publish verifies the signed remote manifest and increments its sequence. Dry-run performs the same read without writes. Before mutable uploads, publish reads the remote sequence again and rejects concurrent changes.

Versioned ZIPs, DMGs, and sequence manifests are immutable. Each target also receives a no-cache `latest-<arch>.dmg`; a single-target release additionally receives `latest.dmg`. Signed `latest.json` is uploaded last. The updater manifest contains only `.app.zip` payloads and never a DMG.

The client reads its current `(version, build_number)` from its own bundle. Server `latest.json` represents only the latest available release. Version comparison uses `(version, build_number)`; manifest sequence is solely replay protection.

## Startup checks and user confirmation

After the main window is created, the application silently fetches and verifies `latest.json`. An up-to-date result or a failed startup check does not interrupt the user. When an optional update is available, a window-level confirmation dialog offers **Update Now**, **Download in Background**, and **Later**. The package is downloaded only after the user selects one of the first two choices; a background download opens a restart confirmation when it is ready. A mandatory update caused by `minimum_supported_version` cannot be dismissed or deferred.

A user-initiated update check shows progress immediately and then follows the same confirmation, download, verification, staging, and restart flow. One coordinator is retained per process so startup and manual checks cannot download the same release concurrently.

HTTP plus ad-hoc signing is allowed only for controlled local/LAN tests. Production must use HTTPS,
`signing = "developer_id"`, `notarize = true`, and an `expected_team_id`. Install the Developer ID
Application certificate in the build keychain, use `MACOS_SIGN_IDENTITY` only when selection is
ambiguous, and provision the notarytool keychain profile named by `NOTARY_PROFILE` (default
`nexora`).
