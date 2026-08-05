# Nexora 跨平台桌面自动更新协议

## 范围

本协议用于 Nexora workspace 中一个或多个桌面 app 的构建、发布、匿名更新检查、sidecar 安装、回滚、强制更新和撤回。实现时按以下边界拆分：

- `protocol/core`：TOML 配置、签名信封、版本比较、manifest sequence、目标选择、安全解压和状态机。
- `sidecar runtime`：独立进程下载、验签、等待主程序退出、事务替换、重启、健康确认和回滚。
- `platform adapters`：macOS `.app`、Windows user/machine scope、Linux AppImage/便携版。
- `nexora::desktop integration`：全局 `UpdateCoordinator`、窗口级 Dialog layer、登录前门禁和多窗口阻断。
- `CLI build/publish`：只构建当前宿主可构建产物；只发布已有 artifact；S3 兼容对象存储上传顺序和匿名 URL 校验。

## nexora.toml

根目录 `nexora.toml` 只用于项目、构建和发布，提交到 Git，但不要把完整文件装入应用。推荐结构：

```toml
schema_version = 1

[publish.targets.internal]
provider = "s3"
endpoint = "http://192.168.1.20:9000"
bucket = "desktop-releases"
region = "us-east-1"
force_path_style = true
public_base_url = "http://192.168.1.20:9000/desktop-releases"
allow_insecure_http = true

[publish.targets.internal.channels.nightly]
endpoint = "http://192.168.1.30:9000"
public_base_url = "http://192.168.1.30:9000/desktop-releases"
allow_insecure_http = true

[apps.console]
package = "console"
app_id = "com.example.console"
display_name = "Console"
publish_target = "internal"
object_prefix = ""

[apps.console.release]
channel = "stable"
version = "${CARGO_PKG_VERSION}"
build_number = "${BUILD_DATETIME}"
minimum_supported_version = "0.0.0"
signing_key_file = ".secrets/console-update.key"

[apps.console.updater]
enabled = true
feed_url = "http://192.168.1.20:9000/desktop-releases/console/stable/latest.json"
channels = ["stable", "beta"]
trusted_public_keys = ["2026-main:ed25519:BASE64_PUBLIC_KEY"]
signing_key_env = "CONSOLE_UPDATE_SIGNING_KEY"
check_interval = "15m"
check_jitter = "1m"
offline_grace_period = "24h"
mandatory_restart_delay = "15m"
health_timeout = "2m"

[apps.console.platforms.macos]
signing = "ad_hoc"
notarize = false
```

Rules:

- `apps` table names are stable CLI app IDs. `package` must exist and be a desktop binary. `app_id` is permanent and globally unique.
- Multiple apps may share a publish target. The `apps` table key is the stable remote directory identity; `display_name` is only user-visible metadata and the distribution filename stem. Changing `display_name` must not move the feed root.
- `object_prefix = ""` means no extra prefix. Non-empty values keep safe-path validation; joining must never create leading/doubled slashes or empty components.
- `publish.targets.<name>.channels.<channel>` overrides the base target by field. Omitted fields inherit, and the merged provider, URLs, bucket, region, path style and HTTP policy are validated together.
- Publish resolves each credential field independently in this order: `NEXORA_PUBLISH_<CHANNEL>_<FIELD>`, `NEXORA_PUBLISH_<FIELD>`, then `AWS_<FIELD>`. Empty values continue fallback, access/secret are required, session token is optional, and `RUSTFS_*` is unsupported.
- A single registered app is selected automatically. Multiple apps use an interactive menu; non-interactive commands require `--app`, while only publish accepts explicit `--all`. Publish must never implicitly publish all apps.
- `release.version` accepts a literal SemVer or the complete `${CARGO_PKG_VERSION}` expression. The expression resolves the selected app `package` through `cargo metadata --no-deps --format-version 1`, including `version.workspace = true`; fragments, `${CARGO_VERSION}`, arbitrary environment variables and unknown expressions are rejected.
- `release.build_number` accepts a positive integer or the complete `${BUILD_DATETIME}` expression. The expression uses the build machine's local timezone and 24-hour `yyMMddHHmmss`, then applies `max(current local value, previous local build number + 1)` after same-second builds, clock rollback, daylight-saving fallback or timezone changes. Unknown strings, zero and overflow are rejected.
- Explicit version/build values remain compatible. Build freezes the resolved identity in `dist/<app>/<channel>/release.json`; publish and yank read this receipt and never recompute dynamic values. `manifest_sequence` is never configured locally.
- Server, library, migration and unregistered packages do not participate.

