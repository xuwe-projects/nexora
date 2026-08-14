---
title: Account
order: 3
---

# Account

应用在 `Application::initialize` 中安装 `AccountAuthenticator` 后，Nexora 会自动启用：

- 默认 OIDC Authorization Code + PKCE 登录门禁；
- 登录失败 Notification；存在 `request_id` 时提供复制 Action；
- `/users` 用户管理与 `/roles` 角色权限管理导航；
- 人员与服务账号的统一列表、独立创建入口，以及服务账号资料/凭据管理；
- 可执行用户开通、状态与角色管理的默认用户页面；
- 可执行自定义角色与权限集合管理的默认角色页面；
- 登录用户圆形首字母/默认 Avatar 标识与展示名；
- 退出登录时清理业务 Feature 与 Window。

## 初始化客户端

```rust
let settings: config::Settings = nexora::config::initialize(None)?;
let config = nexora::desktop::client_config(&settings, &settings.api)?;
let authenticator = nexora::desktop::AccountAuthenticator::new(&config)?;

nexora::desktop::install_authenticator(authenticator, cx);
```

`ApiSettings` 默认只允许 HTTPS 或 loopback HTTP。确需连接内网纯 HTTP 服务时，可以在 `[api]`
中显式设置 `allow_insecure_http = true`；纯 HTTP 会明文传输 Bearer Token，只适用于调用方
已经接受风险的内网或受控环境。

不需要额外的 `account_enabled` 开关；没有安装认证器的普通桌面应用不会创建登录门禁，也
不会注入 `/users` 与 `/roles` 默认页面。

## 登录错误诊断

Account 客户端按请求阶段分类错误：连接失败、请求超时、响应读取失败、成功 HTTP
响应与客户端契约不兼容、服务端结构化拒绝，以及非结构化异常响应。界面不会把连接失败与
契约不兼容混为同一提示；有安全 `request_id` 时继续提供复制操作。

错误的 Display/Debug 摘要不包含完整 endpoint、Bearer token、Authorization header 或原始响应
正文。连接、超时、临时服务错误、响应读取与契约不兼容均保留 Keychain/Credential
Manager 凭据和恢复资格；只有 `invalid_grant`、OIDC subject 不一致、
`account_not_registered`、`account_suspended` 等已有明确永久语义的失败会清理恢复状态。

## 会话保持与自动续期

桌面 Account 配置会确保 OIDC scope 包含 `offline_access`，但不会重复添加调用方已经提供的
scope。默认登录页在 Windows/macOS 首次显示“保持登录状态”且默认选中；该选项只写入
`workspace.toml` 中的非敏感偏好。Linux 上复选框固定未选中并禁用，不接入 Secret Service，
也不会把 token 写入文件。

勾选后，登录成功会先把仅含 refresh token、OIDC subject、最小 profile 和版本号的记录写入
macOS Keychain 或 Windows Credential Manager，成功后才提交 `recovery_allowed`。重启后只有
偏好允许且安全存储记录存在时才会静默刷新，并再次调用 Account `/me`；恢复期间不会打开浏览器。
Provider 轮换 refresh token 时会先撤销旧的恢复许可，再保存新记录。安全存储暂时失败不影响
当前进程登录，后续刷新会再次尝试；Access Token 接近过期时运行时会在后台自动续期，旧任务由
generation 丢弃，同一账号的资料刷新不会销毁业务 Feature 或 Window。

安全存储 service 使用 `${application_identity}.account.oidc`：正式构建的应用身份来自
`nexora.toml` 注册的 `app_id`，开发运行则使用应用名与规范化可执行文件路径生成的稳定身份，
并遵守显式 application identity override。OIDC issuer 与 client ID 的摘要继续作为该 service
下的凭据 key，因此不同应用即使复用相同 OIDC 配置也不会共享 refresh token。

这是相对早期固定 `nexora.account.oidc` service 的破坏性变更。框架不会读取、迁移或删除旧项；
升级已有应用后用户需要重新登录一次，新的可恢复凭据才会写入应用自己的命名空间。

