## 桌面 API 传输安全

- `ApiSettings` 新增 `allow_insecure_http`，serde 缺省为 `false`；HTTPS 与 loopback HTTP 保持
  可用，其他 HTTP 必须在调用方接受 Bearer Token 明文传输风险后显式启用。
- 使用 Rust 结构体字面量构造 `ApiSettings` 的下游需要补充该字段；配置文件可依赖安全默认值。
