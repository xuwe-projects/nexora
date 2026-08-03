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

[apps.console]
package = "console"
app_id = "com.example.console"
display_name = "Console"
publish_target = "internal"
object_prefix = "console"

[apps.console.release]
channel = "stable"
version = "${CARGO_PKG_VERSION}"
build_number = "${BUILD_DATETIME}"
minimum_supported_version = "0.0.0"
signing_key_file = ".secrets/console-update.key"

[apps.console.updater]
enabled = true
feed_url = "http://192.168.1.20:9000/desktop-releases/console"
channels = ["stable", "beta"]
trusted_public_keys = ["2026-main:ed25519:BASE64_PUBLIC_KEY"]
signing_key_env = "CONSOLE_UPDATE_SIGNING_KEY"
check_interval = "15m"
check_jitter = "1m"
offline_grace_period = "24h"
mandatory_restart_delay = "15m"
health_timeout = "2m"

[apps.console.targets]
required = ["aarch64-apple-darwin", "x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"]

[apps.console.platforms.macos]
signing = "ad_hoc"
notarize = false
```

Rules:

- `apps` table names are stable CLI app IDs. `package` must exist and be a desktop binary. `app_id` is permanent and globally unique.
- Multiple apps may share a publish target, but `app_id`, `object_prefix` and output paths must not conflict.
- A single registered app is selected automatically. Multiple apps use an interactive menu; non-interactive commands require `--app`, while only publish accepts explicit `--all`. Publish must never implicitly publish all apps.
- `release.version` accepts a literal SemVer or the complete `${CARGO_PKG_VERSION}` expression. The expression resolves the selected app `package` through `cargo metadata --no-deps --format-version 1`, including `version.workspace = true`; fragments, `${CARGO_VERSION}`, arbitrary environment variables and unknown expressions are rejected.
- `release.build_number` accepts a positive integer or the complete `${BUILD_DATETIME}` expression. The expression is UTC `yyMMddHHmmss` and uses `max(current UTC value, previous local build number + 1)` after same-second builds or clock rollback. Unknown strings, zero and overflow are rejected.
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

- `nexora build` compiles only the selected app main binary and updater sidecar, initial installer artifact, update payload, SHA-256 and `artifact.json`.
- It does not access S3 and does not publish.
- Before any target build, resolve one identity and atomically write `dist/<app>/<channel>/release.json` with `schema_version`, `app_key`, `package`, `channel`, final `version`, final `build_number`, `version_source`, `build_number_source`, signed-integer Unix-second `created_at`, and planned `targets`.
- All targets in one build use that receipt. A matching incomplete retry reuses it. After every planned target artifact is complete, another explicit dynamic build creates a strictly higher build number and updates the current receipt without deleting old versioned artifacts. Corrupt/unsupported receipts fail before build and are never reconstructed from directory names.
- Targets come from `apps.<id>.targets.required`; build processes every required target the current host can legally build.
- Cross-OS pseudo-packaging is forbidden. Complete releases use platform runners.
- Sidecar locations: macOS `Contents/Helpers/<app>-updater`, Windows `<app>-updater.exe`, Linux `<app>-updater`.
- Sidecar embeds `app_id`, protocol, trusted public keys and main-program identity. Before applying, it copies itself to a random temp directory.
- Outputs are isolated by `app/channel/version/build/target`.

Platform outputs:

- macOS: write version, build number, configured ICNS, updater settings and sidecar before signing; create both DMG and `ditto` `.app.zip` from that same signed `.app`. The ICNS is copied to Resources and written to `CFBundleIconFile` for the `.app` only. The DMG file and mounted volume keep their system-default appearance. `artifact.json` describes ZIP and DMG; the updater manifest contains only `.app.zip`.
- Windows: optional user or machine `Setup.exe`, plus update ZIP. User scope writes LocalAppData Programs; machine scope writes Program Files with UAC. First release has no Authenticode requirement.
- Linux: AppImage, optionally user-directory portable archive. Update AppImage itself or `tar.zst`; package-manager formats are future work.

## Publish

- `nexora publish` publishes existing artifacts only and must never run build implicitly.
- It reads `nexora.toml`, selects apps, then reads the current release receipt. It validates the receipt against app/package/channel, selected Cargo package version, current sources/configuration and required targets; it validates each `artifact.json`, file existence, size and SHA-256 before upload.
- Dry-run performs the same receipt, artifact and remote checks without local or remote writes. An available identity must be strictly greater than the remote `(version, build_number)`. Yank uses the receipt identity.
- Stable publish must include all required targets.
- Upload versioned `.app.zip`, versioned DMG, notes, immutable sequence manifest, target-specific latest DMGs, optional single-target `latest.dmg`, and finally `latest.json`; then verify mutable objects and every updater URL anonymously.
- Read and verify remote `latest.json` before publishing. HTTP 404 means sequence 1; otherwise use remote sequence plus one. Dry-run performs the same read without writes. Re-read before mutable uploads and reject concurrent sequence changes.
- Use layout:

```text
<prefix>/<app>/<channel>/latest.json
<prefix>/<app>/<channel>/manifests/<sequence>.json
<prefix>/<app>/<channel>/releases/<version>/<build>/<target>/...
```

Versioned objects should be long cached; `latest.json` should be no-cache or short-cache. Keep `endpoint` and `public_base_url` separate. Support region and path-style S3. Never log secrets.

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
