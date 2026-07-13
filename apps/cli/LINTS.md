# Xuwe Lint 规则

`xuwecli lint` 用于检查 Cargo workspace、Rust 公开 API、GPUI 生命周期、技术选型、HTTP API 和数据库边界。它补充 rustc 与 Clippy，不重复实现编译器已经能够稳定检查的语言规则。

## 使用方式

```bash
xuwecli lint
xuwecli lint --workspace /path/to/workspace
xuwecli lint --deny-warnings
xuwecli lint --format json
```

- `error` 表示确定违反团队规范，命令始终返回非零退出码。
- `warning` 表示需要结合业务语境确认，默认只报告；传入 `--deny-warnings` 后也会阻止提交。
- `--format json` 输出结构化诊断和错误、警告数量，适合 CI 与编辑器接入。

## Cargo 与 Workspace

| 规则 | 级别 | 检查内容 |
| --- | --- | --- |
| `xuwe::dependency_not_in_workspace` | error | 成员 crate 的普通、开发、构建及平台依赖必须使用 `{ workspace = true }`。 |
| `xuwe::broad_dependency_feature` | warning | 警告 `full`、`all`、`everything` 等聚合 feature。 |
| `xuwe::forbidden_dependency_edge` | warning | 检查 library 到 app、console 到 server，以及 contracts/models 到基础设施的错误依赖方向。 |
| `xuwe::forbidden_technology` | error | 数据库只使用 SQLx，HTTP 服务只使用 Axum，异步运行时只使用 Tokio。 |
| `xuwe::invalid_crate_name` | error | 禁止 `xxx-core`、`xxx-handle`、`xxx-handler`。 |
| `xuwe::invalid_migration_location` | error | 迁移 crate 和 SQL 文件必须位于 `crates/migrate`。 |
| `xuwe::mixed_binary_library` | error | 同一 package 禁止同时声明 binary 和 library target。 |
| `xuwe::modified_migration` | error | 禁止修改 Git 已跟踪的既有迁移，应新增后续迁移。 |

## Rust API 与测试

| 规则 | 级别 | 检查内容 |
| --- | --- | --- |
| `xuwe::inline_test_module` | error | `src/` 中禁止 `#[cfg(test)]`、`cfg_attr(test, ...)` 和 `mod tests`。 |
| `xuwe::forbidden_mod_rs` | error | 模块入口禁止使用 `mod.rs`，统一使用与模块同名的 `.rs` 文件。 |
| `xuwe::missing_errors_section` | error | 返回 `Result` 的公开接口必须包含 `# Errors`。 |
| `xuwe::missing_panics_section` | error | 存在明确 `panic!`、断言、`unwrap` 或 `expect` 的公开接口必须包含 `# Panics`。 |
| `xuwe::non_chinese_public_docs` | error | 公开类型、字段、变体、模块、trait、函数和方法必须具有中文 rustdoc。 |

## GPUI 与桌面交互

| 规则 | 级别 | 检查内容 |
| --- | --- | --- |
| `xuwe::action_outside_actions` | error | `actions!`、`impl_actions!` 和 Action derive 必须位于 `crates/actions`。 |
| `xuwe::copied_global_state` | warning | 业务组件不能保存完整 `Theme` 或 `ThemeRegistry` 副本。 |
| `xuwe::detached_lifecycle` | warning | `.detach()` 必须确认任务或订阅确实不需要由 Entity 字段管理。 |
| `xuwe::empty_event_handler` | error | 禁止 `.on_click(|...| {})` 等空事件处理器。 |
| `xuwe::global_refresh_scope` | warning | 非主题模块不应调用 `refresh_windows()` 扩大刷新范围。 |
| `xuwe::hardcoded_visual_color` | warning | `theme` crate 之外的 GPUI 代码不直接调用 `rgb()`、`rgba()`、`hsl()` 或 `hsla()`。 |
| `xuwe::icon_button_without_tooltip` | warning | 纯图标 Button 必须提供 Tooltip、可见文本或可访问名称。 |
| `xuwe::non_gpui_global_state` | error | GPUI crate 禁止使用 `static mut`、`thread_local!` 或静态锁保存应用状态。 |
| `xuwe::render_side_effect` | error | `render()` 中禁止直接创建 Entity、订阅、启动任务、修改 Global 或执行 I/O。 |
| `xuwe::unstable_element_id` | warning | ElementId 不使用列表位置、时间戳或随机值。 |
| `xuwe::untracked_task` | warning | `Task` 和 `Subscription` 返回值不能被直接丢弃。 |

## Axum、REST 与 SQLx

| 规则 | 级别 | 检查内容 |
| --- | --- | --- |
| `xuwe::database_entity_in_contract` | warning | contracts/models 不暴露 `FromRow` 或 SQLx 映射属性。 |
| `xuwe::dynamic_sql_concatenation` | error | 禁止把 `format!` 或字符串加法产生的 SQL 传给 SQLx 查询和 `QueryBuilder::push`。 |
| `xuwe::non_rest_route` | warning | Axum 路由不使用动作式、大小写混合或下划线路径。 |
| `xuwe::raw_axum_request` | warning | handler 优先使用 `Path`、`Query`、`Json`、`Form`、`Multipart`、`State` 或自定义 extractor。 |
| `xuwe::unbounded_request_body` | warning | 消费原始正文或上传的 handler 必须配置 `DefaultBodyLimit`。 |

## 局部豁免

只有 `warning` 可以豁免，并且必须紧邻目标代码、填写原因。Rust 源码使用：

```rust
// xuwe-lint: allow(xuwe::detached_lifecycle) reason="监听由窗口持有并随窗口销毁"
subscription.detach();
```

Cargo manifest 使用 `#` 注释。目前 Cargo 豁免用于确实需要聚合 feature 的依赖：

```toml
# xuwe-lint: allow(xuwe::broad_dependency_feature) reason="工具需要解析完整 Rust AST"
syn = { version = "2", features = ["full"] }
```

豁免只表达经过审查的例外，不应作为消除未知警告的快捷方式。
