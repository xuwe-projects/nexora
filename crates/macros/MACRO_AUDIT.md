# Nexora 宏审查记录

更新时间：2026-07-19

本文记录 `nexora-macros` 中自定义派生宏的展开结果、手写对比和验证闭环。完整输出可用本文命令重新生成；文中只保留关键运行时代码片段，省略测试断言、标准库派生和 `inventory` 的平台构造细节。

## 验证命令

```bash
cargo expand -p nexora-macros --test crud_table_row
cargo expand -p nexora-macros --test feature
cargo expand -p nexora-macros --test navigation_group
cargo expand -p nexora-macros --test singletons
cargo expand -p nexora-macros --test sidebar
cargo expand -p nexora-macros --test settings
cargo test -p nexora-macros --test crud_table_row
cargo bench -p nexora-macros --bench crud_table_row
cargo bloat --release -p nexora-macros --test crud_table_row -n 20
```

## 宏清单

| 宏 | 测试目标 | 展开行为 | 性能结论 |
| --- | --- | --- | --- |
| `Feature` | `tests/feature.rs` | 实现 `Feature`、转发 `Render`、生成工厂函数并提交 `FeatureRegistration` | 静态 metadata 和一次性注册，不在数据热路径 |
| `NavigationGroup` | `tests/navigation_group.rs` | 实现 `NavigationGroup` 并提交 `NavigationGroupRegistration` | 静态 metadata 和一次性注册 |
| `Window` | `tests/singletons.rs` | 实现 `Window`、转发 `Render`、生成窗口工厂和选项工厂 | 静态 metadata 和窗口创建路径 |
| `LoginFeature` | `tests/singletons.rs` | 实现 `LoginFeature`、生成登录页工厂并提交注册 | 静态注册和登录页创建路径 |
| `SettingsWindow` | `tests/singletons.rs` | 组合 `Window` 与 `SettingsWindow` 注册 | 静态注册和窗口创建路径 |
| `SidebarHeader` | `tests/sidebar.rs` | 实现 Header 插槽注册和工厂 | 静态注册和插槽创建路径 |
| `SidebarFooter` | `tests/sidebar.rs` | 实现 Footer 插槽注册和工厂 | 静态注册和插槽创建路径 |
| `Settings` | `tests/settings.rs` | 实现配置 trait 与账号配置 section provider | 只顺序调用已有校验函数 |
| `CrudTableRow` | `tests/crud_table_row.rs`、`benches/crud_table_row.rs` | 实现列定义、表头/正文对齐、单元格渲染和文本导出 | 与手写实现等价，bench 达标 |

## `CrudTableRow` 展开结果

命令：

```bash
cargo expand -p nexora-macros --test crud_table_row | sed -n '392,566p'
```

关键展开：

