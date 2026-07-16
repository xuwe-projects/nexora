# Config

这个目录用于放置模板项目的“运行时配置样例”和“环境变量样例”。新成员第一次打开项目时，
应该先看这里，而不是从源码里猜系统支持哪些配置。

## 文件约定

复制可提交的示例文件，创建被 Git 忽略的本地配置：

```bash
cp config/example.server.toml config/server.toml
```

服务端和迁移程序默认读取该文件，也可以把其他配置路径作为位置参数传入。首次安装空数据库
必须显式添加 `--initialize-empty-database`：

```bash
cargo run -p migrate -- --initialize-empty-database /path/to/server.toml
cargo run -p server -- /path/to/server.toml
```

配置优先级为：

```text
代码默认值 < 指定 TOML 文件 < 环境变量
```

`config/` 下除 `README.md` 和名称中带 `example` 的示例外均被递归忽略。真实数据库密码、PAT
和部署地址只能放在本地配置或密钥管理系统注入的环境变量中。

环境变量不带项目名前缀，嵌套字段使用双下划线：

```bash
SERVER__HOST=0.0.0.0
SERVER__PORT=8080
DATABASE__URL=postgres://nexora:secret@postgres:5432/nexora
DATABASE__MAX_CONNECTIONS=20
SETUP__SECRET=long-random-setup-secret
OIDC__ISSUER_URL=https://id.example.com
OIDC__AUDIENCE=api-application-client-id
OIDC__PROJECT_ID=zitadel-project-id
OIDC__PERSONAL_ACCESS_TOKEN=zitadel-service-account-pat
```

只验证配置文件和环境变量而不连接外部服务时，使用：

```bash
cargo run -p server -- --check-config config/server.toml
```

## 服务端认证与 RBAC

服务端把认证与业务授权拆成两个边界：OIDC Provider 负责登录和签发 access token；本地
PostgreSQL 保存用户、角色、权限及关联关系。API 只接受 `Authorization: Bearer <token>`，
并校验 JWT 签名、`iss`、`aud`、`exp`、可选的 `nbf` 和 `sub`。生产 OIDC issuer 必须使用
HTTPS，仅 `localhost`、loopback IPv4/IPv6 开发地址允许 HTTP。Provider 必须签发可通过 JWKS
验证的 JWT access token；当前实现不接受 opaque token。

数据库迁移与服务启动相互独立。首次安装使用显式空库初始化参数；后续部署只运行普通向前
升级命令。普通升级遇到空库、缺失迁移历史或核心表丢失会失败，不会自动重建 schema：

```bash
cargo run -p migrate -- --initialize-empty-database config/server.toml # 仅首次安装
cargo run -p migrate -- config/server.toml
cargo run -p server -- config/server.toml
```

### 一次性系统初始化与超级管理员

数据库的 `account.system_initialization` 单例记录用于判断系统是否已完成初始化。
系统未初始化时，服务端会记录 `/setup` 访问地址。先输入 `[setup].secret`，再由
服务端使用 PAT 通过 ZITADEL UserService v2 gRPC 读取启用状态的人类用户；服务账户不会进入
候选列表。提交所选用户后，服务端再通过 ProjectService v2 gRPC 把全部本地系统角色创建到
配置的 Project。所有调用使用 gRPC 官方 Rust `grpc` 库；这些 RPC 已受 ZITADEL 支持，因此不
提供 REST 回退。

```toml
[setup]
secret = "long-random-setup-secret"

[oidc]
project_id = "zitadel-project-id"
personal_access_token = "zitadel-service-account-pat"
```

推荐通过嵌套环境变量和部署平台的密钥管理系统注入：

```bash
SETUP__SECRET=long-random-setup-secret \
OIDC__PROJECT_ID=zitadel-project-id \
OIDC__PERSONAL_ACCESS_TOKEN=zitadel-service-account-pat \
cargo run -p server
```

