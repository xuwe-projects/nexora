# Desktop updater

Nexora uses a signed `latest.json`, platform update archives, and an independent sidecar for in-app updates. First installation uses a macOS DMG or a Windows MSI/Setup EXE. Signing keys and S3/RustFS credentials remain in the developer's publish environment; end users configure no updater environment variables.

The production path supports macOS and Windows x86_64/ARM64. Windows builds create a Simplified
Chinese WiX MSI and a Burn Setup EXE while in-app updates consume only `windows.zip`. The default
Windows floor follows the pinned GPUI baseline: Windows 10 1703, build 15063. Linux release
resources follow the same metadata contract, but Linux auto-installation is not documented here.

## First-time prerequisites

```bash
xcode-select --install
rustup target add aarch64-apple-darwin
cargo install cargo-bundle
brew install create-dmg
cargo install --git https://github.com/xuwe-projects/nexora \
  --tag v0.26.0 nexora --locked --force \
  --no-default-features --features cli --bin nexora
nexora doctor
# Installs cargo-bundle/create-dmg if either is missing:
nexora doctor --fix
```

Rust, Xcode Command Line Tools, and Homebrew themselves must be installed before `doctor --fix`.
On Windows, install the Windows 10/11 SDK Desktop C++ tools and .NET SDK, then install the pinned
modern cargo-wix revision and WiX 5.0.2. `nexora doctor --fix` installs the matching UI and
BootstrapperApplications extensions, but it does not install the SDK, WiX itself, or Rust targets.
SDK tools are discovered from the standard Windows Kits directory and do not need to be added to
PATH manually.
The complete field-by-field reference, including required status, sources, defaults, secret status,
examples, and failure behavior, is maintained in the
[Chinese updater reference](/desktop/updater). The code defaults are `force_path_style = false`,
`allow_insecure_http = false`, `branding.managed = false`, `minimum_supported_version = "0.0.0"`,
`check_on_launch = false`, `check_interval = "15m"`, `check_jitter = "1m"`,
`offline_grace_period = "24h"`, `mandatory_restart_delay = "15m"`, and
`health_timeout = "2m"`. `expected_team_id` is unset, and updater `app_id` inherits the app
identifier when omitted.

## Configuration

The repository-root `nexora.toml` is the only build and publish project configuration. Each app declares its technical `package`, user-facing `display_name`, release channel/version/build number, updater trust, and platform policy. `targets.required` is optional: by default build uses the host reported by `rustc -vV`, while repeated `--target` arguments provide an explicit override. New projects use:

```toml
[apps.desktop.release]
default_channel = "stable"
version = "${CARGO_PKG_VERSION}"
build_number = "${BUILD_DATETIME}"
minimum_supported_version = "0.0.0"
notes = "docs/releases/1.2.3/en.md"

[apps.desktop.release.channels.beta]

[apps.desktop.release.channels.stable]
```

The supported release identity parameters are:

| Parameter | Supported values | Result |
| --- | --- | --- |
| `release.version` | Exact `${CARGO_PKG_VERSION}` or a literal SemVer such as `"1.2.3"` | A validated SemVer |
| `release.build_number` | Exact `${BUILD_DATETIME}` or a literal positive integer such as `42` | A positive `u64` |
| `release.notes` | Repository-relative UTF-8 Markdown path; required when updater is enabled and overridable per channel | Frozen `notes.md`, limited to 1 MiB |

`package` controls Cargo, the cargo-bundle source path, and technical artifact names. `display_name` controls Info.plist, the DMG volume, and the app name users see after installation. `${CARGO_PKG_VERSION}` reads the selected app package through `cargo metadata --no-deps --format-version 1`, including packages that use `version.workspace = true`; it is not the workspace root name or Nexora CLI version. A literal SemVer remains supported.

`${BUILD_DATETIME}` is the complete 24-hour `yyMMddHHmmss` value in the build machine's local timezone. A build in the same second or after clock rollback, daylight-saving fallback, or a timezone change uses `max(current local value, previous local build number + 1)`. A literal positive integer remains supported. Expressions must occupy the entire field; unknown expressions, fragments, arbitrary environment variables, zero, and overflow are rejected.

The signing key is read from `release.signing_key_file`, resolved relative to `nexora.toml`. Only an absent or empty file setting falls back to the environment variable named by `signing_key_env`. A configured missing file is an error and never falls back.

