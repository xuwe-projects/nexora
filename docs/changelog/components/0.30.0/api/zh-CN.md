---
title: API 0.30.0
---

- 本版本迁移 Windows 首次安装打包链路，不改变 Axum API、Account 契约、SQLx schema 或服务端
  启动顺序。
- Windows 发布自动化应停止读取 windows_msi，只消费 windows_setup_exe 与
  windows_update_zip 及其 SHA-256。
