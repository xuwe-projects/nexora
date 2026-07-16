---
name: define-page
description: 用于在本仓库的 GPUI 桌面应用中创建或修改页面。适用于新增带 Sidebar 与 Tab 导航的业务 Feature，或新增类似 Settings、About、Profile 的独立窗口；覆盖具体 App 的目录归属、模块注册、FeatureId、导航目录、RootView、Action、Global 窗口句柄、主题挂载和测试。
---

# 定义桌面页面

## 共同约束

- 同时遵守 `rust-code-style`、`gpui-desktop-development` 和 `desktop-ui-component-selection`。
- 先定位具体应用，例如 `examples/console`。应用专属页面、FeatureId、导航和窗口状态归具体 App，不放进共享 `desktop` 或 `ui` crate。
- 禁止创建 `mod.rs`。父模块使用 `features.rs`、`windows.rs` 等同名文件，子模块放在对应目录。
- 优先使用 `gpui-component` 现有组件和主题 token，不手写已有 Sidebar、Settings、Tabs、Dialog、Menu、Table 等能力。
- 普通 `#[test]` 只测试纯逻辑；依赖 App、Window、Entity、Global、Action 或 GPUI 调度时使用 `#[gpui::test] + TestAppContext`。

## 选择页面类型

先判断页面属于哪一种：

1. 需要出现在主 Sidebar、打开为 Tab，并参与前进后退历史：创建“带导航 Feature”。
2. 不属于主工作流，需要独立窗口生命周期，例如设置、关于、账号资料：创建“独立窗口”。
3. 只是独立窗口内的一个分类：只增加窗口内部的 Page，例如 Settings 中新增 `SettingPage`，不要增加主应用 FeatureId。

## 创建带导航 Feature

以下路径使用 `apps/<app>` 表示具体桌面应用。

### 1. 创建 Feature 模块

创建：

```text
apps/<app>/src/features/<feature>.rs
```

无独立状态时使用静态渲染函数或 `RenderOnce`：

```rust
//! 订单功能模块。

use gpui::{AnyElement, Context, IntoElement, div, prelude::*};
use ui::Card;

/// 订单功能视图。
pub struct OrdersFeature;

impl OrdersFeature {
    /// 渲染订单页面内容。
    pub fn render<T>(_cx: &mut Context<T>) -> AnyElement
    where
        T: 'static,
    {
        Card::new().p_5().child(div().child("订单")).into_any_element()
    }
}
```

需要独立刷新、焦点、订阅或异步任务时使用 `Entity<Feature> + Render`，让 Feature 自己持有状态、Task 和 Subscription。RootView 只保存 Entity 句柄，不复制内部状态。

### 2. 在父模块注册

修改 `apps/<app>/src/features.rs`：

```rust
/// 订单管理功能模块。
#[path = "features/orders.rs"]
pub mod orders;
```

不要创建 `features/mod.rs`。

### 3. 注册 FeatureId 与导航目录

在 `FeatureId` 中增加带中文 rustdoc 的变体，并在 `title()` 中增加展示名称：

```rust
/// 订单功能区，用于管理业务订单。
Orders,

Self::Orders => "订单",
```

在 `feature_catalog()` 中加入导航项：

```rust
FeatureItem::new(FeatureId::Orders, "业务管理"),
```

需要子导航时定义 `FeatureChildItem`，并使用 `FeatureItem::with_children`。Sidebar 分组从 catalog 自动生成，不再手工修改 `render_sidebar()`。

### 4. 接入 RootView

修改 `apps/<app>/src/features/root.rs`：

1. 导入 Feature 类型。
2. 在 `render_active_feature()` 中增加匹配分支。
3. 在 `feature_icon()` 中选择已有 `IconName`。
4. 在 `nav_badge()` 中提供稳定编号；插入中间位置时同步调整后续编号。
5. 页面需要自己管理滚动时，把它加入关闭外层滚动的判断。

无状态页面：

```rust
FeatureId::Orders => OrdersFeature::render(cx),
```

有独立生命周期的页面：在 RootView 构造阶段创建 `Entity<OrdersFeature>`，匹配分支渲染该 Entity。不要在 `render()` 中创建长期 Entity。

