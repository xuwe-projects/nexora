---
title: API 0.35.0
---

- Account 桌面客户端现在区分连接、超时、响应读取、成功响应契约不兼容、结构化拒绝和非结构化响应。
- 结构化拒绝继续保留安全消息、稳定错误码与 request ID；日志与界面不会输出 endpoint、token 或原始正文。
- HTTP 路径、Account DTO、授权规则、SQLx migration 与 PostgreSQL 基线保持不变。
