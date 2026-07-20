## 迁移文件换行符修正

- 新增仓库级 `.gitattributes`，强制 SQL 文件使用 LF 换行。
- 框架内置迁移 SQL 语义保持不变，没有新增 HTTP 路由、数据库结构或服务端配置变更。
- Windows checkout 后迁移文件不再因为 CRLF 转换产生跨平台字节差异。
