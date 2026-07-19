---
title: Application 与品牌
order: 1
---

# Application 与品牌

`Application` 负责启动 GPUI、发现注册项并创建主窗口。应用直接从 `gpui` 导入类型：

```rust
use gpui::App;
use nexora::{Application as _, ApplicationOptions};

struct DesktopApplication;

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("My App")
            .application_version(env!("CARGO_PKG_VERSION"))
    }

    fn initialize(&mut self, _cx: &mut App) {}
}
```

## Logo

默认登录页与 Sidebar Header 共用品牌配置。PNG 应编译进最终二进制：

```rust
use nexora::ApplicationLogo;

ApplicationOptions::new().application_logo(ApplicationLogo::png(include_bytes!(
    "../assets/logos/logo-icon-128.png"
)))
```

生成器会把整套 PNG、ICNS 与 ICO 图标放进桌面 package 的 `assets/logos`。只改名称、
版本或 Logo 不需要覆盖登录页。需要替换完整布局时，再实现唯一的
`#[derive(nexora::LoginFeature)]`。

自定义 `SidebarHeader` 会替换默认品牌区域。Shell 固定保留 Header 结构边界与下方分隔线，
但不会给自定义区域增加 hover 或点击语义。需要同时展示品牌与应用 Context 时，在自定义
Header 内使用 `SidebarRegion::new("application-context")` 等稳定 ID 组合独立命中区域；
Logo 没有动作时可以完全没有 hover，工厂选择器则可以自行添加 hover 与 Popover。

## 标签样式

主窗口顶部 Feature 标签默认使用 gpui-component story 中的官方默认 `Tabs` 样式。需要切换
视觉变体时，通过 `ApplicationOptions::tab_style` 选择官方 `Tab`、`Underline`、`Pill`、
`Outline` 或 `Segmented` 样式，标签切换、置顶、滚动和右键菜单行为不变；标签栏会使用
`theme::component_size(cx)` 跟随设置中的组件尺寸：

```rust
use nexora::{ApplicationOptions, ApplicationTabStyle};

ApplicationOptions::new().tab_style(ApplicationTabStyle::Underline)
```

## Account 自动发现

`desktop` 会编译 Account 客户端，但普通应用默认不显示认证门禁。应用在
`Application::initialize` 中调用 `install_authenticator` 后，框架自动启用登录门禁及默认
用户、角色权限页面，不需要在 `ApplicationOptions` 中重复声明开关。
