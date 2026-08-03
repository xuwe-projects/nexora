---
title: API 0.21.2
---

- 修复 `install_updater` 健康重启时 `nexora::config::initialize(None)` 把 updater 内部参数误当
  配置文件路径的问题；新版本现在可以加载默认配置并完成 sidecar 健康确认。
