---
name: gpui-desktop-development
description: 用于设计、实现或审查 Nexora/GPUI 桌面应用架构、状态、生命周期和组件边界。任何可见交互编码前必须完成组件选型记录，优先复用 nexora::desktop 与 gpui-component；已有组件时禁止重复实现，纯 GPUI Element 自定义必须有布局、绘制、性能或交互能力缺口证据。
---

# GPUI 桌面程序开发规范

## 规范关系

- 同时遵守 `rust-code-style`，处理 Rust 文档、依赖、测试和代码质量。
- 涉及 UI 交互时先使用 `desktop-ui-component-selection` 和 `gpui-component`；“检查组件库”是编码前门禁，不是事后清单问题。
- 以 workspace 当前锁定的 GPUI 和 `gpui-component` 源码 API 为准；外部文档与锁定版本冲突时，先核对源码再实现。
- 编码前必须写出简短组件选型记录：UI 区域、首选 `nexora::desktop` 封装或 `gpui-component` 组件、是否需要增强、选择依据。
- 审查时发现手写标准控件，应优先替换为现有组件后再继续新增功能。

## 核心原则

> 状态按生命周期归属，谁使用谁持有；多人共享就提升到最近共同父级。数据向下传，意图向上发，副作用离开 `render`，刷新显式触发。

## 1. 全局状态

### 使用 GPUI Global

- 所有可变的应用级全局状态都实现 GPUI [`Global`]，通过 `App` 上下文注册和访问。
- 优先使用 `ReadGlobal`、`UpdateGlobal` 或 `cx.global::<T>()`、`cx.update_global(...)`，不要使用 `static mut`、`thread_local!`、全局 `Mutex` 或随意创建的单例替代 GPUI 状态系统。
- 只把确实跨窗口、跨 feature、与应用生命周期一致且具有唯一实例的状态定义为 `Global`。
- 不要因为传参不方便就把局部状态升级为 `Global`；只在部分组件之间共享的状态应使用 `Entity<Model>`。
- 把 Global 实现类型保持为私有，并通过有业务语义的读取和更新 API 限制访问范围；不要让任意调用方直接修改全部字段。
- 在应用启动阶段集中注册 Global，读取前确保初始化已经完成。

```rust
use gpui::{App, Global, ReadGlobal as _, UpdateGlobal as _};

#[derive(Default)]
struct ThemeState {
    selection: ThemeSelection,
}

impl Global for ThemeState {}

pub fn init(cx: &mut App) {
    ThemeState::set_global(cx, ThemeState::default());
}

pub fn selection(cx: &App) -> &ThemeSelection {
    &ThemeState::global(cx).selection
}

pub fn set_selection(value: ThemeSelection, cx: &mut App) {
    ThemeState::update_global(cx, |state, cx| {
        state.selection = value;
        cx.refresh_windows();
    });
}
```

### 避免复制全局状态

- 需要读取时直接从 `App` 获取引用，不要把整份 Global 克隆到每个组件字段中。
- 不要为了绕过借用关系复制大型集合、主题、注册表或配置树；在读取或更新闭包内完成操作，只提取渲染真正需要的小型派生值。
- 只有在跨越 `'static` 回调或异步边界时，才按需复制 `Copy` 值，或克隆 `Entity`、`WeakEntity`、`SharedString`、`Arc` 等廉价句柄。
- `gpui_component::Theme` 已经是 Global。组件直接通过 `cx.theme()` 读取 token；自定义主题模块只保存主题 ID、模式等最小选择状态，不复制完整 `Theme` 或 `ThemeRegistry`。

### 处理 Global 更新

- 修改 Global 会通知 Global observer，但依赖它的 Entity 仍需建立明确观察关系并在可见结果变化时调用 `cx.notify()`。
- 主题、语言等确实影响所有窗口的状态可以调用 `cx.refresh_windows()`；不要用全窗口刷新代替局部 Entity 通知。
- 需要响应 Global 的组件使用 `cx.observe_global::<T>(...)`，并按组件生命周期保存或明确 `.detach()` 返回的 `Subscription`。

## 2. 状态归属与最小化

按以下顺序判断状态所有者：

