# Console 0.34.0

- 默认使用单进程多窗口，主窗口、额外 Shell、Settings 和注册 Window 共享应用级状态与缓存。
- 修复单独关闭窗口后立即退出时，旧窗口会话仍在下次启动恢复的问题。
- 需要进程隔离的应用仍可显式使用 `.subprocess_windows(true)`，多窗口与恢复能力保持可用。
- 不修改 `workspace.toml` schema、HTTP API、数据库迁移或 Updater 协议。
