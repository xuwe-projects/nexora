## 新增

- 提供默认 `/setup` 初始化流程，可通过 Setup secret 获取 ZITADEL 人类用户并选择系统超级管理员。
- 服务端 Account 能力开放用户、角色、权限定义与授权接口，应用可以注册自己的权限。
- 未完成初始化时输出可访问的 Setup URL，初始化完成后 Setup 页面永久返回 404。

## 架构调整

- 应用自行创建 PostgreSQL 连接池、Axum State、TCP Listener，并使用标准 `axum::serve` 启动。
- Nexora `Server` 只负责迁移、Account/ZITADEL 初始化以及提供可合并的 Router。
- 服务端配置拆分监听 IP 与端口，并补充 Project ID、PAT、Setup secret 等字段说明。
- 数据库升级完全依据 SQLx 迁移记录执行，已经应用的版本会自动跳过。
