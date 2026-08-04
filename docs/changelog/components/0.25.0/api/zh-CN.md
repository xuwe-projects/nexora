---
title: API 0.25.0
---

- 桌面 Account 支持系统安全存储、静默恢复、自动刷新、refresh token 撤销和账号选择登录；
  macOS 使用 Keychain，Windows 使用 Credential Manager，Linux 不持久化 token。
- `nexora build` 继续从 `${CARGO_PKG_VERSION}` 解析版本，并将 `${BUILD_DATETIME}` 调整为构建
  机器本机时区的 24 小时制时间；最终产物同时生成并发布标准 `.sha256` 旁车。
