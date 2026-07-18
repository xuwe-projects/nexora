---
title: Rust 服务端 API 完整参考
order: 3
---

# Rust 服务端 API 完整参考

本页描述启用 `nexora` 的 `server,derive` features 后，宿主可以从 `nexora::server` 使用的
公开 Rust 接口。HTTP 路由和 JSON 契约见 [HTTP API 完整参考](./http-api)。

## 模块职责

`nexora::server` 是宿主 composition root 的公共入口。它负责暴露框架迁移、Account/OIDC
装配、可组合 Router、认证授权 extractor、Setup 扩展点和可信宿主写入函数，但不负责：

- 创建第二个 `PgPool`；
- 绑定 TCP 端口或调用 `axum::serve`；
- 自动执行迁移；
- 替可信宿主函数执行当前请求授权；
- 管理 TLS、日志、限流、超时或优雅关闭。

## `Server` 生命周期

### `Server::new`

```rust
pub const fn new() -> Server
```

创建空组合器。无 I/O，不读取配置，不持有连接池、State 或监听器。

### `Server::initialize`

```rust
pub async fn initialize<S>(
    &mut self,
    settings: &S,
    pool: &PgPool,
    setup_secret: &str,
) -> Result<(), ServerError>
```

| 参数 | 说明 |
| --- | --- |
| `settings` | 通过 `#[nexora(account_server)]` 提供 `AccountSettings` 的强类型根配置 |
| `pool` | 宿主创建并在全应用共享的唯一 PostgreSQL 连接池 |
| `setup_secret` | 首次初始化页面使用的秘密；调用方负责从安全配置读取 |

该方法执行 OIDC discovery、创建 token verifier、原子绑定/核对部署 issuer、创建 ZITADEL
目录客户端、判断初始化状态，并在已初始化时幂等同步系统角色。它不执行 SQLx 迁移。

返回 `ServerError`：

| 变体 | 原因 |
| --- | --- |
| `AccountInitialization` | OIDC discovery、JWKS 或 issuer 绑定失败 |
| `Account` | 初始化状态或系统角色读取失败 |
| `Directory` | ZITADEL 用户目录或 Project 角色同步失败 |

### `Server::routers`

```rust
pub fn routers<S>(&self) -> Router<S>
where
    S: Clone + Send + Sync + 'static
```

成功初始化后返回适配宿主 State `S` 的 Setup 与 Account Router；初始化前返回空 Router。
调用是同步且无 I/O。返回路由清单见 [HTTP API 完整参考](./http-api)。

### `Server::account`

```rust
pub fn account(&self) -> Option<Account>
```

初始化前返回 `None`，初始化成功后返回可廉价克隆的 `Account` facade。克隆只复用同一个
`PgPool` 和 token verifier。宿主可把它加入 `AppState`：

```rust
use axum::extract::FromRef;
use nexora::server::Account;

#[derive(Clone)]
struct AppState {
    account: Account,
}

impl FromRef<AppState> for Account {
    fn from_ref(state: &AppState) -> Self {
        state.account.clone()
    }
}
```

### `Server::setup_url`

```rust
pub fn setup_url(&self, address: SocketAddr) -> Option<String>
```

系统未初始化时返回 `/setup` URL，否则返回 `None`。如果监听地址是 `0.0.0.0` 或 `[::]`，
URL 使用 `127.0.0.1`，避免输出不可直接访问的通配地址。该方法只格式化地址。

## 迁移接口

```rust
pub fn migrations() -> Vec<sqlx::migrate::Migration>
```

返回 Nexora 全部嵌入式 SQLx 迁移的元数据副本，无数据库 I/O。宿主必须先与应用迁移合并并
拒绝版本号冲突，再构造唯一 `Migrator`：

```rust
let framework = nexora::server::migrations();
let migrations = merge_and_reject_duplicate_versions(framework, application_migrations)?;
sqlx::migrate::Migrator::with_migrations(migrations)
    .run(&pool)
    .await?;
```

不要分别运行框架 Migrator 与应用 Migrator。

## 服务端配置与依赖装配

### `AccountSettings`

`AccountSettings` 包含 `oidc: AccountOidcSettings`。OIDC 字段如下：

| 字段 | 说明 |
| --- | --- |
| `issuer_url` | Provider 规范 issuer；生产必须 HTTPS，loopback 开发可用 HTTP |
| `audience` | access token `aud` 必须包含的资源服务标识 |
| `organization_id` | `POST /users` 创建人类用户的 ZITADEL Organization ID |
| `project_id` | ZITADEL 中承载系统角色的 Project ID，不是 API Application Client ID |
| `personal_access_token` | 调用 ZITADEL UserService/ProjectService 的服务账号 PAT；Debug 会脱敏 |

### `dependencies`

