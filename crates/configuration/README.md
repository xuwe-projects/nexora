# Configuration

`configuration` 使用 `config-rs` 提供类型化配置加载，并补充桌面用户配置所需的跨平台路径、
原子写入和 schema 版本检查。

## 服务配置

配置优先级为：

```text
Serde 默认值 < TOML 文件 < 环境变量
```

环境变量不使用统一项目或组织前缀，双下划线表示嵌套层级：

```text
SERVER__HOST=0.0.0.0
SERVER__PORT=8080
DATABASE__URL=postgres://localhost/example
```

模板项目约定把运行时配置样例放在仓库根目录 `config/` 中。服务端默认读取
`config/local.toml`，也可以通过 `--profile production` 切换为 `config/production.toml`，
或使用 `--config /path/to/server.toml` 指定任意文件。

## 桌面用户配置

`UserConfigStore` 使用 `directories::ProjectDirs` 计算当前平台标准配置目录。用户配置不会读取
环境变量，也不应保存 API 地址、更新地址、签名身份、访问密钥等受信任配置。

保存时先写入并同步临时文件，再替换正式 TOML，避免中断写入产生半份配置。
