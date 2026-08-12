# Console 0.35.0

- 应用固定为严格单进程、多原生窗口；重复启动只激活当前实例。
- 冷启动只创建主窗口并打开 `initial_path`，不再恢复历史标签、额外 Shell、Settings 或业务窗口。
- `workspace.toml` schema 2 保留主窗口几何、主题、表格布局和 Account 非秘密偏好，并清理旧窗口会话字段。
- Account 临时错误保留安全凭据与恢复资格，明确永久错误才清理恢复状态。
