---
title: Quick Start
order: 2
---

# Quick Start

## Install the CLI

Install the current release from GitHub:

```bash
cargo install --git https://github.com/xuwe-projects/nexora --tag v0.4.1 nexora --locked --force --no-default-features --features cli --bin nexora
```

Framework contributors can install their local checkout from the repository root:

```bash
cargo install --path crates/nexora --locked --force --no-default-features --features cli --bin nexora
```

Both commands are intentionally single-line and work in Bash, zsh, PowerShell, and CMD. Rustup
normally configures Cargo's executable directory. If `nexora` is not found, add
`$HOME/.cargo/bin` on Unix or `%USERPROFILE%\.cargo\bin` on Windows to `PATH`.

```bash
nexora --version
```

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
