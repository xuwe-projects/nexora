## 侧边栏品牌换行修复

- 默认 Sidebar Header 的应用名称和副标题在窄侧边栏中保持单行显示，空间不足时使用省略截断，
  避免品牌文案被压成逐字竖排。
- 自定义 `SidebarHeader` 不会被框架注入该样式；需要相同行为时请在自定义组件内显式添加
  `overflow_hidden`、`whitespace_nowrap` 和 `truncate`。
