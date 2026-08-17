---
title: API 0.38.1
---

- introspection Client ID/Secret 改为可选配置；缺失、不完整或无效时安全降级为 JWT-only。
- 有效 introspection 凭据同时支持 JWT 与 PAT/opaque token，临时故障恢复后无需重启即可自动重试。
- PAT 创建前探测 introspection，创建后校验实际 token 与服务账号 subject；无法验证时撤销且不交付。
