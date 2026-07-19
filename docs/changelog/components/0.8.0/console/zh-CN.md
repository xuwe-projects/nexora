## 标准 CRUD Panel

- 新增 `CrudPanel`、`CrudPanelToolbar` 与 `TableHeaderCell`，标准资源管理页可复用标题摘要、
  刷新、筛选/操作区、撑满剩余高度的数据主体和默认居中且可自定义的表头。
- 默认用户管理页面已采用新 Panel，并修复用户列显示、状态列宽度、表格剩余高度占满和弹窗覆盖
  问题。
- 新生成项目的 `.agents/skills` 已包含 CRUD Panel 使用规则；已有项目升级时建议同步该规则，并
  在本地执行一次 `cargo clean`。
