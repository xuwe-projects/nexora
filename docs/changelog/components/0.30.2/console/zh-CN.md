---
title: Console 0.30.2
---

- 修复 Windows 安装器定义包含空格时被 ISCC 误判为多个脚本文件的问题；安装器变量现在写入
  生成的 `installer.iss`，正式编译只传入一个脚本路径。
- CLI 固定携带简体中文 Inno Setup 消息文件，不再依赖本机 Inno 安装是否包含非官方翻译。
- 固定版本继续保持 Inno Setup 6.7.3，已迁移项目无需修改 `nexora.toml`。
