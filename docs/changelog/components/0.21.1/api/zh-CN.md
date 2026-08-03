## updater sidecar 公共边界

- `nexora::desktop` 现在同时公开 `install_updater`、`run_sidecar_from_env_args` 与健康确认入口；
  应用和 sidecar 不再直接依赖内部 updater crate。
- app 注册、ICNS 写入、公共 `CheckForUpdates` Action、默认登录页、账户菜单、Settings 和 macOS
  原生菜单的现有行为保持不变。
- 更新接入文档补充所有 `nexora.toml` 字段、首次 DMG 安装、RustFS 发布、密钥轮换和生产
  Developer ID/notarization 流程。