网络、Provider 5xx 和 Account 5xx 等临时错误会保留仍可能有效的 refresh token 并退避重试；
`invalid_grant`、subject 不一致、`account_suspended` 和 `account_not_registered` 会禁止恢复、
清理本地凭据并返回登录门禁。默认登录请求使用 `prompt=select_account`，因此可以选择其他
浏览器账号。登录页提供“重试恢复”和“使用其他账号登录”，后者不会执行 Provider 全局退出。

## 退出与撤销

`sign_out()` 先立即清理内存会话和业务门禁，再后台写入 `recovery_allowed=false`、删除安全
凭据并尽力调用 Provider 的 `revocation_endpoint` 撤销 refresh token。撤销请求只包含 token、
`token_type_hint=refresh_token` 和 `client_id`，不发送 client secret；没有撤销端点或撤销失败
也不会阻止本地退出。普通退出不调用 `end_session_endpoint`，不会清除浏览器 Cookie，因此
仍保留 ZITADEL SSO。

## 默认管理能力

`/users` 使用卡片化 DataTable 展示圆形首字母/默认 Avatar 标识、登录用户名和紧凑状态 Tag，支持列顺序移动与列宽
调整，并按实际内容高度展开；超过最大高度后在表格内部滚动，接近底部时继续加载，刷新则从
第一页重新读取。页面创建用户时由服务端调用 ZITADEL gRPC 创建人类用户，再自动绑定返回的
稳定 identity ID；UI 不要求操作者填写内部 ID，也不引入本地密码。登录后的 `GET /me` 会从
ZITADEL 同步最新用户名、邮箱与展示名。页面还支持选择初始角色、启用或停用普通用户，
以及完整替换直接角色集合。空初始角色只要求 `users:provision`；非空集合还要求
`users:roles.write`。角色选择和后续用户角色编辑还需要 `roles:read`。

`/roles` 支持查看角色与权限目录、创建带初始权限的自定义角色、编辑名称和说明、完整替换
权限集合，以及删除自定义角色。创建与编辑统一使用内容区 `FormDialog`，不会锁住整个应用；
系统管理员角色具有特殊标识，新注册权限会被服务端自动补入。创建角色及其初始权限、更新、权限替换和删除统一使用
`roles:write`；查看可选权限需要 `permissions:read`。内置角色保持不可修改。

页面根据当前登录快照中的权限禁用不可执行操作并显示原因；超级管理员、内置角色和最后一个
启用管理员等不变量仍由服务端校验。默认用户管理不提供删除本地用户的能力。

服务账号使用单独的 `FormDialog`，不会把人员字段混入同一个切换表单。创建时可选择
`Client Credentials（推荐）`、PAT 或暂不生成凭据；PAT 可选到期日期，永不过期会显示风险
提示并二次确认。账号创建和初始凭据生成是两个服务端操作：若凭据失败，已创建账号会保留，
页面明确提示可从“账号与凭据”重试。

凭据面板使用现有 DataTable 展示名称、类型、状态、创建人/时间、到期时间、撤销信息和
Provider 外部来源，并支持刷新协调、Client Secret 轮换、PAT 创建和单凭据撤销。敏感内容在
独立一次性 `FormDialog` 中用 Clipboard 复制；用户确认已保存并关闭后，组件立即清除明文状态。
服务账号仍使用统一角色与状态操作，不自动获得 `member`，也不提供删除操作。

## 覆盖默认页面

应用声明相同 ID 或路径的普通 Feature，即可逐页替换 `/users` 或 `/roles`，无需专用宏。
自定义页面通过 `nexora::desktop::api_session(cx)` 获取不暴露 token 的 API 会话，
并调用相同的用户开通、状态、用户角色、角色和权限方法。

完整替换登录布局时使用 `LoginFeature`。结构化错误可以从
`login_snapshot(cx).failure` 读取。该快照还提供 `busy`、`restoring`、`remember_login`、
`secure_storage_supported`、`can_retry_recovery` 等无敏感状态；自定义登录页可调用
`set_remember_login`、`retry_recovery` 和 `login_with_other_account`，不需要读取 Keychain
或任何 token。
