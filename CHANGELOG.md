# Changelog

本项目的重要变更会记录在此文件中。版本格式遵循
[Semantic Versioning](https://semver.org/spec/v2.0.0.html)。

## [0.1.0] - 2026-07-17

Nexora 的首个 early-alpha 源码版本，当前通过 GitHub tag 分发。

### Added

- 提供同名 `nexora` 框架库与 CLI，支持交互式 `create`、`init`、macOS
  `build`、`doctor` 和 workspace `lint`。
- 提供 single 与 workspace 两种 Askama 项目模板，并将框架开发 Skills 写入新项目。
- 提供 GPUI Application Shell、自动注册的 Feature/Window、Sidebar 扩展、登录门禁、
  设置窗口、强类型 path/query 路由和生命周期。
- 通过 Cargo feature 提供可组合的 Account 客户端与 Axum 服务端能力，包括 OIDC、
  用户、角色、权限和可重试初始化接口。
- 提供不启用 Account 的基础 workspace，以及完整 Console、Server 集成示例。

### Fixed

- 生成项目使用可跨电脑迁移、固定到对应版本 tag 的 Nexora Git 依赖。
- 修正桌面 Shell 标签右键菜单的窗口上下文与关闭目标处理。

[0.1.0]: https://github.com/xuwe-projects/nexora/releases/tag/v0.1.0
