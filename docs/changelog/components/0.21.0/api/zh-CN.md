## 多应用品牌与公共更新入口

- `nexora.toml` 的 app 注册新增品牌资源和三平台图标配置；macOS build 会在打包前校验并写入
  所选 app 的 ICNS，不再依赖或修改 `[package.metadata.bundle]`。
- 新增 `nexora::desktop::install_updater`、公共 `CheckForUpdates` Action 与可复用按钮；只有显式
  安装 updater 的应用才会显示默认登录页、账户菜单、设置和 macOS 原生菜单入口。
- 启动检查只提示可用版本，手动或后台流程都必须在用户确认后才下载。
