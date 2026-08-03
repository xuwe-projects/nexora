---
title: 配置
order: 1
---

# 配置

根配置派生 serde 与 `nexora::Settings`：

```rust
#[derive(serde::Deserialize, nexora::Settings)]
struct Settings {
    api: nexora::desktop::ApiSettings,
    #[nexora(account_client)]
    account: nexora::desktop::AccountSettings,
}
```

桌面 API 使用独立 `[api]` 表：

```toml
[api]
endpoint = "http://127.0.0.1:3000"
allow_insecure_http = false
```

HTTPS endpoint 始终允许；`localhost`、IPv4/IPv6 loopback HTTP 可用于本机开发。非 loopback
HTTP 只有在显式设置 `allow_insecure_http = true` 时才会通过校验。纯 HTTP 会以明文传输
Bearer Token，该开关只适用于调用方已经接受风险的内网或受控环境。

生产部署通常使用 HTTPS：

```toml
[api]
endpoint = "https://api.example.com"
allow_insecure_http = false
```

已接受风险的受控内网 HTTP 需要显式放行：

```toml
[api]
endpoint = "http://10.0.0.20:3000"
allow_insecure_http = true
```

服务端监听 IP 与端口分开配置：

```toml
[server]
ip = "127.0.0.1"
port = 3000
```

启用 Account 服务端时还需要 ZITADEL 管理配置：

```toml
[oidc]
issuer_url = "https://identity.example.com"
audience = "nexora-api"
organization_id = "zitadel-organization-id"
project_id = "zitadel-project-id"
personal_access_token = "replace-through-secret-injection"
```

`organization_id` 用于 UserService v2 创建人类用户；`project_id` 用于同步系统角色，两者职责
不同。PAT 必须属于有权管理目标 Organization/Project 的服务账号，生产环境应通过
`OIDC__PERSONAL_ACCESS_TOKEN` 或密钥系统注入，不要提交真实值。

环境变量以 `__` 表示嵌套字段。`nexora::config::initialize(None)` 的读取优先级为：调用方传给
`initialize(Some(path))` 的显式路径、有效的命令行 TOML 路径、标准 macOS app bundle 中的
`Contents/Resources/config/<package>.toml`、本地 `config/<package>.toml`。updater 健康检查
参数不会被误识别为配置路径，bundle 检测也不会任意向上扫描。敏感值应由环境变量或密钥系统
注入。

构建多通道安装包时，每个 channel 的运行配置按以下顺序选择：channel 显式
`runtime_config`、`config/<package>-<channel>.toml`、`config/<package>.toml`。显式路径缺失
不会回退。路径必须是 workspace 内的安全相对路径，canonicalize 后仍需位于 workspace；符号
链接逃逸、绝对路径、`..` 与 Windows Prefix 都会被拒绝。选定配置会在签名前复制到稳定的
bundle 路径。桌面运行配置会随安装包公开分发，不得包含数据库密码、PAT、私钥或其他秘密。

服务端 Setup secret 只在未初始化时有效。迁移记录由 `_sqlx_migrations` 管理，不需要也不
允许通过 `initialize_empty_database` 之类的人工布尔开关控制升级。

## 自动更新发布配置

桌面自动更新的构建和发布配置放在仓库根目录 `nexora.toml`，不会完整打包进客户端。`publish`
目标支持 S3 兼容对象存储：

```toml
[publish.targets.rustfs]
provider = "s3"
endpoint = "http://127.0.0.1:9000"
bucket = "desktop-releases"
region = "us-east-1"
force_path_style = true
public_base_url = "http://127.0.0.1:9000/desktop-releases"
allow_insecure_http = true
```

`endpoint` 是带签名的 S3 API 地址；`public_base_url` 是客户端匿名读取地址。本地 RustFS 使用
HTTP 时必须显式开启 `allow_insecure_http`。发布凭据来自 `AWS_ACCESS_KEY_ID` /
`AWS_SECRET_ACCESS_KEY` 或 `RUSTFS_ACCESS_KEY_ID` / `RUSTFS_SECRET_ACCESS_KEY`，不得写入
配置文件。每次 E2E 应使用唯一 `object_prefix`。

每个桌面 app 必须在同一注册记录中声明稳定 app key、bundle identifier 和品牌资源。single 与
workspace 项目都从 workspace 根目录解析资源，不读取或修改 `[package.metadata.bundle]`：

```toml
[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "示例桌面应用"
publish_target = "rustfs"
object_prefix = "products"

[apps.desktop.branding]
application_logo = "assets/logos/desktop/logo-icon-128.png"
icon_source = "assets/logos/desktop/logo-icon-source.png"
managed = true

[apps.desktop.release]
default_channel = "nightly"
version = "${CARGO_PKG_VERSION}"
build_number = "${BUILD_DATETIME}"
minimum_supported_version = "0.1.0"

[apps.desktop.release.channels.nightly]
runtime_config = "config/desktop-nightly.toml"

[apps.desktop.release.channels.beta]
runtime_config = "config/desktop-beta.toml"

[apps.desktop.release.channels.stable]
runtime_config = "config/desktop-stable.toml"

[apps.desktop.updater]
enabled = true
channels = ["nightly", "beta", "stable"]

[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "ad_hoc"
notarize = false

[apps.desktop.platforms.windows]
icon = "assets/logos/desktop/logo-icon.ico"

[apps.desktop.platforms.linux]
icons = ["assets/logos/desktop/logo-icon-16.png", "assets/logos/desktop/logo-icon-128.png"]
```

路径必须是 workspace 内的相对路径。当前生产打包链路实现 macOS `.app`、DMG 与 ICNS；
Windows/Linux 图标已进入配置、初始化和生成模型，但尚未实现完整安装包与自动安装更新。

`release` 根字段是所有 channel 的默认值，各 channel 可以覆盖 version、build number、最低
支持版本和 runtime config。`default_channel` 必须存在，channel 必须同时属于启用的 updater
channels。新式 `release.channels` 不能与旧 `release.channel` 或静态 `updater.feed_url` 混用；
旧单通道配置仍按原行为兼容。多通道 updater feed 根据 publish target、public base URL、
object prefix、app key 和 channel 自动生成。
