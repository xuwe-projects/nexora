# Desktop updater

Nexora uses a signed `latest.json`, platform update archives, and an independent sidecar for in-app updates. First installation uses a macOS DMG or a Windows Inno Setup EXE. Signing keys and S3/RustFS credentials remain in the developer's publish environment; end users configure no updater environment variables.

The production path supports macOS and Windows x86_64/ARM64. Windows builds create a Simplified
Chinese Inno Setup EXE while in-app updates consume only `windows.zip`. The default
Windows floor follows the pinned GPUI baseline: Windows 10 1703, build 15063. Linux release
resources follow the same metadata contract, but Linux auto-installation is not documented here.

## Manually installed build prerequisites

```bash
cargo install --git https://github.com/xuwe-projects/nexora \
  --tag v0.41.1 cli --locked --force --bin nexora
nexora doctor
```

`nexora doctor` and `nexora build` only detect prerequisites. They never download software, run an
installation command below, change PATH, open a download page, or launch a system installer.
`nexora doctor --fix` has been removed: replace it with `nexora doctor`, copy the reported command,
install the tool yourself, reopen the terminal unless noted otherwise, run the verification
command, then rerun `nexora doctor` or the original `nexora build ...` command.

### Windows

| Tool | Requirement and purpose | Supported capability / detection | Official source | Manual installation | Secrets |
| --- | --- | --- | --- | --- | --- |
| Rustup, rustc, Cargo | Required to compile the app and sidecar | `rustup --version`, `rustc --version`, `cargo --version` | [rustup.rs](https://rustup.rs/) | Download official `rustup-init.exe`, then run `rustup-init.exe -y` | No |
| Rust target | Required for the selected target: `x86_64-pc-windows-msvc` or `aarch64-pc-windows-msvc` | `rustup target list --installed` | [Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html) | `rustup target add <target>` | No |
| Visual Studio Build Tools / `link.exe` | Required MSVC linker | `where.exe link.exe`; install the Desktop development with C++ workload | [Visual Studio Downloads](https://visualstudio.microsoft.com/downloads/) | `winget install --exact --id Microsoft.VisualStudio.2022.BuildTools --source winget --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` | No; installer may require elevation |
| Windows SDK, `rc.exe`, `fxc.exe` | Required for version resources and GPUI shaders | `where.exe rc.exe`, `where.exe fxc.exe`; Windows 10/11 SDK | [Windows SDK](https://developer.microsoft.com/windows/downloads/windows-sdk/) | `winget install --source winget --exact --id Microsoft.WindowsSDK.10.0.26100 --accept-package-agreements --accept-source-agreements` | No; installer may require elevation |
| `signtool.exe` | Required only for `signing = "authenticode"` | `where.exe signtool.exe`; supplied by the Windows SDK | [SignTool](https://learn.microsoft.com/windows/win32/seccrypto/signtool) | Same Windows SDK command | No |
| Inno Setup / `ISCC.exe` | Required for the first-install Setup EXE | `>= 6.7.3, < 8.0.0`; parse `Compiler engine version` from `ISCC.exe -`; 7.x recommended for new installs | [Inno Setup Downloads](https://jrsoftware.org/isdl.php) | `winget install --source winget --exact --id JRSoftware.InnoSetup.7 --version 7.0.2 --scope user --silent --force --accept-package-agreements --accept-source-agreements` | No |
| Authenticode certificate | Required only for production Authenticode validation | `Get-ChildItem Cert:\CurrentUser\My`; `signtool verify /pa <file>`; thumbprint, publisher, and RFC 3161 URL must match | [SignTool](https://learn.microsoft.com/windows/win32/seccrypto/signtool) | Obtain from a trusted CA and import into the current-user certificate store | Yes: certificate private key and PFX password |
| Ed25519 update key | Required to publish; independent of Authenticode | `nexora updater keygen --app <id> --private-key-file <ignored-path>` | This page's signing section | Generate with that command and store the private key in an ignored file or secret manager | Yes: private key |
| S3-compatible test bucket | Required for a real two-version update acceptance run | Anonymous `GET` of channel `latest.json`; publish separately validates the S3 API | Chosen S3/RustFS provider | Administrator creates the bucket, anonymous-read policy, and publisher credentials | Yes: AK/SK/session token are publish-only |

Discovery covers PATH plus user-level and system-level Inno Setup 7 and 6 directories. Nexora runs
every candidate to obtain its actual engine version, ignores broken or unparsable candidates, and
selects the highest compatible version; a stable discovery order breaks equal-version ties.
Directory names never determine the final version. Windows x64 and Windows on ARM use their matching
host targets; Nexora does not perform cross-OS pseudo-packaging. `signing = "none"` skips only
Authenticode: Ed25519, size/SHA-256, ZIP safety, and PE architecture checks remain mandatory.

### macOS

| Tool | Requirement and purpose | Supported capability / detection | Official source | Manual installation | Secrets |
| --- | --- | --- | --- | --- | --- |
| Rustup, rustc, Cargo | Required to compile the app and sidecar | `rustup --version`, `rustc --version`, `cargo --version` | [rustup.rs](https://rustup.rs/) | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh -s -- -y` | No |
| Rust target | Required: `x86_64-apple-darwin` or `aarch64-apple-darwin` | `rustup target list --installed` | [Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html) | `rustup target add <target>` | No |
| Xcode / Command Line Tools | Required for `ditto`, `plutil`, and the developer directory | `xcode-select -p`, `xcodebuild -version` | [Xcode Resources](https://developer.apple.com/xcode/resources/) | `xcode-select --install`; notarization requires full Xcode | No; system installer may prompt |
| `codesign` | Required when `signing != "none"` | `codesign --version` | [Code Signing](https://developer.apple.com/support/code-signing/) | Installed with Xcode/CLT | Developer ID mode uses a certificate private key |
| `notarytool`, `stapler` | Required when `notarize = true` | `xcrun --find notarytool`, `xcrun --find stapler` | [Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) | Install full Xcode and select its Developer directory with `sudo xcode-select -s <path>` | Apple/App Store Connect credentials are secret |
| `cargo-bundle` | Required to create `.app` | `cargo-bundle --version` | [cargo-bundle](https://crates.io/crates/cargo-bundle) | `cargo install cargo-bundle` | No |
| Homebrew | Optional manual installer for tools such as create-dmg | `brew --version` | [brew.sh](https://brew.sh/) | `/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"` | No |
| `create-dmg` | Required to create the DMG | `create-dmg --version` | [create-dmg](https://github.com/create-dmg/create-dmg) | `brew install create-dmg` | No |
| Developer ID Application certificate | Required for production distribution; not required for local `ad_hoc` validation | `security find-identity -v -p codesigning`, `codesign -dv --verbose=4 <app>` | [Apple certificates](https://developer.apple.com/help/account/certificates/) | Create in Apple Developer and import into Keychain | Yes: certificate private key |
| Apple notarization credentials | Required when `notarize = true` | `xcrun notarytool history --keychain-profile <profile>` | [Notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow) | `xcrun notarytool store-credentials <profile>` | Yes: password/API key/issuer |
| Ed25519 key and S3 test bucket | Required for real publish/update acceptance; independent of Apple signing | Same checks as Windows | Security and publish sections below | `nexora updater keygen ...`; administrator provisions object storage | Yes |

`ad_hoc` is only local code signing; it is not Developer ID signing or notarization. Public Internet
distribution requires Developer ID, notarytool, and stapler. Intel and Apple Silicon builds use the
matching host/target. Every installation action remains explicit user or workflow code; Nexora CLI
never executes it.

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

`package` controls Cargo, the internal main executable, and updater sidecar names. `display_name`
controls user-visible metadata and distribution filenames such as `iMES-aarch64.dmg` and
`iMES-x86_64.windows.zip`. Version, build number, package, and full target triple do not enter the
distribution filename. `${CARGO_PKG_VERSION}` reads the selected app package through `cargo
metadata --no-deps --format-version 1`, including packages that use `version.workspace = true`; it
is not the workspace root name or Nexora CLI version. A literal SemVer remains supported.

`${BUILD_DATETIME}` is the complete 24-hour `yyMMddHHmmss` value in the build machine's local timezone. A build in the same second or after clock rollback, daylight-saving fallback, or a timezone change uses `max(current local value, previous local build number + 1)`. A literal positive integer remains supported. Expressions must occupy the entire field; unknown expressions, fragments, arbitrary environment variables, zero, and overflow are rejected.

The signing key is read from `release.signing_key_file`, resolved relative to `nexora.toml`. Only an absent or empty file setting falls back to the environment variable named by `signing_key_env`. A configured missing file is an error and never falls back.

Create the key with `nexora updater keygen --app <key> --private-key-file <ignored-path>`. Rotate by
first shipping a client that trusts both old and new public keys, then changing the signing private
key, and only later removing the old public key. S3 credentials authorize uploads, Ed25519 signs
the update manifest, and Apple Developer ID signs macOS code; these credentials are independent.

Publish resolves every field independently through `NEXORA_PUBLISH_<CHANNEL>_<FIELD>`,
`NEXORA_PUBLISH_<FIELD>`, and `AWS_<FIELD>`. Empty values continue fallback, so a beta access key can
be combined with the base Nexora secret key. Access and secret are required; the session token is
optional. `RUSTFS_*` is no longer read. Channel publish-target overrides inherit omitted fields from
the base target and the merged provider, URLs, and HTTP policy are validated afterward.
`provider = "s3"` protects immutable objects with `If-None-Match: *`; Alibaba Cloud OSS must use
`provider = "aliyun_oss"`, which signs and sends `x-oss-forbid-overwrite: true` instead. Branded
channel-root objects and `latest.json` remain replaceable. Provider selection is explicit and is not
inferred from the endpoint hostname. `object_prefix = ""` means that the stable app key starts
directly at the bucket root.

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

The same build freezes the selected nightly/beta/stable channel's `runtime_config` as
`config/<package>.toml`: `.app/Contents/Resources/config/<package>.toml` on macOS and
`config/<package>.toml` beside the main EXE on Windows. The first-install artifact and self-update
payload carry the same frozen file.

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

Build never accesses object storage and never installs a missing Rust target implicitly. It builds
the selected host-compatible target, main binary, and `<executable>-updater` sidecar. macOS produces
branded `.app.zip` and DMG files. Windows produces a branded Inno Setup `.exe` and `windows.zip`; only the
ZIP enters the updater manifest. Every release artifact receives a standard SHA-256 sidecar using
the complete branded filename and is indexed by `artifact.json`.

A fresh Windows install defaults to
`%LOCALAPPDATA%\Programs\<publisher>\<display_name>`. Both user-visible directory components must
be safe Windows path names. The stable `app_id` continues to identify installation upgrades,
updater transactions, manifests, and feeds; it does not become a directory component.

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
and Setup EXE. In-app update staging rejects an unsigned executable, an invalid Windows trust
chain, a mismatched certificate thumbprint, or a mismatched publisher for either the main executable
or updater. A self-signed certificate is suitable only for controlled testing unless every target
device explicitly trusts its root.

Windows builds compile separate UTF-8/Unicode PE resources for the main process and updater. The
main `FileDescription` is the display name; the updater description appends “更新程序”. Their
`InternalName` and `OriginalFilename` retain the technical package identities. Authenticode signing
runs only after each final resource has been linked.

The native main-window title prefers an application-supplied title, then installed release
`display_name`, then development `ApplicationOptions::application_name`. Shell login pages use one
official `gpui-component::TitleBar`; applications do not implement their own window controls.

Publish, dry-run, and yank read version/build number and targets only from the receipt and never recompute local time or run build. Publish validates the receipt against the app, package, channel, current Cargo package version, and configuration, then validates each receipt target's `artifact.json`, file size, and SHA-256. Dry-run performs the same local and remote checks without local or remote writes. An available release must be strictly greater than the remote `(version, build_number)`.

The DMG is the first-install medium; `.app.zip` is the self-update payload; `release.json` freezes
the local build identity; `artifact.json` indexes
local hashes; `latest.json` is the signed remote release decision; and the sidecar independently
re-verifies, stages, replaces, restarts, confirms health, and rolls back.

When an application uses `nexora::config::initialize(None)`, Nexora selects an explicit path, the
first ordinary positional argument, the frozen formal-bundle configuration, and development
workspace configuration in that order. Formal packages are identified only from the current
executable and a validated `nexora-release.json`; cwd, `CARGO_MANIFEST_DIR`, and fixed installation
paths never locate production configuration. A missing, unreadable, or invalid frozen TOML file
fails startup without falling back to a source checkout. The sidecar's
`--nexora-updater-health-session` and `--nexora-updater-health-file` pairs are ignored, so a health
restart still reads the bundle configuration. Workspace lookup remains only for `cargo run` and
`cargo test` processes without release metadata, and environment overrides are unchanged.

## Sequence and remote objects

Developers do not maintain `manifest_sequence`. A missing remote `latest.json` (HTTP 404 only) yields sequence 1; otherwise publish verifies the signed remote manifest and increments its sequence. Dry-run performs the same read without writes. Before mutable uploads, publish reads the remote sequence again and rejects concurrent changes.

Versioned platform artifacts, their `.sha256` sidecars, and sequence manifests are immutable.
Publish uploads and anonymously verifies immutable objects first, updates branded channel-root
objects next, then writes the sequence manifest and signed `latest.json` last. Updater URLs always
point at versioned immutable payloads. The layout is:

```text
[<object_prefix>/]<app_key>/<channel>/latest.json
[<object_prefix>/]<app_key>/<channel>/manifests/<sequence>.json
[<object_prefix>/]<app_key>/<channel>/<display_name>-<arch><suffix>
[<object_prefix>/]<app_key>/<channel>/<version>/<build>/<arch>/<display_name>-<arch><suffix>
```

Architecture directories are `x86_64` or `aarch64`, never full Rust triples, and there is no
`releases` path segment. Signed `latest.json` remains; installer/update aliases such as
`latest.dmg`, `latest-<arch>.dmg`, `latest.exe`, and `latest.zip` are no longer
generated. Nexora never deletes old remote aliases or immutable objects. Administrators may remove
legacy channel-root aliases manually, but old immutable versioned objects must remain available.

The client reads its current `(version, build_number)` from its own bundle. Server `latest.json` represents only the latest available release. Version comparison uses `(version, build_number)`; manifest sequence is solely replay protection.

## Release notes trust and migration

The sole authoring source for in-app release-note rules is
`.agents/skills/publish-nexora-release/SKILL.md`; the CLI scaffold ships a byte-identical packaged
copy. The Skill uses the selected app's resolved Cargo package version and the release-preparation
date to write user-visible outcomes, separately from the detailed GitHub Release/developer
changelog.

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
first installation do not. Runtime code does not parse version headings or categories and never
depends on their text structure to decide update behavior.

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
The verified main EXE filename is carried explicitly in the helper command and used for preflight,
health launch, and failure relaunch. The sidecar never guesses by scanning other EXEs in the install
directory, so Inno Setup's `unins000.exe` cannot be mistaken for another main executable.

**Restart Later** commits `pending.json` from a synced temporary file with Windows atomic replacement,
including when an older pending record exists. Once committed, best-effort directory durability cannot
turn success into an error or move the payload back to staging. If replacement or health confirmation
fails, the sidecar stops the failed new process, restores the old version, writes a bounded user-safe
failure result, and only then relaunches the old application. The next launch consumes that result via
the existing Notification component. An installation parent that is not writable is a preflight error;
the user should select a writable installation path instead of elevating the whole updater. After the
directory switch and before launching the new version, the sidecar preserves top-level Inno Setup
`unins<digits>.exe`, `.dat`, and `.msg` files from the rollback backup and rejects staged collisions, so
the Apps & Features uninstall entry remains valid after health confirmation.

HTTP plus ad-hoc signing is allowed only for controlled local/LAN tests. Production must use HTTPS,
`signing = "developer_id"`, `notarize = true`, and an `expected_team_id`. Install the Developer ID
Application certificate in the build keychain, use `MACOS_SIGN_IDENTITY` only when selection is
ambiguous, and provision the notarytool keychain profile named by `NOTARY_PROFILE` (default
`nexora`).