### 5. 测试导航与状态

修改 `apps/<app>/tests/` 下的集成测试：

- 更新 catalog 顺序断言。
- 验证标题、分组和子导航。
- 验证选择 Feature 后 Tab 与历史状态。
- Feature 使用 Entity、Window 或 Action 时增加 `#[gpui::test]`。

## 创建独立窗口

独立窗口必须在当前 GPUI 进程中通过 `cx.open_window` 创建，不启动第二个进程，也不再次调用 `Application::run`。

### 1. 定义打开窗口的 Action

在 `crates/actions/src/<window>.rs` 使用 `gpui::actions!` 定义类型化 Action，并按需要提供跨平台快捷键注册函数：

```rust
gpui::actions!(profile_window, [
    /// 打开或激活个人资料窗口。
    OpenProfile
]);
```

在 `crates/actions/src/lib.rs` 公开模块。按钮、菜单和快捷键必须派发同一个 Action，禁止分别实现三套打开逻辑。

### 2. 创建应用专属窗口模块

页面仍归具体 App。可以沿用 Settings 的结构放入：

```text
apps/<app>/src/features/profile.rs
```

如果独立窗口很多，则创建：

```text
apps/<app>/src/windows.rs
apps/<app>/src/windows/profile.rs
```

并由 `windows.rs` 声明 `#[path = "windows/profile.rs"] pub mod profile;`，不要使用 `windows/mod.rs`。

### 3. 保存单实例窗口句柄

为“重复触发时激活已有窗口”的页面定义最小 Global：

```rust
#[derive(Default)]
struct ProfileWindowState {
    window: Option<WindowHandle<gpui_component::Root>>,
}

impl Global for ProfileWindowState {}
```

Global 只保存窗口句柄，不保存窗口内部表单、滚动或业务状态。窗口私有状态由窗口根 Entity 持有。

### 4. 注册 Action 处理器

模块提供 `init(cx: &mut App)`：

1. 防止重复初始化 Global。
2. 注册 `cx.on_action`。
3. 已有句柄且仍有效时调用 `activate_window()`。
4. 句柄失效时清空并创建新窗口。

在具体应用的 `Application::build_root_view` 中调用窗口模块 `init(cx)`，并在同一位置注册快捷键。

### 5. 创建并挂载窗口根视图

使用 `WindowOptions` 配置初始尺寸、最小尺寸和 `TitleBar::title_bar_options()`：

```rust
let handle = cx.open_window(options, |window, cx| {
    let view = cx.new(|_| ProfileWindow::new());
    let root = cx.new(|cx| gpui_component::Root::new(view, window, cx));
    theme::attach_window(window, cx);
    root
})?;
```

- 新窗口必须包裹 `gpui_component::Root`。
- 每个新窗口必须调用 `theme::attach_window`，以同步主题和系统外观。
- macOS 透明标题栏参考现有 Settings 窗口；非 macOS 保留官方 `TitleBar::new()` 和系统窗口按钮。
- 打开成功后保存句柄并激活应用；失败时返回或记录具有上下文的错误。

### 6. 设计窗口内容

- 设置类窗口使用 `Settings`、`SettingPage`、`SettingGroup` 和 `SettingItem`。
- 普通资料、关于或工具窗口按交互语义选择 Form、Tabs、DataTable、Scrollable 等官方组件。
- 一个独立窗口包含多个内部页面时，页面切换由窗口根 Entity 管理，不加入主应用 FeatureId。

### 7. 测试窗口行为

在 `apps/<app>/tests/` 增加 GPUI 集成测试，至少覆盖：

- Action 注册后可以创建窗口。
- 重复派发 Action 不会创建重复窗口，而是激活已有窗口。
- 窗口根 Entity 可以完成渲染。
- Global、窗口句柄和异步任务生命周期符合预期。

## 完成检查

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p nexora -- lint --deny-warnings
```

最后确认：

- Feature 和独立窗口属于具体 App。
- 主导航 Feature 与独立窗口没有混用。
- 没有新增 `mod.rs` 或 bin crate `lib.rs`。
- 没有空按钮、空 Action 处理器或硬编码视觉颜色。
- 所有公开 Rust API 都有详细中文 rustdoc。
