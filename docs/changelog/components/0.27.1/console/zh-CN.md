---
title: Console 0.27.1
---

- Windows 打包现在为主程序和 updater sidecar 显式选择 Rust CRT 入口点，避免缺少
  `WinMain` 导致 iMES 等 GPUI 应用在 release 链接阶段失败。
- 本版本不改变窗口外观、导航、安装器选项或自动更新交互。