`project_id` 必须填写承载本系统角色的 ZITADEL Project ID，不能用 API Application Client ID
替代。PAT 必须由 ZITADEL 服务账号创建。进入 Console 的 `Service Accounts`，创建或选择服务账号，
在 `Personal Access Tokens` 中创建令牌、设置有效期并立即复制；令牌只显示一次。服务账号还需
具备 `project.role.write`，并能读取 setup 候选用户。服务要管理实例级全部资源时可以授予
`IAM_OWNER`，但最高权限 PAT 一旦泄露也会暴露整个实例，必须放在密钥管理系统中并制定轮换、
吊销与审计策略。

PAT 是运行期必填配置，不是 setup secret。初始化用户目录读取与 Project 角色创建都会使用
`[oidc]` 下这一凭据。
setup secret 只用于换取内存中 15 分钟有效的一次性页面令牌，不会写入 URL、数据库或日志。
setup 会先幂等确保 `admin`、`auditor`、`member` 等全部本地系统角色存在于 Project；已存在
的角色键视为成功。全部角色成功后，初始化事务才会把所选目录用户写入
`account.users.identity_id`，清空其角色、标记为超级管理员并完成初始化状态。任一角色 gRPC
调用失败时系统保持未初始化，可修复权限或网络后重试。完成后 `/setup` 的 GET 和 POST 均永久
返回 404。

升级前已经完成初始化的实例不会重新开放 `/setup`。服务启动时会执行同一套幂等检查，补齐
缺失的系统角色；如果 ProjectService gRPC 调用失败，服务拒绝以角色目录不完整的状态启动，并
在错误链中记录 Project ID、角色键、gRPC code 和 message。

超级管理员是普通用户记录上的系统级标记，不挂载任何角色或权限。授权边界检测到该标记后
直接放行，因此新增未知权限也不需要补授。该用户不能停用、删除、替换 identity ID 或挂载角色。
`--check-config` 只验证配置，既不连接数据库，也不调用认证授权服务。

### 普通管理员与 RBAC

所有新用户首次登录只自动获得 `member`；普通管理员和自定义角色必须由已经授权的账号通过 API
显式管理。普通管理员本质上是关联内置 `admin` 角色的普通用户，仍然完整执行 RBAC 权限校验。
`admin`、`member` 和 `auditor` 三个系统角色均不可修改或删除。
账号数据访问函数会在事务中保护最后一个启用管理员，避免通过停用或角色替换造成永久锁死；数据库触发器
同时保护超级管理员，防止绕过应用直接破坏不变量。

预置权限如下：

| 权限 | 用途 |
| --- | --- |
| `users:read` | 查看用户和用户授权详情 |
| `users:roles.write` | 替换用户角色 |
| `users:status.write` | 启用或停用用户 |
| `roles:read` | 查看角色及角色权限 |
| `roles:write` | 管理自定义角色及其权限 |
| `permissions:read` | 查看权限目录 |

REST 契约位于 [`docs/openapi.yaml`](../docs/openapi.yaml)。除 `/health` 外均需要 Bearer token；
认证失败返回 `401`，权限不足返回 `403`，字段校验失败返回 `422`，资源冲突返回 `409`。
数据库集成测试默认不要求本机 PostgreSQL；可用测试实例准备好后运行：

```bash
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/postgres \
  cargo test -p account --features database-tests
```

## 当前样例

- `example.server.toml`：服务端 TOML 配置示例。
- `server.env.example`：服务端运行时环境变量示例。
- `desktop.toml.example`：Console 桌面应用本地运行配置样例。
- `desktop-build.env.example`：桌面端构建、签名和更新相关环境变量示例。
- `updater/latest.example.json`：`nexora build` 生成的更新清单形状示例，不需要手写。

## 桌面端 OIDC 认证

桌面端认证使用标准 OIDC Authorization Code + PKCE 流程。ZITADEL 已部署时，只需要把
Console 配置成 Native/Public Application，并允许下面的 loopback redirect URI：

```text
http://127.0.0.1:0/auth/callback
```

`OIDC_REDIRECT_URI` 是回调 URI 模板。端口为 `0` 时，应用会先绑定本地 listener，再将
`0` 替换为实际端口写入 `authorize` 和 `token` 请求；ZITADEL 需要将这个 Native loopback
redirect URI 视为同一路径上的动态端口。若实例返回 redirect URI 不匹配错误，说明该实例的
redirect 规则没有允许动态端口，应改用该实例允许的固定端口 URI。

