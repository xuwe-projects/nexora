---
title: CLI
order: 2
---

# CLI

## Installation

Install a published GitHub tag:

```bash
cargo install --git https://github.com/xuwe-projects/nexora --tag v0.23.2 nexora --locked --force --no-default-features --features cli --bin nexora
```

Install the current local checkout from the Nexora repository root:

```bash
cargo install --path crates/nexora --locked --force --no-default-features --features cli --bin nexora
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
nexora build --app <id> --channel beta --channel nightly
nexora build --all-apps --all-channels
nexora publish
nexora publish --app <id> --channel nightly --dry-run
nexora publish --all-apps --all-channels --yes
nexora publish --app <id> --channel beta yank
nexora doctor
nexora lint --workspace . --deny-warnings
nexora version
```

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
an explicit `--force` before they can be replaced. `nexora build` only builds existing artifacts for the
current host and writes `artifact.json`; `nexora publish` only publishes existing artifacts and never
runs build implicitly. Publish uploads update files and notes, the immutable manifest, and
`latest.json` to RustFS/S3-compatible storage, then anonymously reads and verifies `latest.json`.
`latest.json` is an Ed25519 signed envelope. Public keys live in `trusted_public_keys`; private
signing keys are read only from a secure file or the environment variable named by
`signing_key_env`. Forced updates are written with `--minimum-supported-version`.

Both `--app` and `--channel` are repeatable; `--all-apps` and `--all-channels` conflict with their
corresponding explicit selectors. Legacy `publish --all` remains an alias for `--all-apps`.
Non-interactive commands use each app's `default_channel` when no channel is provided. Interactive
commands multi-select apps first and channels per app, preselecting each default. Every selected
app/channel pair completes read-only preflight before any build or upload, then executes
sequentially. Publish and yank read the exact receipt and artifacts for each pair; dry-run prints the
entire plan without remote writes.
