# Nexora Unreleased

- 桌面应用现在可以通过 `ApplicationOptions::unauthenticated_window("window-id")` 显式登记
  Account 未登录状态可打开的额外独立 Window。`settings` 继续默认放行；未知、非 Window 或
  重复 ID 会在启动前校验失败。
