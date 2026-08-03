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
  --tag v0.21.1 nexora --locked --force \
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

The repository-root `nexora.toml` is the only build and publish project configuration. Each app declares its technical `package`, user-facing `display_name`, release channel/version/build number, updater trust, required targets, and macOS signing policy.

`package` controls Cargo, the cargo-bundle source path, and technical artifact names. `display_name` controls Info.plist, the DMG volume, and the app name users see after installation. The release channel must be listed in updater channels, version must be SemVer, and build number must be greater than zero.

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

## Commands

```bash
nexora build
nexora publish --dry-run
nexora publish
```

A single app is selected automatically. Multiple apps use an interactive `display_name (app key / package)` menu; non-interactive use requires `--app`, while publishing every app requires explicit `--all`. A real non-interactive publish also requires `--yes`.

Build never accesses object storage. It builds required host-compatible targets, the main binary and `<executable>-updater` sidecar, writes bundle configuration and Info.plist, signs the completed bundle, and creates both a technical `.app.zip` and DMG plus multi-artifact `artifact.json`. Publish never runs build and requires both ZIP and DMG for every required macOS target.

The DMG is the first-install medium; `.app.zip` is the self-update payload; `artifact.json` indexes
local hashes; `latest.json` is the signed remote release decision; and the sidecar independently
re-verifies, stages, replaces, restarts, confirms health, and rolls back.

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
