---
title: API 0.28.0
---

- `nexora publish` 的 Access Key、Secret Key 与 Session Token 现在分别按 channel 专用、
  Nexora 通用和 AWS 通用变量回退，旧 `credential_env_prefix` 配置已删除。
- `nexora build` 继续支持 `${CARGO_PKG_VERSION}` 与 `${BUILD_DATETIME}`，并在交互式终端中
  自动准备 macOS/Windows 打包依赖；更新 manifest 与远程对象布局没有变化。
