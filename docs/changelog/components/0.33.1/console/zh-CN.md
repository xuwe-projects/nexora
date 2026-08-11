# Console 0.33.1

- 修复 `subprocess_windows(false)` 下新增 Shell、Settings 和注册 Window 的同进程创建、恢复、
  会话持久化与关闭清理。
- 多个同进程 Shell 现在按来源窗口或活动窗口分发导航，并在无法确定来源时回退到主 Shell。
- 默认受管子进程窗口模式、可见组件结构与 Updater 协议保持不变。