Create the key with `nexora updater keygen --app <key> --private-key-file <ignored-path>`. Rotate by
first shipping a client that trusts both old and new public keys, then changing the signing private
key, and only later removing the old public key. RustFS credentials authorize uploads, Ed25519 signs
the update manifest, and Apple Developer ID signs macOS code; these credentials are independent.

Applications load `UpdateConfig::from_current_bundle()` from
`.app/Contents/Resources/nexora-updater.json` on macOS or `nexora-updater.json` beside the main
EXE on Windows, then install it once with
`nexora::desktop::install_updater`. Standalone sidecars call
`nexora::desktop::run_sidecar_from_env_args`; applications must not depend directly on the internal
updater crate. Successful installation enables the shared action, default login, account menu,
Settings, and native macOS menu entries. A failed or missing installation exposes no half-working
entry and updater traffic never uses Account tokens or business permissions.

Builds that intentionally support `updater.enabled = false` use
`UpdateConfig::from_current_bundle_if_present()`. It returns `None` only when the bundled file is
absent; an existing but invalid file remains an error and cannot bypass trust or transport checks.

Every formal package receives `nexora-release.json` and optional `notes.md` before signing and
archiving. They live in `.app/Contents/Resources` on macOS and beside the main executable on
Windows. Schema 1 records the app key/ID, display name, package, version, positive build number,
channel, target, and optional notes file name, byte size, and SHA-256. It contains no secrets.

Applications can read the validated identity without retaining updater state:

```rust
let info = nexora::desktop::application_info(cx);
let build_number: Option<u64> = info.build_number();
```

During `cargo run` and tests, a missing metadata file is development mode: name/version fall back
to `ApplicationOptions`, while app ID, build number, and channel are `None`. An existing invalid
file fails startup. An installed updater must match the general metadata's app ID, version, build
number, and channel.

After installation, the Sidebar Footer item, native macOS menu, and default shortcut all dispatch
the same `CheckForUpdates` action. The shortcut is `Cmd+Shift+U` on macOS and `Ctrl+Shift+U` on
Windows and Linux.

## Commands

```bash
nexora build
nexora publish --dry-run
nexora publish
```

A single app is selected automatically. Multiple apps use an interactive `display_name (app key / package)` menu; non-interactive use requires `--app`, while publishing every app requires explicit `--all`. A real non-interactive publish also requires `--yes`. When an app declares multiple `release.channels`, an interactive terminal also shows a channel multi-select with `default_channel` preselected. Non-interactive CI should pass repeatable `--channel` arguments or `--all-channels` explicitly.

Before building any target, build atomically freezes the resolved identity in `dist/<app>/<channel>/release.json`. The receipt contains its schema, app key, package, channel, final version/build number and their sources, signed-integer Unix creation second, and planned targets. Every target in that build shares the identity. An incomplete retry reuses a matching receipt; once all target artifacts are complete, another explicit dynamic build creates a strictly higher number without deleting old versioned artifacts. A corrupt or unsupported receipt fails before target builds and is never reconstructed from directory names.

Build never accesses object storage and never installs a missing Rust target implicitly. It builds the selected host-compatible target, main binary, and `<executable>-updater` sidecar. macOS produces `.app.zip` plus DMG. Windows produces MSI, a Burn Setup EXE that reuses the Chinese MSI UI, and `windows.zip`; only the ZIP enters the updater manifest. Every release artifact receives a standard SHA-256 sidecar and is indexed by `artifact.json`.

## Windows Authenticode policy

Local development, examples, and controlled internal tests may explicitly use:

```toml
[apps.desktop.platforms.windows]
publisher = "Example Publisher"
signing = "none"
```

This mode still enforces the Ed25519 manifest signature, manifest sequence, artifact size and
SHA-256, ZIP safety, and PE architecture checks. It skips only Authenticode. The TOML configuration
must not retain `signing_thumbprint`, `expected_publisher`, or `timestamp_url` in this mode; build
fails immediately when any of those fields is present. A process-wide
`WINDOWS_SIGN_CERTIFICATE_SHA1` variable is ignored while the explicit mode is `none`.

Public production releases should use:

```toml
[apps.desktop.platforms.windows]
publisher = "Example Publisher"
signing = "authenticode"
signing_thumbprint = "00112233445566778899AABBCCDDEEFF00112233"
expected_publisher = "Example Publisher"
timestamp_url = "https://timestamp.example.com"
```

