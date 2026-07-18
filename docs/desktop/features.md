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

`section` 是 Sidebar 顶层大类。需要在 section 内建立只展开/收起、不打开页面的目录时，
使用 `NavigationGroup`；Feature 通过 `group` 引用目录：

```rust
#[derive(nexora::NavigationGroup)]
#[nexora(
    id = "production-model",
    title = "生产建模",
    section = "资料中心",
    icon = "folder",
    order = 10
)]
struct ProductionModelGroup;

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "车间",
    path = "/workshops",
    section = "资料中心",
    group = "production-model",
    order = 10
)]
struct WorkshopsFeature;
```

NavigationGroup 没有 path、页面 factory、Window 或 Tab；点击只切换展开状态。目录可通过
`parent = "另一个目录 ID"` 递归嵌套，但 parent 必须是同 section 的 NavigationGroup。
Feature 永远表示可导航页面，不能充当目录。注册阶段会拒绝重复稳定 ID、未知引用、自引用、
循环和跨 section 引用。活动叶子的全部祖先会在首次打开、刷新和路由恢复时自动展开；面包屑
可以显示目录标题，但目录不产生可点击路由，也不会进入标签页或最近页面。

带动态参数的页面应设置 `navigation = false`。

自定义 `SidebarHeader` 或 `SidebarFooter` 时只返回内容。Shell 只托管整体宽度、与导航区的
间距、内容内边距和 Header/Footer 分隔线，不会注入 hover、selected、圆角、cursor 或点击
语义。品牌与工厂选择器等区域使用稳定 ID 的 `nexora::desktop::SidebarRegion` 独立组合，
各自决定是否 hover 和点击。

长期 Entity、订阅和任务放入 `initialize` 等生命周期，不要在 `render` 中创建。
使用 gpui-component 时直接导入：

```rust
use gpui_component::button::Button;
```