```rust
pub async fn dependencies<S>(
    pool: PgPool,
    settings: &S,
) -> Result<AccountDependencies, AccountServerInitializationError>
```

用于不采用 `Server` 组合器的高级宿主。它发现 OIDC Provider、创建 verifier，并绑定部署
issuer；不执行迁移、创建 Router 或启动服务。`PgPool` 是廉价克隆句柄，不应再包
`Arc<PgPool>`。

### `user_directory`

```rust
pub fn user_directory<S>(settings: &S) -> Result<ZitadelUserDirectory, DirectoryError>
```

根据标准配置创建 ZITADEL gRPC 用户目录与 Project 角色客户端。配置 URL、PAT、Organization ID、Project ID
或 TLS 失败时返回 `DirectoryError`。

### `ZitadelUserDirectory`

该类型也从 `nexora::server` 公开，供自定义初始化流程复用：

| 方法 | 入参 | 出参 | 说明 |
| --- | --- | --- | --- |
| `new` | issuer、PAT、Organization ID、Project ID | `Result<Self, DirectoryError>` | 创建 TLS/loopback gRPC channel；PAT metadata 标记为 sensitive |
| `ensure_project_roles` | `&[SystemRole]` | `Result<(), DirectoryError>` | 逐项幂等创建 Project 角色；AlreadyExists 按成功处理 |
| `list_active_human_users` | 无 | `Result<Vec<DirectoryUser>, DirectoryError>` | 分页读取启用的人类用户，最多 10,000 项，按名称/用户名/ID 排序 |
| `active_human_user` | identity ID | `Result<Option<DirectoryUser>, DirectoryError>` | 按 ID 二次核对启用人类用户 |

该类型同时实现 `IdentityDirectory`：`identity` 读取当前用户资料；`create_human_identity`
使用 `CreateHumanIdentity { username, given_name, family_name, email, display_name }` 调用
UserService v2 创建人类用户并请求默认邮箱验证；`delete_identity` 用于本地事务失败后的补偿。
错误稳定映射为 `Conflict`、`NotFound` 或 `Unavailable`，不会把 PAT 或 Provider 内部响应返回
给 HTTP 客户端。

`DirectoryUser` 包含 `identity_id`、`username`、`display_name`、可选 `email` 和
`avatar_url`；`into_external_identity()` 会保留这些可信目录资料。所有 gRPC 请求使用 15 秒
超时。`DirectoryError` 区分无效配置、TLS、UserService 请求、ProjectService 角色请求、
非法 UTF-8 和目录安全上限；PAT 不会进入 Debug 或错误正文。

## `Account` facade

### 构造和部署绑定

| 方法 | 输入 | 输出 | 主要错误/副作用 |
| --- | --- | --- | --- |
| `Account::new` | `AccountDependencies` | `Account` | 无 I/O；保存 pool 与 verifier |
| `Account::bind_identity_issuer` | `&PgPool`, issuer | `IdentityIssuerBindingOutcome` | 首次原子绑定；不同 issuer 永久拒绝 |
| `initialization_status` | 无 | `Required` 或 `Completed { super_admin }` | 读取数据库 |
| `is_system_initialized` | 无 | `bool` | 读取数据库 |
| `system_roles` | 无 | `Vec<SystemRole>` | 返回需同步到身份 Project 的系统角色 |
| `initialize` | `AccountInitialization` | `Initialized` 或 `AlreadyInitialized` | 在事务中设置唯一超级管理员 |

### 认证与授权

```rust
pub async fn authenticate(&self, access_token: &str)
    -> Result<AccessProfile, AccountError>;

pub async fn authorize(
    &self,
    access_token: &str,
    permission: PermissionKey,
) -> Result<AccessProfile, AccountError>;
```

`authenticate` 验证 token 并检查本地用户是否存在、是否停用，同时同步可信身份资料；
`authorize` 再检查权限。二者都可能返回 token/Provider、issuer、本地账号、权限或数据库错误。

### 查询与写入

