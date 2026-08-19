---
title: CLI
order: 2
---

# CLI

## Installation

Install the released standalone `cli` package from its GitHub tag:

```bash
cargo install --git https://github.com/xuwe-projects/nexora --tag v0.41.0 cli --locked --force --bin nexora
```

Install the current local checkout from the Nexora repository root:

```bash
cargo install --path crates/cli --locked --force --bin nexora
```

These commands use no shell-specific line continuation or environment-variable syntax, so the same
single line works in Unix shells, PowerShell, and CMD.

## Commands

```text
nexora create <name> --layout single
nexora create <name> --layout workspace
nexora create <name> --layout workspace --features account
nexora init [path] --layout workspace
nexora icons generate --app <id>
nexora updater keygen --app <id>
nexora build
nexora build --app <id>
nexora build --app <id> --channel beta
nexora build --app <id> --all-channels
nexora publish
nexora publish --app <id> --dry-run
nexora publish --all --yes
nexora publish --app <id> yank
nexora doctor
nexora lint --workspace . --deny-warnings
nexora update
nexora version
```

## Read-only dependency diagnostics

`nexora doctor` only reads tool paths, versions, and capabilities for the current host. It never
downloads or installs software, runs `winget install`, `rustup target add`, `cargo install`,
`brew install`, or `xcode-select --install`, changes PATH, opens a browser, or launches a system
installer. Interactive terminals and CI use identical detection semantics; only presentation may
differ.

Every diagnostic includes purpose, required or conditional status, detected path and version,
supported range, official download URL, copyable manual installation and verification commands,
and the Nexora command to rerun. A missing required tool or incompatible version returns a non-zero
status; a tool used only by a disabled signing/notarization mode is a warning.

`nexora build` performs the same read-only preflight before writing
`dist/<app>/<channel>/release.json`, creating staging, or starting Cargo. A failed preflight writes
no new receipt or build state. Install the dependency manually, then rerun the original build
command.

`nexora doctor --fix` has been removed. Replace the old command with `nexora doctor`, then run the
installation commands yourself. The complete Windows/macOS tool, version, certificate, and key
matrix is in [Desktop updater](../desktop/updater.md#manually-installed-build-prerequisites).

Account needs both a desktop and a server package and therefore requires workspace layout.
Generated projects pin the current Nexora Git tag. Install the CLI with `cargo install --path` when
testing local source changes.

The local installation only replaces the CLI binary. To test a generated application against
unpublished framework code, temporarily change its root `nexora` workspace dependency to an
absolute `path` pointing at this repository's `crates/nexora` directory.

You only need to push a new Git tag when another repository must consume the changes. Testing the
current repository or a locally installed CLI does not require a release tag.

Both `nexora create` and `nexora init` generate a root `AGENTS.md` plus `.agents/skills`. The root
file contains always-on architectural constraints, while Skills provide task-specific workflows.
`init` preserves existing project rules and Skill files. The generated `publish-nexora-release`
Skill covers version bumps, complete release notes, contributor and Issue/PR attribution,
previous-to-current upgrade guides, and the tag/Release publishing gates.

Desktop updates are configured by the repository-root `nexora.toml`, which registers apps, updater
policy, branding resources, platform icons, and S3-compatible publish targets. Each app owns an
`assets/logos/<app_key>/` directory. `nexora icons generate --app <id>` regenerates only that app's
standard PNG, ICNS, and ICO files from its configured source PNG; manually managed resources require
an explicit `--force` before they can be replaced. When an app declares multiple `release.channels`,
interactive `nexora build` and `publish` commands show a channel multi-select with `default_channel`
preselected. CI should pass repeatable `--channel <name>` arguments or `--all-channels`; legacy
single-channel `release.channel` remains supported. `nexora build` only builds existing artifacts for the
current host and writes `artifact.json`; `nexora publish` only publishes existing artifacts and never
runs build implicitly. Publish uploads and verifies immutable versioned files first, then updates
the branded channel-root files and sequence manifest, and uploads signed `latest.json` last.
`latest.json` is an Ed25519 signed envelope. Public keys live in `trusted_public_keys`; private
signing keys are read only from a secure file or the environment variable named by
`signing_key_env`. Forced updates are written with `--minimum-supported-version`.

`nexora update` updates only the CLI itself from the official GitHub Release. It accepts HTTPS
assets only, verifies manifest schema, version, target, size, and SHA-256, then uses the platform's
safe replacement flow. It never edits project dependencies or source, updates desktop apps, falls
back to local Cargo compilation, or requests sudo/UAC.
