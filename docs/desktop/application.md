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

## 应用单例与窗口生命周期

Nexora 默认只允许一个应用主实例，并在同一个 GPUI 事件循环中承载主窗口、额外 Shell、
唯一 Settings 和注册 Window。因此默认配置只有一个操作系统进程，应用级 Global、业务模型、
Account、偏好和缓存可以由全部窗口共享；每个窗口仍独立持有自己的 Shell、Feature UI、焦点、
输入和表单草稿。窗口不能切换为子进程承载。

运行期仍可以打开额外 Shell、唯一 Settings 和注册 Window。Nexora 只持久化主窗口的
显示器、位置、尺寸与最大化/全屏状态；退出后不保留标签、额外 Shell、Settings 或注册
Window 会话。冷启动始终只创建一个主窗口并打开 `initial_path`。重复启动仍通过
应用身份级单例 IPC 激活已运行进程。

多个同进程 Shell 中，Feature 的
`Context<T>::navigate` 路由到来源 Entity 所在窗口，`App::navigate` 路由到活动 Shell，
无法确定来源时回退到主 Shell。

### 从窗口会话版本迁移

删除应用中的 `.single_instance(...)`、`.subprocess_windows(...)` 和
`.restore_window_sessions(...)` 调用；应用身份级单例门禁固定启用，新版本不提供替代开关。
`WindowSession`、`WindowSessionRole` 和 `WindowTabSession` 也已从公开 Rust API 删除。首次读取旧
`workspace.toml` 时，schema 0 保留 `main_window` 几何并丢弃 `pinned_tabs`；schema 1 只从
`windows` 中 ID 为 `main` 的记录提取主窗口位置。成功保存后，会话字段从文件中清除，
主题、DataTable 布局和 Account 非秘密偏好保留。

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

## Sidebar 导航搜索

Sidebar 导航搜索默认关闭。启用后，Shell 会在默认或自定义 `SidebarHeader` 与导航列表之间
显示搜索输入框，只过滤当前用户有权看到的 Section、NavigationGroup 和 Feature 标题：

```rust
use nexora::ApplicationOptions;

ApplicationOptions::new().sidebar_search(true)
```

搜索支持标题原文、无声调全拼和拼音首字母连续子串匹配。搜索期间目录使用临时展开状态；
清空搜索词后会恢复用户原来的展开/折叠状态。

## 标签样式

主窗口顶部 Feature 标签默认使用 gpui-component story 中的官方默认 `Tabs` 样式。需要切换
视觉变体时，通过 `ApplicationOptions::tab_style` 选择官方 `Tab`、`Underline`、`Pill`、
`Outline` 或 `Segmented` 样式，标签切换、置顶、滚动和右键菜单行为不变；标签栏会使用
`theme::component_size(cx)` 跟随设置中的组件尺寸：

```rust
use nexora::{ApplicationOptions, ApplicationTabStyle};

ApplicationOptions::new().tab_style(ApplicationTabStyle::Underline)
```

## 主面板标题栏动作

应用可以在 `Application::initialize` 中安装主面板标题栏右侧动作。Shell 会把这些动作渲染到
所有业务 Feature 的 `PanelHeader` 右侧，并放在框架内置的“置顶当前标签页”按钮之前：

```rust
use gpui::App;
use gpui_component::{Sizable as _, button::Button};
use nexora::{PanelHeaderAction, install_panel_header_actions};

fn initialize(&mut self, cx: &mut App) {
    install_panel_header_actions(
        vec![PanelHeaderAction::new(|_cx| {
            Button::new("open-tasks")
                .small()
                .label("任务")
                .on_click(|_, _, _| {
                    // 在这里触发导航、打开弹窗或派发应用动作。
                })
        })],
        cx,
    );
}
```

`PanelHeaderAction` 的渲染闭包会在标题栏渲染时收到当前 `App` 上下文。闭包应只读取状态并构造
元素；导航、弹窗、网络请求或业务副作用应放在按钮等元素自己的事件回调中。再次调用
`install_panel_header_actions` 会完整替换上一次安装的列表，传入空列表可清空已安装动作。

## Account 自动发现

`desktop` 会编译 Account 客户端，但普通应用默认不显示认证门禁。应用在
`Application::initialize` 中调用 `install_authenticator` 后，框架自动启用登录门禁及默认
用户、角色权限页面，不需要在 `ApplicationOptions` 中重复声明开关。
