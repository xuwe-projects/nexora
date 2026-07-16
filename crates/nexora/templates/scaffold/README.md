# {{ project_name }}

这是一个使用 [Nexora](https://github.com/xuwe-projects/nexora) 生成的 Rust 桌面应用。

## 环境要求

- 支持 Rust 2024 edition 的稳定 Rust 工具链

## 运行

```bash
cargo run
```
{% if account_enabled %}
首次启动前，请先完善 `config/server.toml` 和 `config/{{ project_name }}.toml`，然后分别启动服务端与桌面端：

```bash
cargo run -p server -- config/server.toml
cargo run -- config/{{ project_name }}.toml
```
{% endif %}
