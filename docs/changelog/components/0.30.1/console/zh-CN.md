---
title: Console 0.30.1
---

- 修复已安装官方 Inno Setup 6.7.3 时 CLI 仍重复要求安装的问题；版本检查现在读取
  `ISCC.exe` 的 PE 固定文件版本资源。
- 固定版本继续保持 6.7.3，不升级到 Inno Setup 7，也不需要修改已迁移的 `nexora.toml`。
