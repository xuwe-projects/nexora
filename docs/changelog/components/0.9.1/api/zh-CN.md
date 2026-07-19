## API 契约保持不变

- 本版本只修复桌面主窗口顶部标签栏的 `Segmented` 视觉连续性，以及默认用户管理页激活时的
  GPUI Entity 数据流问题；不改变 HTTP API、OpenAPI 路径、请求字段或响应结构。
- Account、ZITADEL 集成、数据库 schema 与迁移历史均未变化。
- 下游严格 DTO 客户端不需要为本版本调整 API 契约。
