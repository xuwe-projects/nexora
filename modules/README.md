# Business Modules

`modules` 中的每个目录都是能独立管理 State、HTTP 和数据库行为的业务库 crate。业务模块依赖
共享基础 crate，但不得依赖 `apps/server` 或其中的具体 `AppState`。

## 标准形状

以单数 `account` 模块为例：

```text
modules/account/
├── Cargo.toml
├── src/
│   ├── account.rs              # crate 入口、Account 与 AccountState
│   ├── entities.rs
│   ├── entities/account.rs
│   ├── errors.rs
│   ├── handlers.rs
│   ├── handlers/accounts.rs
│   ├── routers.rs
│   ├── routers/accounts.rs
│   ├── stores.rs
│   └── stores/accounts.rs
└── tests/
```

复杂模块可以继续拆分子文件，但仍需保持以下边界：

- 模块 State 直接保存服务端共享的 `PgPool`，不要使用 `Arc<PgPool>`。
- handler 只处理 HTTP 输入输出；SQL 和事务只出现在 `stores` 边界。
- 模块先调用 `with_state::<S>(module_state)`，再返回仍等待宿主 State `S` 的 Router。
- 模块不初始化连接池、不读取服务端配置，也不依赖服务端 crate。
- HTTP 契约需要跨端复用时放在 `crates/contracts`，只在模块边界执行显式映射。
- 迁移统一追加到 `crates/migrate/migrations`，模块目录不保存建表 SQL。

模块入口的 State 组合形状如下：

```rust,ignore
pub fn routers<S>(self) -> Router<S> {
    routers::initialize().with_state::<S>(self.state)
}
```

`Router<S>` 表示“仍缺少 State `S`”。服务端合并模块时选择 `S = AppState`，并只在顶层注入
最终 State。
