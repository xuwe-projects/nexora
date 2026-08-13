# gpui-component 参考快照来源

- 仓库：`https://github.com/longbridge/gpui-component`
- 锁定 revision：`55b6bb88905d8e76cd23d9e3ebea3151dcdb84a0`
- 上游路径：`skills/gpui-component/references/usage.md`
- 同步日期：`2026-08-13`
- SHA-256：`98e73cf3e8d77bfe60f5aeba763bf52a69a982499a4a51b90cd0f8a13d53ea3f`

`usage.md` 是上游文件的原样快照，不在其中修正示例。当前 revision 的 usage 快速示例仍写有
`window.open_modal`，但锁定源码公开的是 `gpui_component::WindowExt::open_dialog`、
`open_alert_dialog` 与 `open_sheet`。项目实现遇到冲突时，以锁定源码、对应 component 完整文档和
`crates/story` 为准；不得把已知冲突示例复制到应用模板。
