## Sidebar 导航搜索

- `ApplicationOptions::sidebar_search(true)` 可启用 Shell 内置导航搜索，支持原文、无声调全拼和
  拼音首字母连续子串，并且只展示当前账号有权访问的导航。
- 搜索框复用 gpui-component `Input`、`Sidebar` 与 `Icon`，搜索期间临时展开匹配目录，清空后
  恢复用户原有的展开/折叠状态。
