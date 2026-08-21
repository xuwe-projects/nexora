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

## 未登录独立 Window 白名单

安装 `AccountAuthenticator` 后，`settings` Window 默认仍可在未登录状态打开，应用无需重新
登记。应用确实有关于页、许可信息等不依赖账号的独立 Window 时，可以按稳定 Window ID
显式增加白名单：

```rust
use nexora::ApplicationOptions;

ApplicationOptions::new().unauthenticated_window("about")
```

`about` 必须精确匹配一个由 `#[derive(nexora::Window)]` 注册的 Window ID。匹配区分大小写；
不存在的 ID、Feature ID、NavigationGroup ID 或重复登记都会在 `Application::validate()` 和
`run()` 进入 GPUI 事件循环前返回 `ApplicationError`。框架不会把白名单 Window 视为已经认证，
而是只跳过独立窗口门禁，随后继续执行同一套强类型路由提取、Window factory、原生开窗和
生命周期流程。未登记 Window 仍返回认证错误，登录后的窗口行为不变。

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

## 注册应用主题

下游应用可以把多个 gpui-component `ThemeSet` JSON 编译进二进制。每个预设必须恰好包含
一个 `light` 和一个 `dark` 主题；稳定 ID 使用 ASCII `snake_case`，`nexora` 与历史别名
`xuwe` 为框架保留值：

```rust
use nexora::{ApplicationOptions, ApplicationThemePreset};

ApplicationOptions::new()
    .theme_preset(ApplicationThemePreset::new(
        "acme",
        "Acme",
        include_str!("../themes/acme.json"),
    ))
    .theme_preset(ApplicationThemePreset::new(
        "ocean_blue",
        "Ocean Blue",
        include_str!("../themes/ocean-blue.json"),
    ))
    .default_theme_preset("acme")
```

Nexora 在 `Application::validate()` 和 `run()` 进入 GPUI 事件循环前校验 JSON、ID、浅深配对
和默认主题。默认设置窗口会自动列出 Nexora 与全部应用主题；切换后 Shell、业务 Feature、
Settings 和所有已打开或新建窗口同步更新，并写入 Shell 偏好。

启动选择顺序为“已有有效用户偏好 → 应用默认主题 → Nexora”。旧用户保存的有效 Nexora
选择不会因为应用后来增加品牌默认主题而改变；已删除主题的历史偏好会回退到应用默认主题并
自动写回。重置外观时同样恢复应用默认主题和“跟随系统”模式。

自定义 `SettingsWindow` 可以使用稳定 facade，不应直接修改 gpui-component 的全局 Theme：

```rust
use gpui::App;
use nexora::desktop::{
    ColorScheme, default_theme_preset_id, set_color_scheme, set_theme_preset,
    theme_presets, theme_selection,
};

fn select_brand_theme(cx: &mut App) -> Result<(), nexora::desktop::ThemeSelectionError> {
    for preset in theme_presets(cx) {
        println!("{}: {}", preset.id(), preset.label());
    }
    set_theme_preset("acme", cx)?;
    set_color_scheme(ColorScheme::System, cx);
    let _current = theme_selection(cx);
    let _application_default = default_theme_preset_id(cx);
    Ok(())
}
```

成功切换会自动刷新全部窗口并持久化；未知 ID 返回结构化错误且不改变当前选择。

## 全局工具栏动作与搜索 Provider

应用可以在 `Application::initialize` 中安装主窗口右侧全局工具动作。标准构造器使用官方
Button、Icon、Tooltip 与 Badge；页面级创建、导出或筛选操作仍留在 Feature 内：

```rust
use gpui::App;
use nexora::{ShellToolbarAction, install_shell_toolbar_actions};

fn initialize(&mut self, cx: &mut App) {
    install_shell_toolbar_actions(
        vec![ShellToolbarAction::new(
            "open-tasks",
            10,
            gpui_component::IconName::List,
            "任务中心",
            |_, _, _| {
                // 在这里打开窗口级能力。
            },
        )],
        cx,
    );
}
```

`ShellToolbarAction::custom` 只用于官方 Popover/Menu 等受控组合。再次安装会完整替换上一次
列表，重复稳定 ID 会被拒绝。全局搜索使用 `install_search_providers` 扩展；每个 Provider 可按
模式实现异步 `on_change`、`on_search` 和跨重启 `on_resolve_history`，互相独立 loading 与失败。

## Account 自动发现

`desktop` 会编译 Account 客户端，但普通应用默认不显示认证门禁。应用在
`Application::initialize` 中调用 `install_authenticator` 后，框架自动启用登录门禁及默认
用户、角色权限页面，不需要在 `ApplicationOptions` 中重复声明开关。
