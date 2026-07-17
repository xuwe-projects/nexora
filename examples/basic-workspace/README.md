# basic-workspace

这是一个使用 [Nexora](https://github.com/xuwe-projects/nexora) 生成的 Rust 桌面应用。

该示例使用 workspace 布局且不启用 Account，并额外提供“首页”和“关于”两个
Feature，用于验证框架默认 Sidebar、Tabs 与标签右键菜单。仓库中的示例通过相对路径
引用本地 `crates/nexora`；CLI 在仓库外创建的新项目默认使用可跨电脑迁移的 Git 依赖。

## 环境要求

- 支持 Rust 2024 edition 的稳定 Rust 工具链

## 运行

```bash
cargo run --locked
```

应用项目应提交 `Cargo.lock`，并在 CI 和部署中使用 `--locked` 保持完整依赖图稳定。
