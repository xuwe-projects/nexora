# Nexora 项目规则

## 规则与技能

- 本文件是始终生效的仓库级约束；实现具体任务时继续读取 `.agents/skills` 中匹配的 Skill。
- 修改前先检查现有目录、模块、依赖和公共契约，沿用已有架构，不创建平行实现。
- 只修改当前需求需要的范围；不要为了“以后可能使用”提前增加抽象层、全局状态或依赖。

## 保持 Feature 轻量

- `Feature` 只负责路由状态协调、生命周期和顶层布局，不能同时承载完整的列表、筛选、
  创建、更新、详情和确认交互。
- 一个区域有独立状态、事件、异步任务或可独立命名时，立即拆为页面私有组件。
- 使用以下结构，禁止创建 `mod.rs`：

```text
src/features.rs
src/features/users.rs
src/features/users/components.rs
src/features/users/components/create.rs
src/features/users/components/update.rs
src/features/users/components/table.rs
```

- `users.rs` 声明 `mod components;`；`components.rs` 声明子模块，并只通过
  `pub(super) use` 暴露父 Feature 需要的类型。
- 无长期状态的组件使用 `#[derive(IntoElement)] + RenderOnce`，让 Feature 可以直接
  `.child(component)`。
- 有输入状态、焦点、异步任务或订阅的组件使用独立 `Entity<T> + Render`；Feature 在
  `FeatureElement::initialize` 中创建并保存 Entity。
- 组件通过 props、回调、类型化事件或共享 Entity 通信。只移动代码文件但把全部状态和
  handler 留在 Feature 中，不算组件化。
- 只有多个 Feature 确实复用的组件才上移到 `src/components`；不要把页面私有业务组件
  过早做成公共组件。
- 审查到 Feature 同时承担列表与 CRUD 表单，或 `render` 中出现多个可独立命名区域时，
  先拆组件再继续添加功能；不要只用行数判断。

## 遵守 GPUI 状态与渲染边界

- 先检查 `gpui-component` 是否已经提供对应组件；应用直接依赖并导入 `gpui` 与
  `gpui-component`，不要通过 `nexora::gpui` 等路径使用。
- 状态放在最近的真实使用者：局部状态留在组件，共享状态提升到最近共同父 Entity，只有
  跨窗口且与应用同生命周期的唯一状态才使用 `Global`。
- `render` 只能读取状态、计算轻量派生值和构建 Element；不得创建长期 Entity、发起
  网络或文件 I/O、建立订阅、启动不可追踪任务或修改业务状态。
- 修改可见状态后只通知需要刷新的 Entity。交互元素和列表项使用稳定业务 ID，不使用数组
  下标、时间戳或随机数作为 Element ID。
- `Task`、`Subscription` 和子 Entity 必须归属明确生命周期；异步任务更新 UI 时使用
  `WeakEntity` 或受 Context 管理的任务，避免循环强引用。
- 父组件向下传 props，子组件通过事件或回调向上报告意图；不要让组件互相访问并修改私有
  字段。

## 使用 Nexora 注册与导航

- 页面、独立窗口和专用槽位分别使用 Nexora 的 `Feature`、`Window`、
  `SettingsWindow`、`LoginFeature`、`SidebarHeader` 与 `SidebarFooter` 派生能力。
- 确保声明这些类型的模块进入编译，让 inventory 自动发现；不要维护第二套路由表、导航表
  或 RootView 分支。
- 动态 Feature 路径使用强类型 `path_params` / `query_params`，并设置
  `navigation = false`；应用通过 `FeatureContextExt` 读取已校验参数。
- Account 登录门禁和默认管理页面通过安装 `AccountAuthenticator` 自动启用，不增加
  `account_enabled` 一类重复开关。
- 桌面端认证配置、会话和 Account HTTP 客户端统一从 `nexora::desktop` 导入，不使用
  `nexora::account::client` 等内部层级。
- 自定义 `SidebarHeader`/`SidebarFooter` 只提供内容；Header/Footer 的 hover、内边距和
  Header 下方/Footer 上方分隔线由 Shell 固定托管，自定义插槽不能移除这些外壳样式。

