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
# introspection_client_id = "nexora-resource-server-client-id"
# introspection_client_secret = "replace-through-secret-injection"
```

`organization_id` 用于 UserService v2 创建人类用户；`project_id` 用于同步系统角色，两者职责
不同。PAT 必须属于有权管理目标 Organization/Project 的服务账号，生产环境应通过
`OIDC__PERSONAL_ACCESS_TOKEN` 或密钥系统注入，不要提交真实值。
`introspection_client_id`/`introspection_client_secret` 是可选的 ZITADEL API resource server
HTTP Basic 凭据，只用于实时验证 PAT/opaque Bearer Token。两项都省略、只配置一项或凭据被
Provider 拒绝时服务降级为 JWT-only：JWT 仍可认证，opaque token 返回 401，并禁止创建新 PAT。
两项有效时同时支持 JWT 与 opaque token；Provider 临时不可用时 opaque/PAT 操作返回 503，恢复
后自动重试。Secret 应通过 `OIDC__INTROSPECTION_CLIENT_SECRET` 注入。

环境变量以 `__` 表示嵌套字段。配置文件依次选择：`initialize(Some(path))` 显式路径、首个
用户位置参数、正式 bundle 冻结配置、开发配置。正式包由当前可执行文件位置的合法
`nexora-release.json` 识别，macOS 读取 `.app/Contents/Resources/config/<package>.toml`，
Windows 读取主 EXE 同级 `config/<package>.toml`；一旦识别为正式包，配置缺失、不可读或
TOML 无效都会直接失败，不会回退源码仓库。普通 `cargo run`/`cargo test` 没有发布元数据时，
才查找当前目录和 package 清单目录祖先的 `config/<package>.toml`。环境变量仍在选中文件之后
覆盖同名字段；敏感值应由环境变量或密钥系统注入。

服务端 Setup secret 只在未初始化时有效。Nexora 框架迁移历史固定为
`nexora._sqlx_migrations`，应用迁移使用独立历史；两者借用同一个 `PgPool`，不需要也不允许
通过 `initialize_empty_database` 之类的人工布尔开关控制升级。

## 自动更新发布配置

桌面自动更新的构建和发布配置放在仓库根目录 `nexora.toml`，不会完整打包进客户端。`publish`
目标支持通用 S3 兼容对象存储与阿里云 OSS：

```toml
[publish.targets.rustfs]
provider = "s3"
endpoint = "http://127.0.0.1:9000"
bucket = "desktop-releases"
region = "us-east-1"
force_path_style = true
public_base_url = "http://127.0.0.1:9000/desktop-releases"
allow_insecure_http = true

[publish.targets.rustfs.channels.nightly]
endpoint = "http://192.168.0.250:9000"
public_base_url = "http://192.168.0.250:9000/desktop-releases"
allow_insecure_http = true