1. 只有组件 A 使用的状态放进 A。
2. 只有组件 B 使用的状态放进 B。
3. 父组件 C 只负责组合 A 和 B，且不读取或协调它们的状态时，C 只保存必要的 `Entity<A>`、`Entity<B>` 句柄。
4. A 和 B 需要共享、同步或互相影响状态时，把相关状态提升到最近共同父组件 C，或提取为独立 `Entity<Model>`。
5. 只有状态确实属于整个应用时，才继续提升为 Global。

```rust
struct Container {
    a: Entity<A>,
    b: Entity<B>,
}

struct A {
    state_a: AState,
}

struct B {
    state_b: BState,
}
```

- `Container` 持有子 Entity 是为了组合和生命周期，不代表它拥有子组件的业务状态。
- 每份状态只设置一个清晰的事实来源，不在父子组件、Global 和 Model 中重复保存同一数据。
- 能由现有状态计算得到的选中数量、过滤结果、是否可执行等值，在读取或渲染时派生，不要冗余存储。
- 不要把整个窗口设计成包含全部 feature 状态的巨大 Entity。

## 3. 组件抽取范围

- 先检查 `nexora::desktop`、当前 feature 已有封装和 `gpui-component` 是否已有满足需求的组件；已有组件禁止重复实现。
- 只在一个位置使用的简单 UI 可以保留为 feature 内的局部渲染函数或 Element 组合。
- 相同语义在两个或更多位置使用时，必须抽成组件，避免复制布局、状态逻辑、交互和主题样式。
- 只在单个 feature 内复用或增强的组件放在该 feature 模块；只有多个 feature 已经真实复用时，才提升到共享 UI 边界。
- 不要仅因一小段 `div()` 相似就制造公共组件；抽取对象必须具有稳定的产品语义或交互契约。
- 重复组件存在少量合法差异时，使用命名清晰的 variant 或 builder API，不要堆积难以理解的布尔参数。
- 分组语义使用官方 `GroupBox::fill()` / `GroupBox::outline()`，普通排版使用布局容器；不得恢复
  公共 `Card`、`FormItem` 或 `LabeledControl` 视觉 API。

## 4. 选择组件层级

按最低必要层级实现：

| 场景 | 实现方式 | 状态规则 |
| --- | --- | --- |
| 一次性静态布局 | `div()`、`h_flex()`、`v_flex()` 等 Element 组合 | 不创建独立状态 |
| 可复用但无独立状态 | `#[derive(IntoElement)]` + `RenderOnce` | 字段只保存本次渲染需要的 props |
| 有独立状态或生命周期 | `Entity<T>` + `Render` | 状态由 Entity 管理 |
| 自定义布局、绘制或极致性能 | 实现底层 `Element` | 明确 layout、prepaint、paint 状态 |

- 能用 `RenderOnce` 表达时不要创建 Entity。
- 出现独立变化、独立刷新、焦点、输入、订阅、异步任务、被观察需求或独立生命周期时，才使用 `Entity + Render`。
- `div()`、`h_flex()`、`v_flex()` 只用于普通布局和内容组合，不得模拟已有语义控件及其 hover、focus、selected、disabled、loading 或键盘导航。
- 只有 `nexora::desktop`、`gpui-component` 原生组件、组合、builder API 和 feature 私有薄包装都无法满足，并且存在可验证的布局、绘制、性能或交互能力缺口时，才实现底层 `Element`。

## 5. 保持单向数据流

- 父组件向子组件传递 props、共享 `Entity<Model>` 或调用语义明确的方法。
- 子组件通过 `EventEmitter + cx.emit()` 向上报告用户意图或业务事件。
- 使用 `cx.observe()` 表达“被观察 Entity 的状态发生了变化”。
- 使用 `cx.subscribe()` 表达“被观察 Entity 发生了某种类型化事件”。
- 修改影响当前 Entity UI 的状态后调用 `cx.notify()`。
- 跨组件命令和快捷键使用类型化 Action，不要通过查找其他组件后直接修改其私有字段。
- 避免组件之间互相持有并任意更新对方，防止形成隐式双向依赖。

