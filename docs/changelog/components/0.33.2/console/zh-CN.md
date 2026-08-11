# Console 0.33.2

- 修复 Windows/MSVC 下 `configuration` 缺少 `windows/std` 导致的编译失败。
- 修复 Windows 将锁竞争返回为 `ERROR_LOCK_VIOLATION` 时桌面二次启动无法激活已有主进程的问题。
- Windows 配置原子替换继续保留底层错误来源，并保持保存、更新、锁和临时文件语义不变。
- 公开 API、配置格式、桌面组件、导航和 Updater 协议保持不变。
