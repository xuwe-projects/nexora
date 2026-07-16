---
name: gpui
description: 提供 GPUI 框架知识，涵盖 Action 与快捷键、异步与后台任务、上下文管理（App、Window、Context、AsyncApp）、自定义元素（底层 Element trait）、Entity 状态管理、事件系统、焦点处理、全局状态、布局与样式（Flexbox、类 CSS）以及测试。处理任何 GPUI 框架概念、构建 GPUI 应用，或需要 GPUI 专用 API 与模式指导时使用。
---

## 导航

根据任务加载相关参考文件：

| 主题 | 文件 | 加载时机 |
|------|------|----------|
| Action 与快捷键 | [action.md](references/action.md) | 使用 `actions!`、`bind_keys`、`on_action`、`key_context` 时 |
| 异步与后台任务 | [async.md](references/async.md) | 使用 `cx.spawn`、`background_spawn`、`Task`、异步 I/O 时 |
| 上下文管理 | [context.md](references/context.md) | 使用 `App`、`Window`、`Context<T>`、`AsyncApp` 时 |
| 自定义元素（底层） | [element.md](references/element.md) | 实现 `Element` trait、`request_layout`、`prepaint`、`paint` 时 |
| Entity 状态 | [entity.md](references/entity.md) | 使用 `Entity<T>`、`WeakEntity` 或管理状态时 |
| 事件与订阅 | [event.md](references/event.md) | 使用 `cx.emit`、`cx.subscribe`、`cx.observe` 时 |
| 焦点与键盘导航 | [focus-handle.md](references/focus-handle.md) | 使用 `FocusHandle`、`track_focus` 或 Tab 导航时 |
| 全局状态 | [global.md](references/global.md) | 使用 `Global` trait、`cx.global`、`cx.update_global` 或应用级配置时 |
| 布局与样式 | [layout-style.md](references/layout-style.md) | 使用 `div()`、`h_flex()`、`v_flex()`、Flexbox、溢出或定位时 |
| ElementId | [element-id.md](references/element-id.md) | 使用 `ElementId`、`.id()`、唯一性规则或有状态元素时 |
| 测试 | [test.md](references/test.md) | 使用 `#[gpui::test]`、`TestAppContext`、`VisualTestContext` 或 `VisualTestAppContext` 时 |

## 扩展参考

深入研究以下主题时，可加载对应的扩展参考文件：

**Element trait：**

- [element-api.md](references/element-api.md) — 完整 API、命中区域系统和事件处理
- [element-patterns.md](references/element-patterns.md) — 文本、交互、容器和复合元素模式
- [element-examples.md](references/element-examples.md) — 文本、交互和复杂元素的完整示例
- [element-best-practices.md](references/element-best-practices.md) — 性能、状态和常见陷阱
- [element-advanced.md](references/element-advanced.md) — 瀑布流/环形布局、异步更新和虚拟列表

**Entity 管理：**

- [entity-api.md](references/entity-api.md) — 完整 Entity API、方法和生命周期
- [entity-patterns.md](references/entity-patterns.md) — 模型-视图、跨 Entity 通信和观察者模式
- [entity-best-practices.md](references/entity-best-practices.md) — 内存、性能和生命周期
- [entity-advanced.md](references/entity-advanced.md) — 集合、注册表、防抖和状态机

**测试：**

- [test-examples.md](references/test-examples.md) — 测试示例与模式
- [test-reference.md](references/test-reference.md) — 完整测试 API 参考
