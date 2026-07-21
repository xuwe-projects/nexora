# Nexora

Nexora 是一个基于 GPUI、Axum 与 PostgreSQL 的 Rust 桌面全栈应用框架，提供自动发现的
Feature、桌面 Shell、Account 认证授权、默认用户与角色管理，以及可组合的服务端启动器。

> 当前处于 early alpha，公共 API 仍可能调整。

完整指南请阅读 [Nexora 文档](https://xuwe-projects.github.io/nexora/)。

## 安装 CLI

安装已发布的 `v0.15.1`：

```bash
cargo install --git https://github.com/xuwe-projects/nexora --tag v0.15.1 nexora --locked --force --no-default-features --features cli --bin nexora
```

在 Nexora 仓库根目录安装本地源码：

```bash
cargo install --path crates/nexora --locked --force --no-default-features --features cli --bin nexora
```

这两条单行命令都可以直接用于 Bash、zsh、PowerShell 和 CMD。Rustup 通常会自动把 Cargo
的可执行目录加入 `PATH`；Unix 默认为 `$HOME/.cargo/bin`，Windows 默认为
`%USERPROFILE%\.cargo\bin`。

本地安装只替换 CLI，不需要 GitHub tag。若要让生成项目也使用尚未发布的框架代码，请把
生成项目根 `Cargo.toml` 中的 Nexora 依赖临时改成当前仓库的绝对路径：

```toml
nexora = { path = "/path/to/nexora/crates/nexora", default-features = false }
```

## 创建项目

```bash
# 桌面应用
nexora create my-app --layout workspace

# 带 Account、服务端和初始化流程的全栈应用
nexora create my-app --layout workspace --features account
```

生成的桌面 package 只需启用 `desktop,derive`，服务端只需启用 `server,derive`。GPUI 与
gpui-component 由应用直接依赖，Nexora 不再把它们作为自己的公共命名空间导出。

```bash
cd my-app
cargo run
```

Account workspace 需要先填写带注释的 `config/server.toml` 和桌面配置，然后分别启动：

```bash
cargo run -p server -- config/server.toml
cargo run -- config/my-app.toml
```

服务端尚未初始化时会在日志中输出 `/setup` 地址。宿主先把
`nexora::server::migrations()` 与业务迁移合并成唯一 SQLx `Migrator`，数据库升级继续由
`_sqlx_migrations` 自动管理，不需要 `initialize_empty_database` 开关。

生成项目自带 `publish-nexora-release` Skill，用于整理完整改动、标注处理人、编写上一版本到
当前版本的升级说明，并在验证通过后发布 tag 与 GitHub Release。

## 开发验证

```bash
cargo fmt --all -- --check
cargo test -p nexora --all-features
cargo clippy -p nexora --all-features --all-targets -- -D warnings
cd docs && bun install && bun run build
```

License: MIT OR Apache-2.0
