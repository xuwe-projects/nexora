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

- Windows first-install packaging now uses pinned Inno Setup 6.7.3 and produces only a Simplified
  Chinese Setup EXE plus the sidecar update ZIP; MSI and Burn bundles are no longer generated. Build
  and doctor detect or repair ISCC through winget, and no longer require cargo-wix, WiX extensions,
  or a Nexora-managed .NET SDK. Installation is fixed to the unelevated current-user scope under
  `%LOCALAPPDATA%\Programs\<app_id>` by default, with directory selection, desktop/Start menu
  shortcuts, and launch-after-finish options preserved. Setup and the update ZIP share one staging
  directory. Publish uploads both artifacts and checksums while signed `latest.json` references only
  `windows_update_zip`. Windows startup loads `nexora-updater.json` beside the main EXE; GUI apps
  and sidecars do not create console windows. Rust writes update ZIP entries with `/` separators.
  `signing = "none"` retains Ed25519, SHA-256, ZIP safety, and PE architecture checks, while
  `authenticode` continues to enforce the Windows trust chain, certificate thumbprint, and publisher.
  In-app transactions still use a hidden sibling directory on the installation volume, preflight
  replacement permission before exit, and restore/relaunch the old version on failure.
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

## Fixed

- Windows build dependency checks now read the `ISCC.exe` PE fixed file-version resource instead
  of parsing a patch version that the official `/?` banner does not print. The pinned winget
  installation of Inno Setup 6.7.3 is now detected exactly.

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