## Runtime Overrides

System-level read-only `updater.toml` may override only `feed_url` and `allow_insecure_http`.

- macOS: `/Library/Application Support/<app_id>/updater.toml`
- Windows: `%ProgramData%\<app_id>\updater.toml`
- Linux: `/etc/<app_id>/updater.toml`

If absent, use build defaults. If present but invalid, disable this check and surface an error; do not silently fall back. Administrators cannot override `app_id`, channel, public keys, interval or grace periods. Main app and updater must not write or delete this file.

## Security

- Updates are independent from Account and must work before login.
- Client performs anonymous reads. S3 AK/SK are publish-only secrets and must never enter the client.
- Plain HTTP is denied unless the relevant config explicitly sets `allow_insecure_http = true`.
- Ed25519 manifest signatures are mandatory and cannot be disabled. SHA-256 only protects payload integrity.
- The updater validates the manifest independently and must not trust main-process URL, hash, `app_id`, target or version arguments.
- Reject archive entries with absolute paths, `..`, symlink escape, cross-device staging, insufficient space or wrong permissions.
- On Windows, keep staging, pending payloads, backup, health state and install results under the hidden sibling root `<install-parent>/.nexora-updater/<app_id>`. Preflight same-volume and directory-rename permission before the main process exits; never require elevation to compensate for a cross-volume design.

## Keys And Signed Manifest

`nexora updater keygen --app <id>` generates an Ed25519 keypair. Store public keys as `key_id:ed25519:BASE64_PUBLIC_KEY` in `trusted_public_keys`; store private keys only in a secret file or CI secret. `publish` first reads `release.signing_key_file` relative to `nexora.toml`, and falls back to the environment variable named by `signing_key_env` only when the file setting is absent or empty. Multiple public keys support rotation.

`latest.json` is a signed envelope:

- `payload` contains at least `manifest_sequence`, `app_id`, `channel`, `version`, `build_number`, `minimum_supported_version`, `published_at`, release status, release notes and target URL/size/SHA-256 entries.
- `signatures` contains one or more `{ key_id, algorithm = "ed25519", signature }` entries over the canonical JSON payload bytes.
- Clients persist the highest accepted `manifest_sequence` and reject lower sequences.

## Versions And Channels

- `version` and `build_number` are resolved from the selected app's release configuration, then frozen in its release receipt. Version is valid SemVer and build number is greater than zero.
- Clients compare `(version, build_number)`. Same SemVer may be republished only with a higher build number.
- `stable`, `beta` and `nightly` are fixed at build time and use separate `latest.json`; runtime channel switching is not allowed.

## Build