可以按下面的语义区分：

> `notify/observe` 表达“状态变了”；`emit/subscribe` 表达“发生了什么”。

## 6. 保持 Render 无副作用

`render()` 只执行：

- 读取当前状态和 Global；
- 计算轻量派生值；
- 构造 Element 树；
- 注册只作用于本次 Element 树的轻量事件处理器。

禁止在 `render()` 中：

- 创建需要长期存在的 Entity；
- 发起网络、数据库或文件请求；
- 建立永久订阅；
- 修改业务状态或 Global；
- 启动无法跟踪、取消或归属生命周期的异步任务。

在构造函数、初始化方法或明确的业务方法中创建子 Entity、订阅和任务。不要依赖 `render()` 只执行一次。

## 7. 控制 Entity 与刷新边界

- 把 Entity 边界设计成独立状态、生命周期和刷新边界。
- 不要为每个纯展示图标或文字创建 Entity，也不要让根 Entity 承担所有局部状态。
- 状态变化后只通知真正需要更新的 Entity；避免无条件、高频 `cx.notify()` 或 `cx.refresh_windows()`。
- 同一层级不要重复渲染具有相同 Entity 身份的 View，否则内部 Element 状态、焦点和滚动位置可能冲突。

## 8. 保持 ElementId 稳定

- 所有交互元素、列表项、滚动区域和需要跨帧状态的 Element 使用稳定且唯一的 `ElementId`。
- 列表项优先使用稳定业务 ID，不要使用数组当前位置或每次渲染变化的临时值。
- 同一 ID 作用域内不得出现重复 ID。
- 不要使用时间戳、随机数或不断递增的渲染计数生成 Element ID。

## 9. 管理回调、任务和订阅生命周期

- 事件回调操作当前组件时优先使用 `cx.listener()`。
- 异步任务捕获组件时使用 `WeakEntity<T>`，执行更新前先升级，避免任务永久持有组件。
- 需要随组件销毁而取消的 `Task` 和 `Subscription` 保存在组件字段中。
- 只有明确希望监听持续到相关 Entity 或窗口销毁时，才调用 `.detach()`；不要把 `.detach()` 当作消除编译警告的手段。
- 避免组件之间用强 `Entity` 句柄形成循环持有。
- 后台线程或 background executor 只执行计算、文件和网络 I/O；所有 UI 状态更新回到 GPUI `App`/`Context` 所在的前台事件循环。

## 10. 设计语义化组件 API

- 使用消费 `self` 并返回 `Self` 的 builder 风格，保持与 GPUI 生态一致。
- API 描述业务和交互语义，例如 `disabled`、`loading`、`toolbar`、`filter_bar`，不要暴露组件内部布局节点。
- 外部尺寸和位置由父组件控制，组件内部负责自己的内容结构。
- 颜色、边框、字号、间距和状态样式使用主题 token 或组件库语义 API，不要写散落的固定视觉值。
- 控件支持对应的焦点、键盘操作、可访问角色与可访问名称。
- 不要允许调用方任意覆盖所有内部样式；只开放确实稳定且有复用价值的定制点。

## 11. 增强 gpui-component

基础组件不满足业务需求时，创建独立增强组件，不修改、复制或破坏官方组件：

1. 先确认官方组件和扩展点确实无法表达需求。
2. 默认在当前 feature 中使用组合薄包装官方组件，保持业务状态和作用范围局部化。
3. 只有多个 feature 已经真实复用同一增强语义时，才提升到共享 UI 边界；共享后可以沿用原组件名称，让调用方使用 `ui::DataTable::new(...)`。
4. 保留原组件构造方式、默认行为和项目已使用的基础 builder API。
5. 只添加增强 API，例如 `.toolbar(element)`、`.filter_bar(element)`。
6. 共享增强层只处理通用 UI 能力，不混入某个 feature 的请求、权限或业务状态。
7. 官方组件升级后优先适配包装层，不把兼容工作扩散到所有调用方。

概念结构：

