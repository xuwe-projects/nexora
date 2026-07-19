## 表单弹层 Panel 高度修正

- `FormDialog` 常规创建/编辑表单默认高度改为当前 Feature Panel 高度的 80%。
- `PanelDialog` 百分比高度不再被 overlay 内边距缩小，弹层会基于整个 Panel 垂直居中。
- 使用标准 `FormDialog` 的应用升级到 0.11.2 后无需改业务代码。