[publish.targets.rustfs.channels.stable]
provider = "aliyun_oss"
endpoint = "https://s3.oss-cn-shenzhen.aliyuncs.com"
bucket = "desktop-releases"
region = "cn-shenzhen"
force_path_style = false
public_base_url = "https://downloads.example.com"
allow_insecure_http = false
```

`endpoint` 是带签名的 S3 API 地址；`public_base_url` 是客户端匿名读取地址。本地 RustFS 使用
HTTP 时必须显式开启 `allow_insecure_http`。channel 表按字段覆盖基础 target，未出现的字段继承；
合并完成后会重新验证 provider、endpoint、公开 URL 与 HTTP 安全开关。`provider = "s3"`
对不可变对象发送 `If-None-Match: *`；阿里云 OSS 必须显式使用 `provider = "aliyun_oss"`，
此时不可变对象改用并签名 `x-oss-forbid-overwrite: true`。Nexora 不按 endpoint 域名推断 provider。

发布凭据不配置前缀，而是按当前 channel 为每个字段独立解析：先读取
`NEXORA_PUBLISH_<CHANNEL>_<FIELD>`，再读取 `NEXORA_PUBLISH_<FIELD>`，最后读取
`AWS_<FIELD>`。例如 beta 的 Access Key 优先使用 `NEXORA_PUBLISH_BETA_ACCESS_KEY_ID`；即使
Secret Key 来自 `NEXORA_PUBLISH_SECRET_ACCESS_KEY` 也有效。空值继续回退，Access Key 与
Secret Key 最终必须存在，Session Token 可选。`RUSTFS_*` 不再读取，凭据不得写入配置文件。
`object_prefix = ""` 表示 app_key 直接位于 bucket 根目录，不会生成空路径段或双斜杠。

每个桌面 app 必须在同一注册记录中声明稳定 app key、bundle identifier 和品牌资源。single 与
workspace 项目都从 workspace 根目录解析资源，不读取或修改 `[package.metadata.bundle]`：

```toml
[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "示例桌面应用"
publish_target = "rustfs"
object_prefix = ""

[apps.desktop.branding]
application_logo = "assets/logos/desktop/logo-icon-128.png"
icon_source = "assets/logos/desktop/logo-icon-source.png"
managed = true

[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "ad_hoc"
notarize = false

[apps.desktop.platforms.windows]
icon = "assets/logos/desktop/logo-icon.ico"
publisher = "Example Publisher"
signing = "none"
start_menu_shortcut_default = true
launch_after_install_default = true
minimum_windows_build = 15063

[apps.desktop.platforms.linux]
icons = ["assets/logos/desktop/logo-icon-16.png", "assets/logos/desktop/logo-icon-128.png"]
```

`app_id` 和 `display_name` 是 stable 基础值。CLI 对 beta 固定派生 `.beta` / ` Beta`，对 nightly
固定派生 `.nightly` / ` Nightly`，并拒绝基础值预含这些保留后缀。派生值隔离安装目录、进程
单例、updater 状态与日志、Account 凭据和用户偏好；远端对象目录仍使用稳定 app key 与 channel。

路径必须是 workspace 内的相对路径。`targets.required` 可省略，构建会使用 `rustc -vV` 返回的
本机 target；需要显式覆盖时使用 `nexora build --target <triple>`。当前生产打包链路实现 macOS
`.app`/DMG，以及 Windows x86_64/ARM64 的简体中文 Inno Setup EXE 和更新 ZIP。

平台配置不会授权 CLI 安装机器级工具。`nexora doctor` 与 build 预检只读检测；Windows Inno
Setup 接受 `>= 6.7.3, < 8.0.0`，新安装推荐 7.x。完整 Windows SDK、MSVC、Rust target、macOS
Xcode、cargo-bundle、create-dmg、证书和公证凭据的人工安装表见
[桌面自动更新](../desktop/updater.md#人工安装构建依赖)。缺少依赖时，用户执行诊断给出的命令后
重跑原命令；配置中不存在恢复 `doctor --fix` 或自动安装的开关。

Windows 的 `publisher` 在所有签名模式下都是安装器元数据，也是全新安装默认目录
`%LOCALAPPDATA%\Programs\<publisher>\<effective_display_name>` 的发布者目录名；因此必须是安全的 Windows
路径分量，不能包含 `/`、`\`、`:` 等非法字符、保留设备名或尾随点/空格。`signing = "none"` 仍保留
Ed25519 manifest、artifact SHA-256、ZIP 安全和 PE 架构校验，但不得同时配置
`signing_thumbprint`、`expected_publisher` 或 `timestamp_url`。`signing = "authenticode"` 时需要
证书 thumbprint（或 `WINDOWS_SIGN_CERTIFICATE_SHA1`）与 RFC 3161 `timestamp_url`；
`expected_publisher` 省略时回退到 `publisher`，更新器会同时验证主程序和 sidecar 的证书身份。