```rust
impl ::nexora::desktop::CrudTableRow for CityRow {
    fn columns() -> ::std::vec::Vec<::nexora::__private::gpui_component::table::Column> {
        vec![
            ::nexora::__private::gpui_component::table::Column::new("id", "ID")
                .width(::nexora::__private::gpui::px(64.))
                .min_width(::nexora::__private::gpui::px(48.))
                .fixed_left(),
            ::nexora::__private::gpui_component::table::Column::new("name", "城市")
                .width(::nexora::__private::gpui::px(160.))
                .sortable(),
            ::nexora::__private::gpui_component::table::Column::new("code", "代码")
                .width(::nexora::__private::gpui::px(96.))
                .ascending(),
            ::nexora::__private::gpui_component::table::Column::new("sort_order", "排序")
                .width(::nexora::__private::gpui::px(80.))
                .descending()
                .text_right(),
            ::nexora::__private::gpui_component::table::Column::new("status", "状态")
                .width(::nexora::__private::gpui::px(76.))
                .max_width(::nexora::__private::gpui::px(96.))
                .resizable(false)
                .movable(false)
                .selectable(false)
                .text_center(),
        ]
    }

    fn cell_alignment(key: &str) -> ::nexora::__private::gpui::TextAlign {
        match key {
            "sort_order" => ::nexora::__private::gpui::TextAlign::Right,
            "status" => ::nexora::__private::gpui::TextAlign::Center,
            _ => ::nexora::__private::gpui::TextAlign::Left,
        }
    }

    fn render_cell(
        &self,
        key: &str,
        window: &mut ::nexora::__private::gpui::Window,
        cx: &mut ::nexora::__private::gpui::App,
    ) -> ::nexora::__private::gpui::AnyElement {
        match key {
            "id" => ::nexora::__private::gpui::IntoElement::into_any_element(
                ::nexora::desktop::TableCell::new(::std::string::ToString::to_string(&self.id))
                    .align(<Self as ::nexora::desktop::CrudTableRow>::cell_alignment("id"))
                    .vertical_align(
                        <Self as ::nexora::desktop::CrudTableRow>::cell_vertical_alignment("id"),
                    ),
            ),
            "status" => ::nexora::__private::gpui::IntoElement::into_any_element(
                Self::render_status(self, window, cx),
            ),
            _ => ::nexora::__private::gpui::IntoElement::into_any_element(
                ::nexora::__private::gpui::Empty,
            ),
        }
    }

    fn cell_text(&self, key: &str, cx: &::nexora::__private::gpui::App) -> ::std::string::String {
        match key {
            "id" => ::std::string::ToString::to_string(&self.id),
            "status" => Self::status_text(self, cx),
            _ => ::std::string::String::new(),
        }
    }
}
```

与手写等价实现对比：

- 派生宏生成的 `columns` 与手写 `Column::new(...).width(...).sortable()` 链式调用等价，保留 gpui-component 原生 `Column` 用法。
- 派生宏生成 `match key`，没有动态查表、闭包分发或运行时字符串拼接。
- 默认字段渲染只做字段 `ToString` 并包一层 `TableCell`；自定义列直接调用用户提供的 `Self::render_xxx` 和 `Self::text_xxx`。
- 操作列不由派生宏生成，统一由 `CrudTableDelegate::action_column` 接入，避免把按钮/开关回调藏进字段属性。

基准结果：

```text
cargo bench -p nexora-macros --bench crud_table_row
derived: 8919917 ns
handwritten: 9581250 ns
```

派生实现略快于手写等价实现，低于当前 3 倍回归阈值。无需进入 `cargo asm` 迭代；`cargo asm 0.1.16` 已安装，后续未达标时使用。

体积结果：

```text
cargo bloat --release -p nexora-macros --test crud_table_row -n 20
.text section size: 1.0MiB, file size: 1.9MiB
top entries: trybuild/std/toml_parser/test harness
```

前 20 个体积热点都来自 `trybuild`、标准测试框架或 TOML 解析；`CrudTableRow` 展开代码没有进入热点。

## 注册类宏展开结果

`Feature`：

```rust
impl ::nexora::Feature for UserFeature {
    type Path = UserPath;
    type Query = UserQuery;
    const METADATA: ::nexora::FeatureMetadata = ::nexora::FeatureMetadata::new(
        "user", "用户详情", "/users/:id", None, None, None, 0i32, false,
    ).with_content_scrollable(false);
    const REGISTRATION: Option<::nexora::__private::FeatureRegistration> =
        Some(::nexora::__private::FeatureRegistration::new(
            Self::METADATA,
            __nexora_feature_factory_UserFeature,
        ));
}

fn __nexora_feature_factory_UserFeature(
    route: ::nexora::RouteMatch,
    window: &mut ::nexora::__private::gpui::Window,
    cx: &mut ::nexora::__private::gpui::App,
) -> Result<::nexora::FeatureInstance, ::nexora::FeatureRuntimeError> {
    ::nexora::__private::create_feature::<UserFeature>(
        route,
        window,
        cx,
        |_, _| Default::default(),
    )
}
```

`NavigationGroup`：