- `nexora build` compiles only the selected app main binary and updater sidecar, initial installer artifact, update payload, standard `<artifact>.sha256` sidecars and `artifact.json`.
- Distribution files use `<display_name>-<arch><suffix>` with normalized `x86_64`/`aarch64`; version, build number, package and full triple do not enter the filename. `display_name` must pass cross-platform filename validation. Internal executable and sidecar names continue to use `package`.
- It does not access S3 and does not publish.
- Before any target build, resolve one identity and atomically write `dist/<app>/<channel>/release.json` with `schema_version`, `app_key`, `package`, `channel`, final `version`, final `build_number`, `version_source`, `build_number_source`, signed-integer Unix-second `created_at`, and planned `targets`.
- All targets in one build use that receipt. A matching incomplete retry reuses it. After every planned target artifact is complete, another explicit dynamic build creates a strictly higher build number and updates the current receipt without deleting old versioned artifacts. Corrupt/unsupported receipts fail before build and are never reconstructed from directory names.
- Build defaults to the exact host target from `rustc -vV`. Repeated `--target` arguments explicitly override it. `apps.<id>.targets.required` remains an optional legacy compatibility source and is not required in new projects.
- In an interactive terminal, build repairs missing host dependencies before compiling, including Rust targets, Cargo packaging tools, macOS Homebrew tools, pinned cargo-wix, Nexora-managed user-level .NET SDK, WiX 5.0.2 and matching extensions. System installers such as Xcode Command Line Tools and Windows SDK may pause the build and require rerunning the same command. Non-interactive builds fail with exact installation commands instead of starting installers.
- Cross-OS pseudo-packaging is forbidden. Complete releases use platform runners.
- Sidecar locations: macOS `Contents/Helpers/<app>-updater`, Windows `<app>-updater.exe`, Linux `<app>-updater`.
- Sidecar embeds `app_id`, protocol, trusted public keys and main-program identity. Before applying, it copies itself to a random temp directory.
- Outputs are isolated by `app/channel/version/build/target`.

Platform outputs:

- macOS: write version, build number, configured ICNS, updater settings and sidecar before signing; create both DMG and `ditto` `.app.zip` from that same signed `.app`. The ICNS is copied to Resources and written to `CFBundleIconFile` for the `.app` only. The DMG file and mounted volume keep their system-default appearance. `artifact.json` describes ZIP and DMG; the updater manifest contains only `.app.zip`.
- Windows x86_64/ARM64: a Simplified Chinese WiX MSI, a branded `.exe` installer, and an update ZIP. Generate separate UTF-8 RC files for main/updater with `#pragma code_page(65001)`, string table `080404B0` and Translation `0x0804,1200`. Main `FileDescription` is `display_name`; updater appends “更新程序”; `InternalName`/`OriginalFilename` retain package identities. Link each resource before Authenticode signing. User scope defaults to LocalAppData Programs without UAC while preserving the install-directory chooser; machine scope is rejected until elevation and updater permissions have an explicit design. The UI exposes desktop shortcut, Start menu shortcut, and launch-after-finish checkboxes. Main and sidecar executables use the Windows GUI subsystem. `signing = "none"` still enforces Ed25519, artifact size/SHA-256, ZIP safety and PE architecture checks but skips Authenticode; Authenticode-only TOML fields conflict with that mode. `signing = "authenticode"` signs all Windows artifacts and requires the updater to verify both staged EXEs with Windows trust, certificate thumbprint and publisher. The bundled thumbprint and publisher must be both absent or both present. The Rust ZIP writer must encode archive entry names with `/`; do not use PowerShell `Compress-Archive` or serialize native Windows `Path` separators into the update protocol. User-initiated removal disables MSI rollback only after `InstallInitialize` so a stale cross-volume `Config.Msi` ACL cannot break cleanup; major-upgrade removal retains rollback. The current pinned GPUI floor is Windows 10 1703/build 15063.
- Linux: AppImage, optionally user-directory portable archive. Update AppImage itself or `tar.zst`; package-manager formats are future work.

## Publish

- `nexora publish` publishes existing artifacts only and must never run build implicitly.
- It reads `nexora.toml`, selects apps, then reads the current release receipt. It validates the receipt against app/package/channel, selected Cargo package version and current sources/configuration; the receipt itself freezes the release targets. It validates each target's `artifact.json`, file existence, size and SHA-256 before upload.
- Dry-run performs the same receipt, artifact and remote checks without local or remote writes. An available identity must be strictly greater than the remote `(version, build_number)`. Yank uses the receipt identity.
- Stable publish must include every target frozen in the release receipt.
- Upload and anonymously verify versioned artifacts, checksum sidecars and notes first; after the concurrent-sequence recheck, update channel-root branded artifacts and checksums, then the immutable sequence manifest, and finally signed `latest.json`. Updater URLs always reference immutable versioned payloads. Do not generate installer/update `latest.*` aliases; `latest.json` remains mandatory and last.
- Read and verify remote `latest.json` before publishing. HTTP 404 means sequence 1; otherwise use remote sequence plus one. Dry-run performs the same read without writes. Re-read before mutable uploads and reject concurrent sequence changes.
- Use layout:

