---
title: Feature 与导航
order: 2
---

# Feature 与导航

```rust
use gpui::{Context, IntoElement, Window, div};
use nexora::FeatureElement;

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "首页",
    path = "/",
    section = "工作台",
    icon = "layout-dashboard",
    order = 0
)]
struct HomeFeature;

impl FeatureElement for HomeFeature {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div().child("Hello Nexora")
    }
}
```

`section` 会成为 Sidebar 分组标题，不同分组之间显示分隔关系。`parent` 用于二级导航；
带动态参数的页面应设置 `navigation = false`。

自定义 `SidebarHeader` 或 `SidebarFooter` 时只返回内容。Shell 仍会统一保留 hover、内边距，
以及 Header 下方和 Footer 上方的分隔线，因此自定义插槽不会破坏整体导航外壳。

长期 Entity、订阅和任务放入 `initialize` 等生命周期，不要在 `render` 中创建。
使用 gpui-component 时直接导入：

```rust
use gpui_component::button::Button;
```
