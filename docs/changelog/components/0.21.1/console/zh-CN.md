## updater sidecar 接入修正

- 独立 updater sidecar 改为通过 `nexora::desktop` 公共 facade 启动，不再要求应用直接依赖内部
  updater crate。
- 公共更新弹窗、进度、通知、默认登录页、账户菜单、Settings 与 macOS 原生菜单入口保持同一
  app 级会话协调器。
