---
title: CLI
order: 2
---

# CLI

```text
nexora create <name> --layout single
nexora create <name> --layout workspace
nexora create <name> --layout workspace --features account
nexora init [path] --layout workspace
nexora build
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
`init` preserves existing project rules and Skill files.
