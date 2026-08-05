# Nexora Unreleased

## Added

- Added the `develop-nexora-updater` Skill and included it in the CLI scaffold Skill distribution.
  Downstream projects receive it through `nexora create`, `nexora init`, or by syncing
  `crates/nexora/templates/skills`. It covers updater protocol, build/publish, sidecars, forced
  gates, yanks, and legacy cleanup.
- Added `nexora updater keygen` for Ed25519 update signing keys. `nexora publish` and
  `publish yank` now use `nexora.toml` app configuration to generate signed `latest.json` files.
- `nexora publish` now performs real RustFS/S3-compatible uploads in update-file, immutable-manifest,
  and `latest.json` order, then verifies `latest.json` through the anonymous public URL.
- Added `examples/updater-macos/` for local RustFS validation of macOS v1 → v2 updates, forced
  updates, and health-failure rollback.
- Added Chinese and English updater docs covering the security model, RustFS configuration,
  keygen/build/publish/yank, macOS signing, Developer ID/notarization, and troubleshooting.

## Changed

- Windows first-install packaging now uses cargo-wix and WiX 5 to produce both a Simplified Chinese
  MSI and a Burn Setup EXE. The wizard exposes desktop shortcut, Start menu shortcut, and
  launch-after-finish options. Windows x86_64/ARM64 defaults to the pinned GPUI floor, Windows 10
  1703 (build 15063). The launch condition reads the real Windows build and does not use MSI's
  compatibility-reported `VersionNT64`, which is fixed at `603` on Windows 10. Windows startup now
  loads `nexora-updater.json` beside the main EXE instead of incorrectly searching for a macOS
  `.app`; builds without updater configuration may explicitly return no updater while existing
  invalid configuration remains a startup error. Windows GUI apps and sidecars no longer create a
  console window. The install-directory chooser remains available, while user-initiated removal no
  longer reports 1926 when another installer left an inaccessible cross-volume `Config.Msi`;
  major-upgrade removal keeps rollback enabled. Windows update ZIPs are now written directly in
  Rust with `/` archive separators, so PowerShell cannot introduce backslashes rejected by the
  safe extractor. Windows `signing = "none"` now retains Ed25519, SHA-256, ZIP safety, and PE
  architecture checks while correctly skipping Authenticode. `signing = "authenticode"` continues
  to enforce the Windows trust chain, certificate thumbprint, and publisher. Authenticode-only
  fields left in `none` mode now fail during build configuration validation.
  Windows in-app update transactions now use a hidden sibling directory on the installation volume
  and preflight replacement permission before exit. `pending.json` uses overwrite-capable atomic
  publication instead of opening a directory as a file for synchronization. The temporary sidecar
  no longer inherits and holds the installation directory as its working directory, while launched
  old and new apps still use that directory explicitly. Failed replacement restores and relaunches
  the old version, then reports the durable failure result on the next launch. Windows publish now
  emits architecture-specific `latest-<arch>.exe` / `latest-<arch>.msi` aliases and also emits
  `latest.exe` / `latest.msi` for a single-target release.
- The default scaffold and `examples/updater-windows` now declare `stable`, `beta`, and a
  `default_channel`. Interactive `nexora build` shows channel selection, while CI can use
  `--channel` or `--all-channels` explicitly.
- `apps.<app>.targets.required` is optional. `nexora build` defaults to the `rustc -vV` host target,
  supports repeated `--target` overrides, and repairs missing Rust targets plus macOS/Windows
  packaging dependencies in interactive terminals. System-installer completion can resume by
  rerunning the same command; non-interactive environments receive exact installation commands.
- Publish targets no longer accept `credential_env_prefix`. Access, secret, and session fields each
  fall back through channel-specific Nexora, base Nexora, and AWS variables independently.
- `nexora build` again writes standard `.sha256` sidecars for final ZIP and DMG artifacts, publish
  uploads them beside versioned releases, and `${BUILD_DATETIME}` now uses the build machine's local
  timezone with 24-hour `yyMMddHHmmss` formatting.
- The update protocol now uses an Ed25519 signed envelope and `build_number`; SHA-256 remains the
  payload integrity check.
- macOS updater installation now starts an independent sidecar copied to a random temporary
  directory and uses one-time health confirmation to keep the new version or roll back.
- The account menu key context uses framework naming: `nexora_account_menu`.

## Removed

- Removed Jenkinsfile, the old desktop build env example, the old raw `latest.json` example, the
  macOS shell updater helper and test, and the macOS-only updater README.
- Removed `credential_env_prefix` from `nexora.toml`; existing configuration must delete the field.

## Upgrade Notes

1. Add root `nexora.toml` and declare each desktop app's `app_id`, publish target, object prefix,
   updater public keys. Ordinary projects no longer declare required targets.
2. Run `nexora updater keygen --app <id>`, put the public key in `trusted_public_keys`, and store the
   private key in a secure file or CI secret.
3. Existing downstream projects should sync `.agents/skills/develop-nexora-updater` or rerun
   `nexora init .` so the CLI writes missing Skills.

## Validation

- Run `cargo fmt --all`, relevant crate tests, `cargo check`, strict Clippy, and
  `nexora lint --deny-warnings`. Windows/Linux replacement and macOS signing/notarization require
  matching host validation.
