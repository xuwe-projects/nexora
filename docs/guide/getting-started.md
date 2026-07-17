---
title: 快速开始
order: 2
---

# 快速开始

## 安装本地 CLI

在 Nexora 仓库根目录执行：

```bash
cargo install --path crates/nexora --locked --force \
  --no-default-features --features cli --bin nexora
```

从 Git tag 安装时把 `--path` 换成 `--git` 与 `--tag`。

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
