---
title: Account
order: 3
---

# Account

应用在 `Application::initialize` 中安装 `AccountAuthenticator` 后，Nexora 会自动启用：

- 默认 OIDC Authorization Code + PKCE 登录门禁；
- 登录失败 Notification；存在 `request_id` 时提供复制 Action；
- `/users` 用户管理与 `/roles` 角色权限管理导航；
- 默认用户、角色权限页面；
- 登录用户头像与展示名；
- 退出登录时清理业务 Feature 与 Window。

## 初始化客户端

```rust
let settings: config::Settings = nexora::config::initialize(None)?;
let config = nexora::desktop::client_config(&settings, &settings.api)?;
let authenticator = nexora::desktop::AccountAuthenticator::new(&config)?;

nexora::desktop::install_authenticator(authenticator, cx);
```

不需要额外的 `account_enabled` 开关；没有安装认证器的普通桌面应用不会创建登录门禁，也
不会注入 `/users` 与 `/roles` 默认页面。

## 覆盖默认页面

应用声明相同 ID 或路径的普通 Feature，即可逐页替换 `/users` 或 `/roles`，无需专用宏。
自定义页面通过 `nexora::desktop::api_session(cx)` 获取不暴露 token 的 API 会话，
并调用用户、角色和权限方法。

完整替换登录布局时使用 `LoginFeature`。结构化错误可以从
`login_snapshot(cx).failure` 读取。