The thumbprint may instead come from `WINDOWS_SIGN_CERTIFICATE_SHA1`.
`expected_publisher` defaults to `publisher`. Build signs and verifies the main executable, updater,
MSI, and Setup EXE. In-app update staging rejects an unsigned executable, an invalid Windows trust
chain, a mismatched certificate thumbprint, or a mismatched publisher for either the main executable
or updater. A self-signed certificate is suitable only for controlled testing unless every target
device explicitly trusts its root.

Publish, dry-run, and yank read version/build number and targets only from the receipt and never recompute local time or run build. Publish validates the receipt against the app, package, channel, current Cargo package version, and configuration, then validates each receipt target's `artifact.json`, file size, and SHA-256. Dry-run performs the same local and remote checks without local or remote writes. An available release must be strictly greater than the remote `(version, build_number)`.

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

Versioned platform artifacts, their `.sha256` sidecars, and sequence manifests are immutable. Publish derives each checksum sidecar from the revalidated artifact digest, so older builds without local sidecars remain publishable. Each macOS target receives a no-cache `latest-<arch>.dmg`; each Windows target receives `latest-<arch>.exe` and `latest-<arch>.msi`. A single-target release additionally receives the corresponding `latest.dmg`, `latest.exe`, or `latest.msi`. Signed `latest.json` is uploaded last. The updater manifest still contains only in-app update ZIP payloads, never a first-install DMG, Setup EXE, or MSI.

The client reads its current `(version, build_number)` from its own bundle. Server `latest.json` represents only the latest available release. Version comparison uses `(version, build_number)`; manifest sequence is solely replay protection.

## Release notes trust and migration

`release.notes` is resolved from the repository root and may be overridden per channel. With the
updater enabled it must be a readable, non-empty UTF-8 regular file inside the repository and no
larger than 1 MiB. Build freezes it once as
`dist/<app>/<channel>/<version>/<build_number>/notes.md`; every target packages identical bytes,
and publish reads only that frozen file.

Available manifests sign `notes_url`, `notes_sha256`, and `notes_size`. Old manifests without the
integrity fields remain installable, but new clients do not render their remote URL. The in-app
dialog downloads notes only on first request and renders them only after transport, size, digest,
UTF-8, and content checks. A failed notes request never blocks an update. After a successful
sidecar health launch, the new package shows its locally verified notes once; ordinary launches and
first installation do not.

Existing updater projects must add `notes = "docs/releases/current/en.md"` to each effective
release (or channel override), move or reference the previous changelog explicitly, and rebuild the
release before publishing. The former hard-coded changelog directory is no longer an implicit
fallback. Old release receipts use an unsupported schema and must be regenerated by build rather
than edited by hand.

## Startup checks and user confirmation

After the main window is created, the application silently fetches and verifies `latest.json`. An up-to-date result or a failed startup check does not interrupt the user. When an optional update is available, a window-level confirmation dialog offers **Update Now**, **Download in Background**, and **Later**. The package is downloaded only after the user selects one of the first two choices; a background download opens a restart confirmation when it is ready. A mandatory update caused by `minimum_supported_version` cannot be dismissed or deferred.

A user-initiated update check shows progress immediately and then follows the same confirmation, download, verification, staging, and restart flow. One coordinator is retained per process so startup and manual checks cannot download the same release concurrently.

Windows in-app updates do not require administrator privileges. Nexora creates the hidden transaction
root `<install-parent>/.nexora-updater/<app_id>` beside the user-selected installation directory, so
staging, pending payloads, backups, health state, and install results remain on the same volume and
outside the directory being replaced. Before **Restart Now** exits the main process, preflight checks
the current and staged layouts, PE entry points, volume identity, and create/rename permission in the
installation parent. A failed preflight leaves the application running and reports the error.

**Restart Later** commits `pending.json` from a synced temporary file with Windows atomic replacement,
including when an older pending record exists. Once committed, best-effort directory durability cannot
turn success into an error or move the payload back to staging. If replacement or health confirmation
fails, the sidecar stops the failed new process, restores the old version, writes a bounded user-safe
failure result, and only then relaunches the old application. The next launch consumes that result via
the existing Notification component. An installation parent that is not writable is a preflight error;
the user should select a writable installation path instead of elevating the whole updater.

HTTP plus ad-hoc signing is allowed only for controlled local/LAN tests. Production must use HTTPS,
`signing = "developer_id"`, `notarize = true`, and an `expected_team_id`. Install the Developer ID
Application certificate in the build keychain, use `MACOS_SIGN_IDENTITY` only when selection is
ambiguous, and provision the notarytool keychain profile named by `NOTARY_PROFILE` (default
`nexora`).
