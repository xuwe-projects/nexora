## 桌面标题栏动作

- 桌面应用可以通过 `PanelHeaderAction` 和 `install_panel_header_actions` 在所有业务页面的
  `PanelHeader` 右侧安装应用级入口，动作会显示在当前标签页置顶按钮之前。
- `docs/desktop/application.md` 补充了 `Application::initialize` 中的接入示例；升级 iMES 等
  下游应用时可直接按该示例从 vendored 差异迁移到 `v0.17.0` tag。
