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

- The update protocol now uses an Ed25519 signed envelope and `build_number`; SHA-256 remains the
  payload integrity check.
- macOS updater installation now starts an independent sidecar copied to a random temporary
  directory and uses one-time health confirmation to keep the new version or roll back.
- The account menu key context uses framework naming: `nexora_account_menu`.

## Removed

- Removed Jenkinsfile, the old desktop build env example, the old raw `latest.json` example, the
  macOS shell updater helper and test, and the macOS-only updater README.

## Upgrade Notes

1. Add root `nexora.toml` and declare each desktop app's `app_id`, publish target, object prefix,
   updater public keys, and required targets.
2. Run `nexora updater keygen --app <id>`, put the public key in `trusted_public_keys`, and store the
   private key in a secure file or CI secret.
3. Existing downstream projects should sync `.agents/skills/develop-nexora-updater` or rerun
   `nexora init .` so the CLI writes missing Skills.

## Validation

- Run `cargo fmt --all`, relevant crate tests, `cargo check`, strict Clippy, and
  `nexora lint --deny-warnings`. Windows/Linux replacement and macOS signing/notarization require
  matching host validation.