```rust
pub struct DataTable<D> {
    inner: gpui_component::table::DataTable<D>,
    toolbar: Option<AnyElement>,
    filter_bar: Option<AnyElement>,
}

impl<D> DataTable<D> {
    pub fn toolbar(mut self, toolbar: impl IntoElement) -> Self {
        self.toolbar = Some(toolbar.into_any_element());
        self
    }

    pub fn filter_bar(mut self, filter_bar: impl IntoElement) -> Self {
        self.filter_bar = Some(filter_bar.into_any_element());
        self
    }
}
```

- 不要仅依靠 `Deref` 声称 API 兼容：官方组件中消费 `self` 并返回原类型的 builder 方法会丢失包装类型。
- 显式转发项目使用的基础 builder 方法，并把返回的官方组件重新放回包装类型，确保基础方法和增强方法能够任意合理组合。
- 为“仅使用原能力”和“使用增强能力”分别添加集成测试，防止包装层改变默认行为。
- 无法保持兼容时使用明确的新名称，并在实现前说明差异；不要用同名类型静默改变语义。

## 12. 应用单例与多窗口承载

- Nexora 默认启用应用身份级单例门禁，并在同一个 GPUI 事件循环中通过 `App::open_window`
  承载主窗口、额外 Shell、唯一 Settings 和注册 Window。应用级 Account、Preferences、Updater、
  业务模型和缓存使用唯一 Global 或共享 Model，不为每个窗口复制。
- 窗口承载严格保持单个应用进程；不存在子进程窗口配置或第二套窗口 IPC。
- 不要把单进程与关闭多窗口混为一谈；额外 Shell、Settings 和注册 Window 仍由
  `App::open_window` 在运行期创建。冷启动只恢复主窗口几何并打开 `initial_path`。
- 不要为了打开第二个窗口自行再次启动当前二进制、调用第二次 `Application::run` 或复制 Nexora
  的进程协调协议。Shell、SettingsWindow 和注册 Window 继续走统一注册表与运行时生命周期。
- 同进程包含多个 Shell 时，`Context<T>::navigate` 路由到当前 Entity 所在窗口的 Shell；
  `App::navigate` 路由到活动 Shell；无法确定来源或活动 Shell 时回退到主 Shell。窗口路由仍按
  注册表打开独立原生窗口。
- 不要在普通子线程中直接创建或操作窗口。GPUI 的 `App` 和窗口创建属于前台应用事件循环；
  子线程只负责准备数据或发出打开窗口的请求，再回到 `App` 上下文执行 `open_window`。
- 每个 Shell 和窗口保留自己的根 Entity、局部输入状态、焦点、订阅和任务；应用级 Account、
  Preferences、Updater 与窗口组协调使用唯一 Global 或框架运行时，不为每个窗口复制。
- 不得在 Nexora 之外为普通窗口设计多进程边界或维护第二套窗口进程协议。

## 13. GPUI 测试策略

普通 Rust 测试和 GPUI 集成测试必须按被测行为依赖的运行环境分层：

- 纯数据转换、解析、排序、配置 builder、状态机和不依赖 GPUI 上下文的业务逻辑继续使用普通 `#[test]`。
- 凡是被测行为依赖 `App`、`Window`、`Entity`、`Global`、`Action`、GPUI 前后台执行器或事件循环，优先使用 `#[gpui::test]`，并让测试函数接收 `&mut TestAppContext`。
- 是否使用 `#[gpui::test]` 由测试执行的行为决定，不由源码是否导入了 GPUI 类型决定。仅为了实现 trait 而在类型签名中出现 `App`、`Window` 或 `Entity`，但实际断言只检查纯配置时，仍使用普通 `#[test]`。
- GPUI 测试应通过 `TestAppContext::new`、`update_entity`、`read_entity`、`add_window`、`open_window` 等测试 API 操作状态和窗口，不要手工构造假的 `App`、`Window` 或绕过 Entity 所有权模型。
- 测试 `gpui-component` 组件、主题或其他 Global 前，先在 `cx.update(...)` 中完成 `gpui_component::init` 及被测模块初始化，保证测试环境与真实应用启动顺序一致。
- 测试 Action 时通过 GPUI Action 注册与分发链路触发，不要只直接调用 Action 处理函数后声称快捷键或菜单集成已经得到覆盖。
- 测试 GPUI 异步任务、订阅或延迟副作用时，使用 `TestAppContext` 提供的确定性执行器和 `run_until_parked` 等能力推进任务，不要使用真实休眠等待。
- `TestAppContext` 使用模拟平台，适合状态、事件、窗口生命周期、布局和绘制流程测试；需要验证真实平台像素输出时，再使用 GPUI 提供的 `VisualTestAppContext` 或项目约定的截图测试。
- 所有测试继续遵守 `rust-code-style`：放在 crate 的独立 `tests/` 目录中，不在生产源码内增加 `#[cfg(test)]` 测试模块。

