# Changelog

本项目的重要变更会记录在此文件中。版本格式遵循
[Semantic Versioning](https://semver.org/spec/v2.0.0.html)。

## [0.1.1] - 2026-07-17

### Added

- 增加默认 Setup 初始化流程、ZITADEL 人类用户选择、系统超级管理员绑定，以及用户、角色、
  权限管理能力。
- 增加可自定义应用品牌的登录页、Account 默认管理页面、Sidebar section 分组和通知错误反馈。
- 增加基于 VitePress 与 Bun 的中英文文档站及 GitHub Pages 部署工作流。
- 为生成项目增加带注释的配置、Logo 资源、项目 Rules 与模块化 Feature 开发 Skills。

### Changed

- 服务端由应用自行创建 `PgPool`、`TcpListener` 和 Axum State；Nexora `Server` 只负责迁移、
  模块初始化和提供可合并 Router。
- 桌面认证入口收敛到 `nexora::desktop`，服务端配置和扩展入口收敛到 `nexora::server`。
- `desktop` 与 `server` Cargo feature 分别内置对应 Account 能力，不再要求应用组合内部
  Account feature。
- 数据库升级完全依赖 SQLx 迁移历史，删除首次初始化布尔开关。

### Fixed

- 修正生成服务端配置路径、监听 IP/端口配置、用户展示名同步和未初始化系统启动提示。
- 修正 Sidebar Header/Footer 外壳样式、section 分隔位置和登录失败 request ID 反馈。

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

[0.1.1]: https://github.com/xuwe-projects/nexora/releases/tag/v0.1.1
[0.1.0]: https://github.com/xuwe-projects/nexora/releases/tag/v0.1.0
