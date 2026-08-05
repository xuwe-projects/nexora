---
title: API 0.27.1
---

- Windows 主程序与 updater sidecar 现在同时使用 `/SUBSYSTEM:WINDOWS` 与
  `/ENTRY:mainCRTStartup`，只定义 Rust `main` 的桌面应用不再触发 `WinMain` 链接错误。
- `nexora build` 仍使用 `${CARGO_PKG_VERSION}` 解析应用版本，并用
  `${BUILD_DATETIME}` 生成构建号；本版本没有修改发布配置或更新 manifest 契约。
