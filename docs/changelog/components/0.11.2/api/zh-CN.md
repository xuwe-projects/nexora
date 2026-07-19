## FormDialog API 修正

- 新增 `FormDialog::panel_height_ratio(ratio)`，用于调整常规表单相对当前 Feature Panel 的高度比例。
- 新增 `FormDialog::auto_height()`，用于少字段表单恢复内容自适应高度，同时保留默认 80% Panel 高度上限。
- 本版本没有新增 HTTP 路由、数据库迁移或服务端配置变更。
