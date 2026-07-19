## FormDialog API 变更

- `FormDialog::new(id, state, title, content, on_submit)` 已替换为
  `FormDialog::new(id, state).title(...).child(FormItem::new(...)).section(...).on_submit(...)`。
- `PanelDialog` 默认不再点击遮罩关闭；需要遮罩关闭时显式调用 `.overlay_closable(true)`。
- `ApplicationTabStyle` 新增 `Tab` 并作为默认值；需要旧分段视觉时显式设置 `Segmented`。
