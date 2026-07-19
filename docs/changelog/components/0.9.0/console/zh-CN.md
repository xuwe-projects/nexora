## CRUD 表格与顶部标签

- 新增 `CrudTableRow` 派生宏和 `CrudTableDelegate<T>`，标准资源管理表格可以继续使用
  gpui-component `DataTable`，同时减少列、单元格和文本导出的样板代码。
- `TableCell` 提供正文默认垂直居中、水平靠左的单元格增强 API；表头继续默认水平、垂直居中。
- 顶部 Feature 标签默认使用官方 `Segmented` 样式，并通过 `ApplicationTabStyle` 暴露可选变体。
- 新生成项目的 `.agents/skills` 已包含 CRUD Panel、CRUD DataTable 和宏验证闭环规则；已有项目
  升级时建议同步，并在本地执行一次 `cargo clean`。