示例：

```rust
use gpui::{AppContext as _, TestAppContext};

#[gpui::test]
fn feature_entity_updates_inside_gpui(cx: &mut TestAppContext) {
    let feature = cx.new(|_| Feature::default());

    cx.update_entity(&feature, |feature, cx| {
        feature.select(ItemId::Home);
        cx.notify();
    });

    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.selected()),
        ItemId::Home
    );
}
```

## 14. 实现与审查流程

按顺序执行：

1. 检查 workspace 当前 GPUI、`gpui-component` 版本和已有本地封装。
2. 形成组件选型记录；已有标准组件时先替换或复用，不继续手写控件。
3. 判断每份状态属于组件、共同父级、共享 Model 还是 Global。
4. 删除重复状态和能直接计算的派生状态。
5. 根据选型记录决定原生组件、组合、feature 私有薄包装、`RenderOnce`、`Entity + Render` 或底层 `Element`。
6. 设计 props 向下、event 向上以及 observe/subscribe 关系。
7. 明确 Entity、Task、Subscription、窗口和后台工作的生命周期。
8. 使用稳定 Element ID、主题 token、焦点和可访问语义。
9. 修改可见状态后只触发必要范围的刷新。
10. 为共享组件、增强组件、状态转换和多窗口入口添加独立集成测试。
11. 根据测试是否依赖 GPUI 运行环境，选择普通 `#[test]` 或 `#[gpui::test] + TestAppContext`。

## 检查清单

- [ ] 真正的全局状态是否实现 `Global`，且没有复制完整 Global？
- [ ] 局部状态是否留在最近使用者，共享状态是否只提升到最近共同父级？
- [ ] 是否删除了可计算或重复保存的状态？
- [ ] 编码前是否完成组件选型记录？
- [ ] 是否先检查了 `nexora::desktop`、仓库已有封装和 `gpui-component` 现有组件？
- [ ] 已有标准组件时，是否避免了手写重复控件？
- [ ] 组件真的需要 Entity，还是 `RenderOnce` 已足够？
- [ ] 父子通信是否保持 props 向下、event 向上？
- [ ] `render()` 是否没有长期副作用？
- [ ] 状态变化后是否只刷新必要 Entity？
- [ ] Element ID 是否稳定唯一？
- [ ] Task、Subscription 和 Entity 句柄是否有清晰生命周期？
- [ ] 增强组件是否保持官方默认行为并只增加 API？
- [ ] feature 私有增强是否留在当前 feature，只有真实跨 feature 复用时才进入共享 UI 边界？
- [ ] 纯自定义 `Element` 是否有可验证的布局、绘制、性能或交互能力缺口证据？
- [ ] 多窗口是否由同一进程中的 `open_window` 创建？
- [ ] 是否支持焦点、键盘操作和无障碍语义？
- [ ] 依赖 App、Window、Entity、Global、Action 或 GPUI 异步调度的测试是否使用了 `#[gpui::test] + TestAppContext`？

## 官方参考

- [GPUI Global](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/global.rs)
- [GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)
- [GPUI Test support](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/test.rs)
- [Ownership and data flow](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/_ownership_and_data_flow.rs)
- [View implementation](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/view.rs)
- [Zed UI components](https://github.com/zed-industries/zed/tree/main/crates/ui/src/components)
- [gpui-component 组件文档](https://longbridge.github.io/gpui-component/zh-CN/docs/components/)

[`Global`]: https://github.com/zed-industries/zed/blob/main/crates/gpui/src/global.rs