```text
[<object_prefix>/]<app_key>/<channel>/latest.json
[<object_prefix>/]<app_key>/<channel>/manifests/<sequence>.json
[<object_prefix>/]<app_key>/<channel>/<artifact>
[<object_prefix>/]<app_key>/<channel>/<version>/<build>/<arch>/<artifact>
```

Versioned objects should be long cached; `latest.json` and channel-root branded objects should be no-cache or short-cache. There is no `releases` segment and public architecture directories are normalized. Keep `endpoint` and `public_base_url` separate. Support region and path-style S3. Never log secrets. Never delete legacy aliases or immutable objects automatically; administrators may clean old channel-root aliases manually.

## Emergency Yank

`nexora publish --app <id> yank` publishes a higher-sequence control manifest for the configured release. It blocks clients that have not installed the yanked build and may cancel forced requirements, but it never downgrades already installed clients. Code rollback requires a higher version or build number.

## Updater Flow

1. `UpdateCoordinator` starts and sidecar fetches/verifies the manifest.
2. Main program shows global update UI.
3. After user confirmation, main program starts a temp-copied updater with random session IPC.
4. Updater downloads, streams progress, re-validates signature/app/channel/target/version/size/hash and safely extracts to staging.
5. Updater reports ready; main program exits; updater waits for PID.
6. Updater keeps old version, performs transactional switch, starts new program and passes one-time session.
7. New program reports health after initialization and first main window creation.
8. On launch failure, crash or health timeout, restore backup, restart old version and record the reason.

On Windows, publishing `pending.json` must use a synced temporary file plus an atomic replace that can overwrite an existing destination. Directory durability is best-effort after that commit and must never turn a committed record into an error that moves its payload back to staging. A failed transaction records a bounded, user-safe result before restarting the restored version; the next launch consumes that result through the shared notification UI.

Only one update session per app may run. Never rerun first-install installers during self-update.

## Policy And UI

- Forced update uses `minimum_supported_version`; login/Shell must be gated before business UI.
- Offline grace defaults to 24h. Signed cache within grace may enter. First launch without cache must check successfully. Clock rollback requires recheck. Administrators cannot extend the grace.
- Known forced updates bypass offline grace. Running forced update downloads in the background, then starts the mandatory restart countdown, default 15 minutes.
- Window UI must use `gpui-component` Dialog/AlertDialog/Progress/Button/Notification as a window-level layer, not `FeatureElement::panel_overlay`.
- The modal covers title bar, Sidebar, login page, tabs and business features. Esc and overlay click cannot bypass. Optional updates require Immediate, Background or Later; forced updates provide no Later.
- The main window owns the single Dialog. Other native windows block interaction and activate the main window.

## Legacy Cleanup

Delete or replace current Jenkins/Pipeline files, `BUILD_MACOS`/`BUILD_WINDOWS` docs, `config/desktop-build.env.example`, `config/updater/latest.example.json`, shell-only install helpers and tests, macOS-only updater README, old latest protocol, macOS-only build parameters, default Console package assumptions, `console_account_menu` product naming and lint special-cases.

Keep historical changelog files, third-party product/crate names such as ZITADEL Console and `console_error_panic_hook`, user-local untracked configs and unrelated workspace changes. Cleanup must be semantic, not broad string deletion.

## Validation

Cover multi-app config, conflicts and selectors; build/publish separation; targets; Ed25519 signatures, unknown keys, rotation and replay; SemVer/build/minimum version; HTTP default deny and explicit allow; S3 upload order and latest failure behavior; anonymous reads; safe extraction; concurrency lock; cleanup and rollback; Windows/macOS/Linux platform paths; GPUI global Dialog and multi-window blocking; frequency, jitter, wake checks and 24h grace; absence of legacy references.
