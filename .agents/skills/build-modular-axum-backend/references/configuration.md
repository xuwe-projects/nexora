# 配置加载约定

## crate 职责

把可复用的 config-rs 配置源分层逻辑放在 `crates/configuration`。把应用专属配置结构放在对应的 `apps/<app>/src/config.rs`。

直接使用 Cargo 包名：

```rust
use config::{Config, Environment, File};
```

只公开预期使用的接口：

```rust
mod layered;

pub use config::ConfigError as ConfigurationError;
pub use layered::LayeredConfigLoader;
```

## 分层加载语义

实现支持以下能力的 `LayeredConfigLoader<T>`：

- 必需配置文件；
- 可选配置文件；
- 后添加的文件覆盖先添加的文件；
- 环境变量最后加载，因此优先级最高；
- 使用 `__` 表示嵌套字段；
- 自动解析布尔值、整数和浮点数；
- 对已登记的点分字段提供可选列表解析；
- 可以关闭环境变量加载。

加载器不决定文件路径，由调用方传入所有文件路径。

## 后端应用配置文件命名

`apps/` 下每个后端应用的目录名必须与 Cargo 包名一致。默认配置文件名由当前应用的 Cargo 包名决定：

```text
apps/server  ──> config/server.toml
apps/admin   ──> config/admin.toml
apps/worker  ──> config/worker.toml
```

允许调用方通过第一个命令行参数指定配置文件；未指定时使用 `config/<当前应用名>.toml`：

```rust
use std::path::PathBuf;

let app_name = env!("CARGO_PKG_NAME");
let config_path = std::env::args_os()
    .nth(1)
    .map(PathBuf::from)
    .unwrap_or_else(|| {
        PathBuf::new()
            .join("config")
            .join(format!("{app_name}.toml"))
    });

let settings = LayeredConfigLoader::<ServerConfig>::new()
    .with_required_file(config_path)
    .load()?;
```

默认路径相对于进程当前工作目录。说明应从工作区根目录执行 `cargo run -p <应用名>`。不要通过应用目录路径的字符串解析名称；优先使用 Cargo 提供的 `CARGO_PKG_NAME`。

## 应用配置类型

使用带 Serde 反序列化的强类型结构：

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub database: DatabaseConfig,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}
```

只把数据库连接地址传给连接池构造函数。禁止让可复用的 configuration crate 依赖应用专属类型。

## Git 忽略规则

保留示例文件，同时递归忽略本地敏感配置：

```gitignore
/config/**
!/config/**/
!/config/**/example.*
```

禁止在 `example.*` 文件中写入真实凭据。

每个后端应用都应提供对应示例文件，例如：

```text
config/example.server.toml
config/example.admin.toml
config/example.worker.toml
```