```rust
impl ::nexora::NavigationGroup for ResourcesGroup {
    const METADATA: ::nexora::NavigationGroupMetadata =
        ::nexora::NavigationGroupMetadata::new(
            "resources",
            "资料中心",
            "资料中心",
            Some("folder"),
            None,
            10i32,
        );
}
```

`LoginFeature`：

```rust
impl ::nexora::LoginFeature for CustomLogin {
    const REGISTRATION: ::nexora::__private::LoginFeatureRegistration =
        ::nexora::__private::LoginFeatureRegistration::new(
            "singletons::CustomLogin",
            __nexora_login_feature_factory_CustomLogin,
        );
}

fn __nexora_login_feature_factory_CustomLogin(
    window: &mut ::nexora::__private::gpui::Window,
    cx: &mut ::nexora::__private::gpui::App,
) -> ::nexora::__private::gpui::AnyView {
    ::nexora::__private::create_login_feature::<CustomLogin>(window, cx, CustomLogin::new)
}
```

`Window` + `SettingsWindow`：

```rust
impl ::nexora::Window for CustomSettings {
    type Path = ::nexora::NoPath;
    type Query = ::nexora::NoQuery;
    const METADATA: ::nexora::WindowMetadata =
        ::nexora::WindowMetadata::new("settings", "设置", "/settings", Some("settings"), 0);
    const REGISTRATION: Option<::nexora::__private::WindowRegistration> =
        Some(::nexora::__private::WindowRegistration::new_settings(
            "singletons::CustomSettings",
            Self::METADATA,
            __nexora_settings_window_factory_CustomSettings,
            __nexora_settings_window_options_CustomSettings,
        ));
}

impl ::nexora::SettingsWindow for CustomSettings {
    const REGISTRATION: ::nexora::__private::SettingsWindowRegistration =
        ::nexora::__private::SettingsWindowRegistration::new(
            "singletons::CustomSettings",
            ::nexora::__private::WindowRegistration::new_settings(
                "singletons::CustomSettings",
                <Self as ::nexora::Window>::METADATA,
                __nexora_settings_window_factory_CustomSettings,
                __nexora_settings_window_options_CustomSettings,
            ),
        );
}
```

`SidebarHeader` / `SidebarFooter`：

```rust
impl ::nexora::SidebarHeader for Header {
    const REGISTRATION: ::nexora::__private::SidebarHeaderRegistration =
        ::nexora::__private::SidebarHeaderRegistration::new(
            "sidebar::Header",
            __nexora_sidebar_header_factory_Header,
        );
}

impl ::nexora::SidebarFooter for Footer {
    const REGISTRATION: ::nexora::__private::SidebarFooterRegistration =
        ::nexora::__private::SidebarFooterRegistration::new(
            "sidebar::Footer",
            __nexora_sidebar_footer_factory_Footer,
        );
}
```

这些注册类宏与手写实现的差异只在于：

- 自动生成稳定的 metadata、factory 函数名和 `inventory` 提交代码；
- 自动补齐 `Render` 转发到 `FeatureElement` 或 `WindowElement`；
- 不生成循环、集合分配、I/O、后台任务或动态分发。

## `Settings` 展开结果

```rust
impl ::nexora::config::Settings for ApplicationSettings {
    const APP_NAME: &'static str = "nexora-macros";
    const MANIFEST_DIR: &'static str =
        "/Users/coloxan/projects/xuwe/desktop-template/crates/macros";

    fn validate(&self) -> Result<(), ::nexora::config::ConfigError> {
        <ClientSettings as ::nexora::config::AccountClientSection>::validate_account_client(
            &self.client,
        )?;
        <ServerSettings as ::nexora::config::AccountServerSection>::validate_account_server(
            &self.server,
        )?;
        Ok(())
    }
}

impl ::nexora::__private::ProvidesAccountClientSettings for ApplicationSettings {
    type AccountClientSettings = ClientSettings;
    fn account_client_settings(&self) -> &Self::AccountClientSettings {
        &self.client
    }
}
```

`Settings` 与手写实现等价，只顺序调用字段已有校验函数并返回字段引用；没有额外分配或全局副作用。
