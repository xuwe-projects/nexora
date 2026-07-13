## 新增

- 建立 API 服务应用入口和 workspace 基础结构。
- 明确非 SSR API 的 REST 资源建模与接口设计规范。

## 工程规范

- HTTP 服务统一使用 Axum 和 Tokio。
- 请求参数优先使用 Axum Extractor 提取。
- 数据库访问和迁移分别统一使用 SQLx 与 SQLx CLI。
