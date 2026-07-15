# Desktop Template

这是一个由 GPUI 桌面端、Axum 服务端和独立业务模块组成的 Rust workspace。

## 后台结构

```text
.
├── apps/server/                 # 进程启动、PgPool 创建和顶层路由组合
├── config/
│   └── example.server.toml      # 可提交的服务端配置示例
├── crates/
│   ├── api/                     # 通用 HTTP 错误、extractor 和中间件
│   ├── configuration/           # config-rs 分层加载
│   ├── contracts/               # 跨进程 JSON 契约
│   └── migrate/                 # 全局 SQLx 迁移程序、迁移和测试数据
└── modules/
    └── account/                 # 账号模块 State、Router、handler、实体和 store
```

服务端只创建一个 `PgPool`。`AccountState` 保存该连接池的廉价克隆句柄，账号模块先为自己的
`Router<AccountState>` 注入模块 State，再返回可与 `Router<AppState>` 合并的路由。只有
`apps/server` 在最外层调用一次 `.with_state(app_state)`。

依赖方向保持单向：

```text
apps/server ──> modules/account ──> crates/api + crates/contracts + SQLx
            └─> crates/configuration

crates/migrate ──> crates/configuration + SQLx
```

业务模块不依赖 `apps/server` 或服务端的 `AppState`。业务 SQL 只出现在模块的 `stores`
边界；表结构、索引、约束和必要基础数据统一位于 `crates/migrate/migrations`。

## 本地运行

先创建不会被 Git 跟踪的本地配置：

```bash
cp config/example.server.toml config/server.toml
```

从 workspace 根目录执行迁移和服务端：

```bash
cargo run -p migrate -- config/server.toml
cargo run -p server -- config/server.toml
```

服务端默认也会读取 `config/server.toml`，因此配置就位后可以省略路径。迁移程序与服务端都在
文件配置之后加载环境变量，嵌套字段使用双下划线，例如 `DATABASE__URL`。

## 新增业务模块

新模块使用单数 crate 名和初始化类型、复数资源路由与表名。模块至少包含
`entities`、`errors`、`handlers`、`routers`、`stores` 和模块 State；服务端只新增 workspace
依赖、构造模块并合并 `module.routers::<AppState>()`。数据库变更以全局唯一版本追加到
`crates/migrate/migrations/` 一级目录，测试数据按模块放入 `crates/migrate/seeds/<module>/`。

详细约定见 [modules/README.md](modules/README.md) 与
[crates/migrate/README.md](crates/migrate/README.md)。

## 质量检查

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p cli --bin xuwecli -- lint --workspace . --deny-warnings
```
