---
title: 快速开始
order: 2
---

# 快速开始

## 安装 CLI

普通用户从 GitHub 安装当前已发布版本：

```bash
cargo install --git https://github.com/xuwe-projects/nexora --tag v0.11.0 nexora --locked --force --no-default-features --features cli --bin nexora
```

框架开发者可以在 Nexora 仓库根目录安装本地源码：

```bash
cargo install --path crates/nexora --locked --force --no-default-features --features cli --bin nexora
```

两条命令保持为单行，因此可以直接用于 Bash、zsh、PowerShell 和 CMD。Rustup 通常会配置
Cargo 的可执行目录；若终端找不到 `nexora`，确认 Unix 的 `$HOME/.cargo/bin` 或 Windows
的 `%USERPROFILE%\.cargo\bin` 已加入 `PATH`。

```bash
nexora --version
```

## 创建桌面应用

```bash
nexora create hello-nexora --layout workspace
cd hello-nexora
cargo run
```

## 创建带 Account 的全栈应用

```bash
nexora create hello-nexora --layout workspace --features account
cd hello-nexora
```

完善生成的两个配置文件后分别启动：

```bash
cargo run -p server -- config/server.toml
cargo run -- config/hello-nexora.toml
```

服务端首次启动且系统尚未初始化时，会在日志中输出可访问的 `/setup` 地址。
