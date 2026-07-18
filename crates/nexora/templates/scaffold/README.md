# {{ project_name }}

这是一个使用 [Nexora](https://github.com/xuwe-projects/nexora) 生成的 Rust 桌面应用。

## 环境要求

- 支持 Rust 2024 edition 的稳定 Rust 工具链

## 运行

```bash
cargo run
```

首次构建会生成 `Cargo.lock`。应用项目应提交该文件，并在 CI 和部署中使用
`cargo run --locked`、`cargo build --locked` 等命令保持完整依赖图稳定。
{% if account_enabled %}
首次启动前，请先完善 `config/server.toml` 和 `config/{{ project_name }}.toml`，然后分别启动服务端与桌面端：

```bash
cargo run -p server -- config/server.toml
cargo run -- config/{{ project_name }}.toml
```
{% endif %}

## 品牌定制

生成的应用名称和版本会自动用于登录页、登录按钮、安全说明与默认 Sidebar Header。
生成器会把整套图标放到桌面 package 的 `assets/logos`，并默认使用 128px PNG。需要替换
Logo 时，覆盖资源或修改 `ApplicationOptions` 中的路径：

```rust
use nexora::ApplicationLogo;

ApplicationOptions::new()
    .application_name("{{ project_name }}")
    .application_version(env!("CARGO_PKG_VERSION"))
    .application_logo(ApplicationLogo::png(include_bytes!(
        "../assets/logos/logo-icon-128.png"
    )))
```

需要替换完整登录布局时，继续使用唯一的 `#[derive(nexora::LoginFeature)]`。

## 自定义图标

桌面入口会把 `assets/icons/**/*.svg` 编译进应用，并注册为 GPUI 资源。Feature 或导航目录
可以直接引用不带 `icons/` 前缀和 `.svg` 后缀的路径：

```rust
#[derive(Default, nexora::Feature)]
#[nexora(title = "仓库", path = "/warehouses", icon = "warehouse")]
struct WarehousesFeature;
```

上例会读取 `assets/icons/warehouse.svg`。需要分组命名时，可以放到子目录，例如
`assets/icons/app/warehouse.svg` 对应 `icon = "app/warehouse"`。

## 发布版本

项目生成时已经包含 `.agents/skills/publish-nexora-release`。发布新版本时使用该 Skill 整理
完整改动、处理人与关联 Issue/PR，只编写上一版本到当前版本的升级说明，并在全部门禁通过后
推送 tag 和创建 GitHub Release。
