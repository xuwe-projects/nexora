# Config

这个目录用于放置模板项目的“运行时配置样例”和“环境变量样例”。新成员第一次打开项目时，
应该先看这里，而不是从源码里猜系统支持哪些配置。

## 文件约定

复制可提交的示例文件，创建被 Git 忽略的本地配置：

```bash
cp config/example.server.toml config/server.toml
```

服务端和迁移程序默认读取该文件，也可以把其他配置路径作为第一个参数传入：

```bash
cargo run -p migrate -- /path/to/server.toml
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
DATABASE__URL=postgres://xuwe:secret@postgres:5432/xuwe
DATABASE__MAX_CONNECTIONS=20
OIDC__ISSUER_URL=https://id.example.com
OIDC__AUDIENCE=api-application-client-id
OIDC__PERSONAL_ACCESS_TOKEN=zitadel-service-account-pat
OIDC__SUPER_ADMIN_SUBJECT=zitadel-human-user-id
```

`OIDC__SUPER_ADMIN_SUBJECT` 只在非交互首次绑定时必须设置；subject 两侧空白会在加载时去除。

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

数据库迁移与服务启动相互独立。部署时先通过 `migrate` 程序应用
`crates/migrate/migrations`，再启动服务端；服务端不会隐式修改 schema：

```bash
cargo run -p migrate -- config/server.toml
cargo run -p server -- config/server.toml
```

### 首次启动的内置超级管理员

数据库尚未绑定超级管理员时，服务会调用与 OIDC issuer 同域的 ZITADEL User v2 API，
只列出启用状态的人类用户，机器用户不会进入候选列表。PAT 配置在 `[oidc]` 下：

```toml
[oidc]
personal_access_token = "zitadel-service-account-pat"
```

推荐通过嵌套环境变量和部署平台的密钥管理系统注入：

```bash
OIDC__PERSONAL_ACCESS_TOKEN=zitadel-service-account-pat cargo run -p server
```

PAT 必须由 ZITADEL 服务账号创建。进入 Console 的 `Service Accounts`，创建或选择服务账号，
在 `Personal Access Tokens` 中创建令牌、设置有效期并立即复制；令牌只显示一次。服务账号还需
获得调用目标 ZITADEL API 所要求的管理员角色。服务要管理实例级全部资源时可以授予
`IAM_OWNER`，但最高权限 PAT 一旦泄露也会暴露整个实例，必须放在密钥管理系统中并制定轮换、
吊销与审计策略。

PAT 是运行期必填配置，不再是绑定完成即可移除的一次性引导密钥。首次管理员目录读取，以及
计划中把本地角色创建、修改等操作同步到 ZITADEL，都会复用 `[oidc]` 下这一身份提供方凭据。
终端首次启动会要求选择用户并输入 `BIND` 二次确认。绑定是不可替换的安全操作：该账号不能
删除、停用、更换 OIDC 身份或修改角色，并始终拥有权限表中的全部当前及未来权限。

容器、systemd 和 CI 等无终端部署必须显式指定 ZITADEL `userId`：

```bash
OIDC__PERSONAL_ACCESS_TOKEN=zitadel-service-account-pat \
OIDC__SUPER_ADMIN_SUBJECT=zitadel-human-user-id \
cargo run -p server -- config/server.toml
```

如果数据库已有超级管理员，`super_admin_subject` 不再参与启动；PAT 仍保留给运行期 ZITADEL
交互。`--check-config` 只验证配置，既不连接数据库，也不调用 ZITADEL。

### 普通管理员与 RBAC

服务端不再通过配置中的 subject 名单隐式授予普通管理员。所有新用户首次登录只自动获得
`member`；普通管理员和自定义角色必须由已经授权的账号通过 API 显式管理。系统还预置只读
`auditor` 和仅供内置账号使用的 `super-administrator`。四个系统角色均不可修改或删除。
Store 会在事务中保护最后一个启用管理员，避免通过停用或角色替换造成永久锁死；数据库触发器
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
- `updater/latest.example.json`：`xuwecli build` 生成的更新清单形状示例，不需要手写。

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
```

`desktop-build.env.example` 是供 shell、CI 和发布机构建桌面安装包时参考的 dotenv 风格文件，
其中还包含签名、公证和更新发布变量。`config-rs` 的文件源不会把这种 `KEY=value` 文件当作
TOML 读取，因此应用运行时只通过 `config/desktop.toml` 加载 OIDC 文件配置。

以后新增 `apps/<app>` 桌面程序时，可以复用 `crates/oidc`，应用层只需要负责自己的
环境变量装配、token 存储位置和 GPUI 状态接入。

Console 只把 refresh token 保存到系统安全凭据库：macOS 使用 Keychain，Windows 使用
Credential Manager，Linux 使用 Secret Service。access token、ID Token 和用户资料只保留在
当前进程内存中。旧版本生成的明文 `auth.toml` 会在 refresh token 成功迁移并回读校验后删除。

应用启动时会使用 refresh token 换取并验证新会话；运行期间会在 access token 到期前自动续期。
Provider 返回 `invalid_grant` 时会清除失效凭据并回到登录页，临时网络错误则保留凭据并延迟重试。

## 更新发布文件

`xuwecli build` 会在 `dist/` 下生成自动更新需要的 `.app.zip`、同名 `.sha256`、
`latest.json` 和 `notes/...md` 更新日志副本。后续 `xuwecli publish` 接入 OSS 后，应先上传
安装包、校验文件和更新日志，最后上传 `latest.json`。

本机 macOS 可以一次构建多个 macOS 架构：

```bash
xuwecli build --targets macos --bundle-version 12
```

该命令会顺序构建 Apple Silicon 与 Intel macOS 产物，并把两个 `.app.zip` 写入同一个
`latest.json.artifacts`。如果只构建当前机器架构，使用默认值或显式传入：

```bash
xuwecli build --targets current
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