| 方法 | 输入 | 输出 | 语义 |
| --- | --- | --- | --- |
| `register_permissions` | `&[PermissionDefinition]` | `Vec<Permission>` | 按 key 幂等创建或更新，并在同一事务中授予系统管理员 |
| `permissions` | 无 | `Vec<Permission>` | 完整权限目录 |
| `roles` | 无 | `Vec<Role>` | 全部角色及直接权限 |
| `role` | `role_id: i64` | `Role` | 不存在返回 `NotFound` |
| `create_role` | key、name、description、permission IDs | `Role` | 同事务创建自定义角色与权限关联 |
| `update_role` | role ID、可选 name、三态 description | `Role` | 系统角色不可修改 |
| `delete_role` | role ID | `()` | 系统角色或被引用角色不可删除 |
| `replace_role_permissions` | role ID、完整 permission IDs | `Role` | 原子替换；空数组清空 |
| `users` | page、page_size | `Page<User>` | page 从 1 开始，size 限制到 1..=100 |
| `user_access` | user ID | `AccessProfile` | 用户、直接角色与合并权限 |
| `refresh_user_from_directory` | identity ID | `AccessProfile` | 同步目录用户名、邮箱、展示名、头像与最近登录时间，不创建陌生用户 |
| `update_user_status` | user ID、`UserStatus` | `User` | 超级管理员和最后管理员受保护 |
| `replace_user_roles` | user ID、完整 role IDs、granted_by | `AccessProfile` | 原子替换并保留 `member` |
| `provision_user` | `ExternalIdentity` | `User` | 不验证身份来源，不自动授权角色 |
| `provision_user_with_roles` | identity、role IDs、granted_by | `User` | 用户与初始角色在同一事务中创建 |
| `create_managed_user_with_roles` | `CreateHumanIdentity`、role IDs、granted_by | `User` | 先创建 Provider 用户，再事务绑定本地账号；失败时尽力删除 Provider 用户 |
| `routers::<S>` | 无 | `Router<S>` | 注入 Account 私有 State，无 I/O |

`AccountDependencies.identity_directory` 控制上述目录同步和托管创建；`Server::initialize`
会自动注入 ZITADEL 实现。直接构造 Account 且设为 `None` 时，`refresh_user_from_directory`
保留本地资料，而托管创建明确返回目录不可用，不会回退为接收裸 identity ID。

这些 facade 写方法不执行“当前请求是否有权调用”的检查；Account 自带 HTTP handler 会用
`Authorized<P>` 执行权限门禁，宿主直接调用时必须先做等价授权。

## Pool-first 可信宿主函数

这些自由函数适合启动同步、后台任务或已经授权的宿主 handler，并且都复用调用方的连接池。

### `create_permissions`

```rust
pub async fn create_permissions(
    pool: &PgPool,
    definitions: &[PermissionDefinition],
) -> Result<Vec<Permission>, AccountError>
```

最多 256 项；同批 key 不能重复。key 必须是 `resource:action`，两段各 2 至 64 位，小写
字母开头；name 为 1 至 100 个字符，description 最多 1000 个字符。相同 key 会更新元数据。

### `create_role`

```rust
pub async fn create_role(
    pool: &PgPool,
    key: &str,
    name: &str,
    description: Option<&str>,
    permission_ids: &[i64],
) -> Result<Role, AccountError>
```

创建自定义角色，最多 256 个权限 ID。角色和关联在同一事务中写入。

### `replace_role_permissions`

```rust
pub async fn replace_role_permissions(
    pool: &PgPool,
    role_id: i64,
    permission_ids: &[i64],
) -> Result<Role, AccountError>
```

替换完整集合；空数组清空。系统角色不可修改。

### `create_user`

```rust
pub async fn create_user(
    pool: &PgPool,
    identity: ExternalIdentity,
) -> Result<User, AccountError>
```

只接受调用方已确认的外部身份；不创建本地密码，不验证身份来源，不授予业务角色。

### `create_user_with_roles`

```rust
pub async fn create_user_with_roles(
    pool: &PgPool,
    identity: ExternalIdentity,
    role_ids: &[i64],
    granted_by: &str,
) -> Result<User, AccountError>
```

最多 64 个业务角色；自动补充 `member`。`granted_by` 必须是现有本地操作者 ID。用户和角色
关系在同一事务中写入，失败不留下部分数据。

### `replace_user_roles`

```rust
pub async fn replace_user_roles(
    pool: &PgPool,
    user_id: &str,
    role_ids: &[i64],
    granted_by: &str,
) -> Result<AccessProfile, AccountError>
```

替换普通用户完整业务角色集合并保留 `member`。不能修改超级管理员或移除最后一个管理员。

## 认证授权 extractors

### `AuthenticatedUser`

从 `Authorization: Bearer ...` 提取并返回已认证用户。要求 `Account: FromRef<AppState>`。

```rust
async fn profile(user: AuthenticatedUser) {
    let user_id = user.profile().user.id.as_str();
}
```

提供 `profile(&self) -> &AccessProfile` 与 `into_profile(self) -> AccessProfile`。

### `RequiredPermission` 与 `Authorized<P>`

```rust
struct ReadFactories;

impl RequiredPermission for ReadFactories {
    const KEY: PermissionKey = PermissionKey::from_static("factories:read");
}

async fn list_factories(auth: Authorized<ReadFactories>) {
    let actor = &auth.profile().user;
}
```

`PermissionKey::from_static` 只声明编译期键；应用仍必须通过 `create_permissions` 或
`register_permissions` 注册同名权限。超级管理员直接通过，普通用户依赖角色权限。

## 核心数据类型

