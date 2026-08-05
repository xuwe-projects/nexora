---
title: Console 0.27.0
---

- 正式安装的原生窗口标题默认使用经过校验的 `display_name`，应用显式标题保持最高优先级。
- 登录页与 Shell 登录门禁继续使用官方 `TitleBar`，并修正绝对定位标题栏的宽度和高度；Shell
  托管登录页不会渲染重复标题栏。
- 发布文件名改为 `<display_name>-<arch><suffix>`；升级 CI 和下载页时同步替换旧版本化文件名与
  `latest.*` 安装器别名。