本地开发可以复制运行时配置样例：

```bash
cp config/desktop.toml.example config/desktop.toml
```

然后修改 `config/desktop.toml` 中的 `[oidc]` 配置。该文件已被 Git 忽略。应用也读取通用
OIDC 环境变量，不使用 Provider 名称前缀；环境变量的优先级高于 `desktop.toml`：

```bash
OIDC_ISSUER_URL=https://id.example.com
OIDC_CLIENT_ID=console-native-client-id
OIDC_SCOPES="openid profile email offline_access"
OIDC_REDIRECT_URI=http://127.0.0.1:0/auth/callback
API_BASE_URL=http://127.0.0.1:3000
```

`API_BASE_URL` 必须指向服务端根地址。Console 完成 OIDC Authorization Code + PKCE 后不会立即
进入工作区，而是携带 access token 请求 `GET /me`。只有服务端确认 `account.users` 中已存在
对应 `identity_id` 且用户未停用时才保存 refresh token 并进入已登录状态；未开通账号返回
`403 account_not_registered`，Console 会保持未登录并清理旧凭据。应用启动恢复会话和自动续期
同样重新调用 `/me`，不会仅凭本地 refresh token 绕过账号门禁。

`desktop-build.env.example` 是供 shell、CI 和发布机构建桌面安装包时参考的 dotenv 风格文件，
其中还包含签名、公证和更新发布变量。`config-rs` 的文件源不会把这种 `KEY=value` 文件当作
TOML 读取，因此应用运行时只通过 `config/desktop.toml` 加载 OIDC 文件配置。

以后新增桌面程序示例时，可以复用 `crates/oidc`，应用层只需要负责自己的
环境变量装配、token 存储位置和 GPUI 状态接入。

Console 只把 refresh token 保存到系统安全凭据库：macOS 使用 Keychain，Windows 使用
Credential Manager，Linux 使用 Secret Service。access token、ID Token 和用户资料只保留在
当前进程内存中。旧版本生成的明文 `auth.toml` 会在 refresh token 成功迁移并回读校验后删除。

应用启动时会使用 refresh token 换取并验证新会话；运行期间会在 access token 到期前自动续期。
Provider 返回 `invalid_grant` 时会清除失效凭据并回到登录页，临时网络错误则保留凭据并延迟重试。

## 更新发布文件

`nexora build` 会在 `dist/` 下生成自动更新需要的 `.app.zip`、同名 `.sha256`、
`latest.json` 和 `notes/...md` 更新日志副本。后续 `nexora publish` 接入 OSS 后，应先上传
安装包、校验文件和更新日志，最后上传 `latest.json`。

本机 macOS 可以一次构建多个 macOS 架构：

```bash
nexora build --targets macos --bundle-version 12
```

该命令会顺序构建 Apple Silicon 与 Intel macOS 产物，并把两个 `.app.zip` 写入同一个
`latest.json.artifacts`。如果只构建当前机器架构，使用默认值或显式传入：

```bash
nexora build --targets current
```

Windows、Linux 和 macOS 完整发布矩阵不应依赖单个 Docker 容器完成。推荐做法是使用 CI 或
远程 runner 矩阵：

- macOS runner：生成 `.dmg`、`.app.zip`、签名、公证。
- Windows runner：生成 `.exe` 或 `.msi`，完成 Windows 签名。
- Linux runner：生成 `.tar.gz`、`.deb`、`.rpm` 或 AppImage。

各 runner 上传自己的产物元数据后，由发布步骤合并生成最终 `latest.json`，并确保最后上传它。

仓库根目录提供了一个普通 Pipeline 版本的 `Jenkinsfile`，默认只启用 `macos-arm64` agent。
你可以先在 Jenkins 中创建 Pipeline Job 指向该文件试跑；Windows stage 通过 `BUILD_WINDOWS`
参数控制，等 Windows 打包能力补齐后再打开。

## 不放在这里的配置

桌面用户偏好不放在仓库 `config/` 目录中，例如主题、颜色模式、最近打开记录等。这些属于用户
本机状态，会由桌面程序保存到操作系统标准配置目录。
