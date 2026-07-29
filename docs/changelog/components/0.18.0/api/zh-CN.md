## 桌面 API 兼容性

- `CrudTableRow` 派生现在必须显式声明 `#[nexora(row_id)]`，`CrudTableDelegate` 的行选择改为
  受控 selected IDs 与类型化选择事件。
- 新增 `LabeledControl` 类型化字段 API，可注册到 `FormDialogState` 在提交前统一等待异步校验、
  执行声明式规则并聚焦首个无效字段。
- `FormDialog::panel_height_ratio` 与 `default_panel_height` 已迁移为
  `max_panel_height_ratio`；短内容自适应高度，长内容按 Panel 上限滚动。
