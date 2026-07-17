# Nexora

Nexora 是一个基于 GPUI、Axum 与 PostgreSQL 的 Rust 桌面全栈应用框架，提供自动发现的
Feature、桌面 Shell、Account 认证授权、默认用户与角色管理，以及可组合的服务端启动器。

> 当前处于 early alpha，公共 API 仍可能调整。

完整指南请阅读 [Nexora 文档](https://xuwe-projects.github.io/nexora/)。

## 本地安装 CLI

在本仓库根目录运行：

```bash
cargo install --path crates/nexora --locked --force \
  --no-default-features --features cli --bin nexora
```

这会直接安装当前工作区代码，不需要先发布 GitHub tag。只有生成的外部项目需要从 Git
拉取尚未发布的 Nexora 版本时，才需要推送新 tag：

如果要在发布 tag 前同时测试“生成后的应用”，请把生成项目根 `Cargo.toml` 中的 Nexora
依赖临时改成当前仓库的绝对路径，例如：

```toml
nexora = { path = "/path/to/nexora/crates/nexora", default-features = false }
```

```bash
cargo install --git https://github.com/xuwe-projects/nexora \
  --tag v0.1.1 --locked --force \
  --no-default-features --features cli --bin nexora
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

服务端尚未初始化时会在日志中输出 `/setup` 地址。数据库升级由 SQLx 迁移记录自动管理，
不需要 `initialize_empty_database` 开关。

## 开发验证

```bash
cargo fmt --all -- --check
cargo test -p nexora --all-features
cargo clippy -p nexora --all-features --all-targets -- -D warnings
cd docs && bun install && bun run build
```

License: MIT OR Apache-2.0
