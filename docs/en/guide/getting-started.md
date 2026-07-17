---
title: Quick Start
order: 2
---

# Quick Start

## Install the local CLI

Run this from the Nexora repository root:

```bash
cargo install --path crates/nexora --locked --force \
  --no-default-features --features cli --bin nexora
```

For a released Git tag, replace `--path` with `--git` and `--tag`.

## Create a desktop application

```bash
nexora create hello-nexora --layout workspace
cd hello-nexora
cargo run
```

## Create a full-stack Account application

```bash
nexora create hello-nexora --layout workspace --features account
cd hello-nexora
```

Complete the generated configuration files, then run:

```bash
cargo run -p server -- config/server.toml
cargo run -- config/hello-nexora.toml
```

When initialization is still required, the server logs the accessible `/setup` URL after binding.
