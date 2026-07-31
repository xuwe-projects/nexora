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

环境变量以 `__` 表示嵌套字段。显式路径优先；未传路径时根据当前 package 名查找
`config/<package>.toml`。敏感值应由环境变量或密钥系统注入。

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
