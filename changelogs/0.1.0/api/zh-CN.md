## 新增

- 建立 API 服务应用入口和 workspace 基础结构。
- 明确非 SSR API 的 REST 资源建模与接口设计规范。
- 使用 Axum/Tokio 启动真实 HTTP 服务，并支持请求 ID、访问日志、正文上限与优雅停止。
- 使用 OIDC discovery/JWKS 验证 Bearer JWT access token 的签名和标准声明。
- 使用 PostgreSQL/SQLx 持久化本地用户、角色、权限及关联关系。
- 提供用户状态、用户角色、自定义角色和角色权限管理 API，并保护最后一个启用管理员。
- 将 SQL 查询和事务集中在账号 store，将连接池和数据库健康检查集中在 API state。
- 把请求、响应、分页和错误模型抽到轻量 `contracts` crate，供服务端、后续 Rust SDK 和桌面端
  共享，避免客户端依赖 Axum 或 SQLx。
- SQLx 迁移保持全局扁平版本序列，并使用 `accounts_` 等领域前缀标明业务归属。
- 首次启动可从 ZITADEL 启用状态人类用户中选择唯一内置超级管理员，支持无终端部署显式指定
  subject；绑定完成后不再依赖目录 PAT。
- 超级管理员始终拥有全部当前及未来权限，并由服务规则与 PostgreSQL 约束共同禁止替换、
  停用、删除或修改角色。

## 工程规范

- HTTP 服务统一使用 Axum 和 Tokio。
- 请求参数优先使用 Axum Extractor 提取。
- 数据库访问和迁移分别统一使用 SQLx 与 SQLx CLI。