## 保持服务端组合权属于应用

- 应用在 composition root 中创建并持有唯一 `PgPool` 和最终 Axum State；不得在
  `Server::new`、handler 或业务模块里隐式创建第二个连接池。
- 先把 `nexora::server::migrations()` 返回的框架迁移与应用迁移合并，拒绝跨来源版本冲突，
  再使用唯一 SQLx `Migrator` 执行一次；禁止分别运行框架和应用 Migrator。
- 使用 `server.initialize(&settings, &pool, setup_secret)` 初始化框架模块；该方法只装配
  Account、ZITADEL 与 Router，不执行迁移。
- 应用通过 `Router::new().merge(server.routers()).merge(application_routes)` 决定路由
  组合和中间件边界；不需要 Nexora HTTP 路由时不合并 `server.routers()`。
- `Server` 不得绑定端口、持有监听器或调用 `axum::serve`。应用自行创建 `TcpListener`、
  注入最终 State，并按标准 Axum 流程决定 TLS、日志与优雅关闭策略。
- Router 构建必须同步且无 I/O。handler 只提取请求、调用业务能力并映射响应；不要在
  handler 中创建依赖、管理迁移或直接编写业务 SQL。
- 数据库访问统一使用 SQLx + PostgreSQL，HTTP 使用 Axum 0.8 + Tower，异步运行时使用
  Tokio；不要为相同职责引入第二套 ORM、Web 框架或 runtime。
- `PgPool` 可以廉价克隆，不要再套 `Arc<PgPool>`，也不要使用全局静态连接池。

## 维护 API、授权与秘密边界

- 非 SSR API 使用资源名词、复数集合路径和正确 HTTP 方法；Axum 路径参数写作
  `{user_id}`，不要使用 `:user_id`，也不要自行增加 `/api/v1` 前缀。
- JSON、query、path 和 form 字段使用 `snake_case`；枚举 wire 值使用小写
  `snake_case`；时间字段使用有符号 Unix 秒整数。
- 请求/响应 DTO、领域模型和数据库实体保持分离；多端共享契约放入轻量 contracts
  边界，不让桌面端依赖完整服务端 crate。
- 使用 HTTP 状态码表达结果。错误响应保持稳定错误码、用户消息和 request ID，不返回
  SQL、堆栈、内部路径、令牌或 Provider 细节。
- 认证、授权和资源归属都必须校验；使用权限能力，不在 handler 中硬编码角色名称。
- 配置模板只放带注释的占位值。真实 PAT、setup secret、数据库密码和 token 不得提交或
  输出到日志；生产秘密通过环境变量或密钥系统注入。

## 管理配置、迁移和依赖

- 桌面 API 使用独立 `[api]` 配置；服务端监听 IP 与端口使用独立字段；服务端敏感字段
  必须附用途、格式和安全注释。
- 数据库升级只依赖 SQLx 迁移历史；不要增加 `initialize_empty_database` 等人工布尔开关。
- 已进入共享环境的迁移禁止修改或重排，修正必须新增后续迁移。
- workspace 第三方版本与来源统一声明在根 `[workspace.dependencies]`；成员 crate 使用
  `{ workspace = true }`，并只开启所需 feature。
- 桌面应用对 Nexora 只启用 `desktop,derive`；服务端只启用 `server,derive`。
- 公开 Rust API 必须具有说明职责、参数、副作用和错误条件的中文 rustdoc。

## 验证

- Rust 测试放在对应 crate 的 `tests/`，不要在生产源码中加入 `#[cfg(test)] mod tests`。
- 纯逻辑使用普通 `#[test]`；依赖 App、Window、Entity、Global、Action 或 GPUI 调度的
  行为使用 `#[gpui::test]`。
- HTTP 测试覆盖成功、认证失败、权限不足、校验失败、未找到、冲突和分页边界。
- 完成修改后运行与范围相称的 `cargo fmt`、`cargo check`、`cargo test`、严格 Clippy
  和 `nexora lint --deny-warnings`；无法运行的外部依赖测试必须明确说明。
