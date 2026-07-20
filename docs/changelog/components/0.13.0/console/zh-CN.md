## iMES 权限可见性与权限蕴含保存

- CLI 安装示例更新到 `v0.13.0`。
- 桌面导航会按当前用户权限隐藏受限 Feature，并自动隐藏没有可见子项的 NavigationGroup。
- Feature 派生宏支持 `visible_permissions(any = ["employees:read"])`，手写元数据可使用同一可见性契约。
