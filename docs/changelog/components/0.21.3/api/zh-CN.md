---
title: API 0.21.3
---

- 修复 `CheckForUpdates` Action 在 Footer 窗口事件中重入活动窗口而无法打开更新对话框的问题；
  `install_updater` 同时注册 macOS `Cmd+Shift+U` 与其他平台 `Ctrl+Shift+U` 默认快捷键。