### `ExternalIdentity`

| 字段 | 类型 | 约束 |
| --- | --- | --- |
| `identity_id` | `String` | trim 后非空，最多 255 字节；稳定绑定键 |
| `username` | `Option<String>` | trim 后非空，最多 200 个字符；可变元数据 |
| `email` | `Option<String>` | 最多 320 字节 |
| `display_name` | `String` | trim 后非空，最多 200 个字符 |
| `avatar_url` | `Option<String>` | 最多 2048 字节 |

### `CreateHumanIdentity` 与 `IdentityDirectory`

| 字段 | 类型 | 约束/用途 |
| --- | --- | --- |
| `username` | `String` | 1 至 200 个字符；ZITADEL Organization 内唯一登录名 |
| `given_name` | `String` | 1 至 200 个字符 |
| `family_name` | `String` | 1 至 200 个字符 |
| `email` | `String` | 合法主邮箱，最多 200 个字符 |
| `display_name` | `Option<String>` | 可选；最多 200 个字符 |

`IdentityDirectory` 是服务端目录端口，包含 `identity`、`create_human_identity` 与
`delete_identity` 三个异步方法；`IdentityDirectoryError` 稳定区分 `Conflict`、`NotFound`
和 `Unavailable`。应用可以注入其他 Provider 实现，默认 Server 注入 ZITADEL。

### 结果实体

- `User`：本地 ID、identity、username、展示资料、状态、超级管理员标记和 UTC 时间。
- `Permission`：数据库 ID、`PermissionKey`、名称和说明。
- `Role`：角色元数据、系统标记、直接权限与 UTC 时间。
- `AccessProfile`：`User`、直接 `roles` 和去重 `BTreeSet<PermissionKey>`。
- `UserStatus`：`Active` 或 `Suspended`。

`AccessProfile::allows(permission)` 对超级管理员始终返回 `true`，普通用户检查合并权限集合。

## Setup 扩展点

### 默认与自定义路由

```rust
pub fn setup_routes<S>(
    account: Account,
    directory: ZitadelUserDirectory,
    setup_secret: &str,
) -> Router<S>;

pub fn setup_routes_with<S, T>(
    account: Account,
    directory: ZitadelUserDirectory,
    setup_secret: &str,
    setup: T,
) -> Router<S>
where
    T: Setup;
```

`setup_routes` 使用 `DefaultSetup` HTML；`setup_routes_with` 允许替换请求 DTO 和表现层。

### `SetupUnlockRequest`

实现 `setup_secret(&self) -> &str`，把自定义解锁 DTO 映射到框架必须验证的 secret。

### `SetupCompletionRequest`

实现：

```rust
fn setup_token(&self) -> &str;
fn super_admin_identity_id(&self) -> &str;
```

### `Setup`

实现者声明 `UnlockRequest` 和 `CompletionRequest`，并提供五种响应：解锁页、用户选择页、
完成页、内部错误页、已关闭时的 404 页。框架仍强制执行：

1. setup secret 的 SHA-256 摘要常量时间比较；
2. 32 字节随机 token 与 15 分钟有效期；
3. 从 ZITADEL 重新读取所选启用人类用户；
4. 幂等同步系统角色；
5. 在 PostgreSQL 事务中设置唯一超级管理员；
6. 完成后清除 session，并永久关闭 Setup 路由。

自定义响应不能绕过这些不变量。

## 公开 re-export 清单

`nexora::server` 直接公开以下宿主稳定边界，应用无需依赖内部 crate 路径：

- 组合与配置：`Server`、`ServerError`、`AccountSettings`、`AccountOidcSettings`、
  `AccountServerInitializationError`、`migrations`、`dependencies`、`user_directory`；
- Account facade 与实体：`Account`、`AccountError`、`AccessProfile`、`ExternalIdentity`、
  `CreateHumanIdentity`、`IdentityDirectory`、`IdentityDirectoryError`、`User`、`Role`、
  `Permission`、`PermissionDefinition`、`PermissionKey`；
- 认证授权：`AuthenticatedUser`、`Authorized<P>`、`RequiredPermission`；
- 可信宿主函数：`create_permissions`、`create_role`、`create_user`、
  `create_user_with_roles`、`replace_role_permissions`、`replace_user_roles`；
- Setup 与目录：`DefaultSetup`、`DefaultSetupUnlockRequest`、
  `DefaultSetupCompletionRequest`、`Setup`、`SetupUnlockRequest`、`SetupCompletionRequest`、
  `setup_routes`、`setup_routes_with`、`DirectoryUser`、`DirectoryError`、
  `ZitadelUserDirectory`。

全部 HTTP 入参、出参、状态码和错误 envelope 见 [HTTP API 完整参考](./http-api) 与
[OpenAPI 3.1](../openapi.yaml)。
