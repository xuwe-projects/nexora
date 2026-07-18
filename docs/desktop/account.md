---
title: Account
order: 3
---

# Account

应用在 `Application::initialize` 中安装 `AccountAuthenticator` 后，Nexora 会自动启用：

- 默认 OIDC Authorization Code + PKCE 登录门禁；
- 登录失败 Notification；存在 `request_id` 时提供复制 Action；
- `/users` 用户管理与 `/roles` 角色权限管理导航；
- 可执行用户开通、状态与角色管理的默认用户页面；
- 可执行自定义角色与权限集合管理的默认角色页面；
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

## 默认管理能力

`/users` 使用卡片化 DataTable 展示头像、登录用户名和状态，并在滚动接近底部时继续加载；
刷新会从第一页重新读取。页面支持创建已经由管理员确认的 OIDC 用户、创建时选择初始角色、
启用或停用普通用户，以及完整替换普通用户的直接角色集合。创建操作不会在 Identity
Provider 创建账号，也不会引入本地密码；登录用户名用于管理识别，认证绑定仍以稳定
`identity_id` 为准。空初始角色只要求 `users:provision`；非空集合还要求
`users:roles.write`。角色选择和后续用户角色编辑还需要 `roles:read`。

`/roles` 支持查看角色与权限目录、创建带初始权限的自定义角色、编辑名称和说明、完整替换
权限集合，以及删除自定义角色。创建角色及其初始权限、更新、权限替换和删除统一使用
`roles:write`；查看可选权限需要 `permissions:read`。内置角色保持不可修改。

页面根据当前登录快照中的权限禁用不可执行操作并显示原因；超级管理员、内置角色和最后一个
启用管理员等不变量仍由服务端校验。默认用户管理不提供删除本地用户的能力。

## 覆盖默认页面

应用声明相同 ID 或路径的普通 Feature，即可逐页替换 `/users` 或 `/roles`，无需专用宏。
自定义页面通过 `nexora::desktop::api_session(cx)` 获取不暴露 token 的 API 会话，
并调用相同的用户开通、状态、用户角色、角色和权限方法。

完整替换登录布局时使用 `LoginFeature`。结构化错误可以从
`login_snapshot(cx).failure` 读取。
