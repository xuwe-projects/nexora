# 主 Shell + 用户列表 · 原型 → gpui 翻译映射

真实结构参照 `crates/nexora/examples/desktop_basic.rs`（Sidebar + CrudPanel + FormDialog）
与 `crates/ui`（SidebarRegion / CrudPanel / CrudPanelToolbar / TableHeaderCell / TableCell）。
这是**框架 Shell + 一个业务 Feature（用户）**的组合，翻译时按 AGENTS.md 的 Feature/组件拆分。

## 区块映射

| 原型区块 | gpui / gpui-component 落地 | 说明 |
| --- | --- | --- |
| 整窗 Shell 布局 | Nexora Shell（Sidebar + 主内容槽）自动托管 | 不自绘，由框架提供 |
| 侧边栏容器 | `gpui_component::sidebar::Sidebar` | 现成组件 |
| 品牌区 / 账号页脚 | `SidebarHeader` / `SidebarFooter` + `SidebarRegion` | 交互视觉由 SidebarRegion 自行声明（见 AGENTS.md）|
| 侧边栏搜索（过滤导航树） | `ApplicationOptions::sidebar_search(true)` | 已有能力，见下「两种搜索」 |
| 导航分组 + 条目 | `#[nexora(section=..., group=...)]` Feature 派生 + `NavigationGroup` | inventory 自动发现，勿维护第二套路由表 |
| 条目徽章（128/7）| `gpui_component::badge::Badge` | 现成组件 |
| **顶部 Feature 标签栏** | Shell 内置标签栏 + `ApplicationOptions::tab_style(ApplicationTabStyle::Tab)` | **框架已有能力**，见下 |
| 摘要卡（标题+描述+刷新） | `CrudPanel::new(title, content).description(...).refresh(id, loading, disabled, on_click)` | **刷新是 CrudPanel 内置能力**，outline 样式 + rotate-ccw 图标，固定在摘要卡右侧 |
| 工具栏卡（筛选行+操作行） | `CrudPanelToolbar::new().filter(..).action(..)` | filter 区放 `Input`/`Select`；action 区放 `Button`，**新建/导入/导出都在这里** |
| （可选）统计板块 | 作为 `content` 传入的普通元素，**不是 CrudPanel 固定结构** | 见下「可选块」 |
| 搜索输入 | `gpui_component::input::Input`（`cleanable(true)`）| 现成组件 |
| 筛选项（关键词/角色…） | **每项用 `LabeledControl`（标签在上、控件在下）** 包 `Input`/`Select` | 原型已改为带标签，勿用裸控件 |
| 角色/状态下拉 | `gpui_component::select::Select`（外层套 `LabeledControl`）| 现成组件，勿用 div 模拟 |
| 快捷筛选标签（状态 Tabs） | **可选**，`gpui_component::tab` | 见下「状态 Tabs 架构」 |
| 数据表格 | `gpui_component::table::Table` + `TableHeaderCell`/`TableCell` | 现成组件，列宽/顺序用 DataTableLayout 持久化 |
| 状态标签（正常/待激活/已停用）| `gpui_component::tag::Tag`（success/warning/muted 语义色）| 现成组件 |
| 行内更多操作（⋮）| `gpui_component::button::Button().ghost()` + `popover`/`menu` | 现成组件 |
| 分页 | `gpui_component::pagination::Pagination` + `table`/`virtual_list` 滚动位置 | 见下「滚动分页联动」 |
| 新建/编辑用户 | `nexora::desktop::FormDialog` + `FormDialogState` | 遮罩只盖当前 Panel，见 AGENTS.md |

## 窗口标题栏 / 平台控制（桌面应用必须）

原型加了平台维度（切换器 macOS / Windows），对应 gpui-component `TitleBar`
（`crates/…/title_bar.rs`，`login_gate.rs` 已用）的真实机制——**两平台完全不同**：

- **macOS**：交通灯是**系统原生绘制**，不是 app 画的。做法：`TitleBar::title_bar_options()` 返回
  `appears_transparent: true` + `traffic_light_position: (9,9)`；TitleBar 左侧保留
  `TITLE_BAR_LEFT_PADDING = 80px` 让内容避开原生灯。→ 交通灯浮在**窗口左上=侧边栏左上**，
  故侧边栏 header 需向下留白让开（原型 `data-os=macos` 时 sb-header `padding-top:40px`）。
  原型里的三个圆点仅为**示意**，真实由系统绘制。
- **Windows/Linux**：控制按钮是 **app 自绘**（`ControlIcon` 枚举 Minimize/Restore/Maximize/Close，
  各 35px，`WindowControlArea` 交系统处理点击；Linux 可 `on_close_window` 自定义）。
  → 位置在**主区右上端**（标签栏最右、全局按钮之后），close hover 变红。左侧只留 12px。
- 标题栏高度 `TITLE_BAR_HEIGHT = 34px`，整条是拖拽区。原型把**标签栏那一行兼作标题栏拖拽区**
  （合并省一行，现代应用通行做法）。

→ 翻译时：窗口用 `TitleBar::title_bar_options()` 建窗，Shell 顶栏用 gpui-component `TitleBar`
承载标签 + 全局按钮，平台差异由 `TitleBar` 自身处理，**不要手绘交通灯/控制按钮**。
登录页 (`LoginGate.title_bar(true)`) 已是这个模式，Shell 与设置窗口需对齐同一套。

## Shell chrome 四层结构（本轮改版）

采用 VS Code / Zed 模型：**全局工具栏独立成行、标签页下移**。侧边栏全高，右侧自上而下三层
（**面包屑已移除**——它多占一条带、显突兀；层级上下文由侧边栏高亮 + 标签标题表达已足够）：

1. **全局工具栏**（独立一行，兼标题栏拖拽区）——三区，各自可扩展：
   - **左**：现由侧边栏 header 内的收起按钮承担（不放全局栏，见下）。
   - **中 `gb-center`**：全局搜索 / 命令面板（居中，⌘K）。
   - **右 `gb-right`**：通知(红点)、检查更新、帮助 + **Windows 控制按钮**（贴窗口角）。
2. **标签栏**：**导航控制（后退 / 前进 / 刷新）放进 `Tabs` 的 prefix 槽** + Feature 页签 + 新建。
   （gpui-component `Tabs` 支持 prefix，落地时前进后退刷新就是 prefix 内容，不另拼容器。）
   - **刷新 = 重载当前 Feature**（这个框架里一个页面就是一个 `#[derive(Feature)]`）。落地：
     触发当前 Feature 重新拉取/重建其内容（重跑数据加载，而非重建整个窗口）。
   - **后退 / 前进 = Shell 导航历史**。**需确认框架 Shell 是否维护 back/forward 历史栈**；
     若无则属框架增强点（另立规格）。前进无历史时置灰。
   - 页头不再放刷新（与这里重复），已移除。
3. **内容区**：页头 + 快捷筛选 + 表格。

设计理由：全局操作与标签页语义不同、分行独立扩展；导航控制（前进后退刷新）是浏览器式标签的
天然搭档，放标签最前。去掉面包屑后 chrome 回到三层（globalbar 44 + tabbar 42 ≈ 86px），更克制。

**落地映射**：全局工具栏三区 = Shell 顶栏需要 leading / center / trailing 三个可扩展槽
（框架增强点，需与 Shell 维护者定 API）。全局搜索/命令面板见下。

**收起/展开导航（本轮）**：
- 收起按钮放在**侧边栏 header 内**（展开时品牌右侧、收起时 logo 下方），因为它控制的是侧边栏，
  按钮就该在侧边栏上——放全局栏会显得脱节。
- 宽度过渡动画：`transition: width 0.22s cubic-bezier(0.4,0,0.2,1)`。落地用
  `gpui_component::sidebar` 的 collapsed 能力（需确认该 rev 是否内置折叠 + 动画；若无动画则属增强点）。
- macOS 收起态 ≥72px 以容纳交通灯。

**两种搜索，别混（本轮澄清）**：
1. **侧边栏搜索**（`sidebar_search`）：只**过滤左侧导航树**。原型行为——展开态是输入框，打字实时
   筛选导航条目（含空分组自动隐藏）；**收起态点搜索图标 → 先展开侧边栏再聚焦输入框**
   （导航都塌成图标了，自然要先展开才能搜）。落地对应框架 `sidebar_search`，只影响导航过滤，
   不改路由/标签。
2. **全局搜索 / 命令面板**（⌘K，全局栏中间）：跨 Feature 搜一切 + 执行命令，弹居中 Dialog。
   - 落地：`gpui_component::dialog`（或 `sheet`）承载遮罩+定位，内部 `input` + `list` 分组结果
     （跳转 / 操作）。键盘：⌘K 开关、Esc 关、↑↓ 选择、Enter 执行 —— gpui Action + 键位绑定。
   - 全局能力（跨 Feature），状态归属 Shell 或独立 Global，不放单个 Feature。

1. **标签栏**（Feature 页签 + 右侧全局按钮区）
   - 左：`opened_tabs` / `pinned_tabs`，见下节。
   - 右：**全局按钮区**——窗口级、与当前页无关的操作，可自由扩展。原型示意：命令面板 ⌘K、
     通知(带红点)、检查更新、帮助。落地位置有两种选择：
     (a) 若框架 Shell 在标签栏预留了 header/trailing 槽 → 放这里（推荐，符合"全局"语义）；
     (b) 否则这些属于框架增强，需在 Shell 顶栏新增一个 trailing actions 槽（另立规格）。
     账号头像**不放这里**——它是 `SidebarFooter` 的固定位置（左下），避免重复。
2. **面包屑细带**：随激活标签变化的上下文（工作区 / 用户）。独立于内容、不随内容滚动、始终可见。
   用 `gpui_component::breadcrumb::Breadcrumb`。它与页头大标题不冗余：面包屑=层级位置，
   大标题=当前页是什么+规模。落地需 Shell 在标签栏与内容间提供一条 breadcrumb 带
   （若当前 Shell 无此结构，属框架增强）。
3. **内容区**：页头 + 快捷筛选 + 表格（见上文"视觉偏离"）。

→ 通知/更新/帮助这类**全局按钮**的诉求，本质是"Shell 顶栏需要一个可扩展的 trailing actions 槽"。
这是对框架 Shell 的合理增强点，翻译阶段需要和 Shell 维护者确认槽位 API。

## 顶部 Feature 标签栏（不要漏！）

主窗口顶部是**打开的 Feature 页签栏**，由 Nexora Shell 内置托管，对应真实实现：
- `ApplicationOptions::tab_style(ApplicationTabStyle::…)`：官方 5 种样式
  `Tab`(默认) / `Underline` / `Pill` / `Outline` / `Segmented`。原型用默认 `Tab`。
- `opened_tabs`（已打开路由）、`pinned_tabs`（置顶，排在前且不可关闭）、`active_route`（当前）。
- 支持拖拽重排、置顶、滚动到可见（`scroll_tab_into_view`）。
- 每个页签 = 一个 Feature，显示其 `icon` + `title`，可关闭；置顶页签显示 pin、无关闭 ×。

→ **翻译时不要自绘标签栏**，接 Shell 已有能力；样式只通过 `tab_style` 选择。原型里的图标/
关闭/置顶视觉仅为示意，真实由框架 + Feature 派生元数据驱动。

## 状态 Tabs 架构：可选 + on_request + 与筛选合并（本轮）

快捷筛选那排（全部/活跃/待激活/已停用）本质是 **状态 Tabs**，应按 Feature 配置驱动，而非写死：

- **可选渲染**：Feature 配置了「状态集」才渲染这排 Tabs；没配就不渲染（有些 Feature 没有状态维度）。
  原型当前写死了状态排作演示——落地时应改为 `if let Some(statuses) = feature.status_tabs()` 条件渲染。
- **每个状态一个数据来源**：切换状态需要拉该状态的数据，因此每个状态（或整个状态维度）对应一个
  **`on_request` 异步回调**。签名约为
  `on_request(Query { status, keyword, role, page, page_size }) -> Future<Output = Page<Row>>`。
- **与筛选框合并成一次查询**：状态 Tab 的选中值 + toolbar 里各 `LabeledControl` 的筛选值
  （关键词/角色/…）+ 分页状态（当前页、页大小），**合并为单一 `Query` 结构**，一次 `on_request`
  发出。不要状态和筛选各发一次请求。
- **状态归属**：`Query`（含当前状态、各筛选值、当前页）由 Feature 持有；Tab 切换、筛选变更、
  翻页都改这个 `Query` 并触发同一个 `on_request`；结果回填表格 + 更新分页器 + 更新各状态计数。
- **计数徽章**：状态 Tab 上的数字（96/8/24）通常由 `on_request` 的聚合结果或单独的 counts 查询提供。

→ 这套「可选状态 Tabs + on_request + 合并查询」是对标准 CrudPanel 的**能力增强**，需要框架/业务层
提供 `Query` 模型与 `on_request` 钩子。翻译时在 Feature 内落地这条数据流；若要复用，可上升为
CrudPanel 的可选配置（另立规格）。

## 滚动分页 ↔ 页码双向联动（本轮）

需求：既支持点底部分页器翻页，也支持**表格内部滚动翻页**，且两者状态始终一致——
滚到第 N 页，底部页码就高亮第 N 页；点第 N 页，表格滚到该页顶部。

原型实现（`app-shell/index.html` 脚本）：表体 `.tbody` 内部滚动；
- 点页码 → `tbody.scrollTo(pageTop(p))`，平滑滚动，期间抑制 scroll 回调避免抖动。
- 滚动 → `floor(scrollTop / (pageSize × rowH)) + 1` 反推当前页并高亮。
- 页码用"首/末/当前±1 + 省略号"策略。

落地映射：
- 分页器用 `gpui_component::pagination::Pagination`（受控 `current_page`）。
- 表格滚动用 `table` 或 `virtual_list` 的 `ScrollHandle` / 滚动事件；
  由 Feature 持有 `current_page` 状态，**同时驱动 Pagination 高亮和表格 scroll_to**，
  scroll 事件反算页码更新同一状态——单一状态源，两个入口都改它。
- 数据量大时优先 `virtual_list`（只渲染可见行），滚动位置换算需按行高/项高。
- 这是对标准 CrudPanel「点分页器翻页」的增强（叠加滚动翻页），属功能增强，翻译时在 Feature
  内实现状态联动即可，不改 CrudPanel 骨架。

## 视觉有意偏离 CrudPanel 默认外观（用户授权）

用户明确希望更现代，不必严格照 CrudPanel 的"三张卡"外观。当前原型的视觉决策：
- **只有表格是浮起的 Card**；页头、快捷筛选、工具栏都直接落在 canvas 上，靠留白+字体分层，
  不再给每块描边。焦点面唯一，更现代。
- **页头**：标题 + 计数徽章 + 内联操作（刷新图标钮 / 导出 / 新建）。
- **快捷筛选标签**（全部/活跃/待激活/已停用 + 计数）：把"统计"自然融入筛选，
  替代独立统计卡。
- **筛选 + 导入并为一行**。

→ 这些是**外观层**的偏离，**功能仍一一映射到 CrudPanel 能力**：刷新=`CrudPanel::refresh`，
新建/导入/导出=`toolbar.action`，筛选=`toolbar.filter`。翻译时二选一：
  (a) 若 CrudPanel 的默认布局够用，直接用，接受它的三卡外观；
  (b) 若要还原这套现代布局，需**扩展 CrudPanel**（新增"快捷筛选标签"槽、页头内联操作槽，
      并允许摘要/工具栏不套 Card），或为该 Feature 写一个薄布局包装。**这属于框架增强，需另立规格。**
  快捷筛选标签可用 `gpui_component::tab`（Underline 样式）落地。

## CrudPanel 能力参照（各页按需组合）

真实 `crates/ui/src/crud_panel.rs` 的固定结构只有三段：**摘要卡（标题+描述+可选刷新）→
可选工具栏卡 → 主内容区**。据此回答常见页面差异：

- **不需要统计板块**：统计不是 CrudPanel 的一部分，默认就没有。需要时把统计卡作为
  `content` 的一部分传入（在表格上方），不需要就不传。原型默认页已按真实结构**移除统计**。
- **不同的操作按钮（新建/导入/导出/批量…）**：全部通过 `toolbar.action(..)` / `.actions([..])`
  加入操作区，按添加顺序排列（源码文档明确点名「导入、导出」）。**主操作也在工具栏里**，
  不浮在页面右上角。无操作时不传，工具栏 action 行自动省略。
- **只有筛选没有操作 / 只有操作没有筛选**：`CrudPanelToolbar` 任一区域可单独提供；
  两个都空则整张工具栏卡省略（`is_empty()` → 不渲染）。
- **页面级刷新**：用 `CrudPanel::refresh(..)`（内置，摘要卡右侧）。注意与「筛选查询」区分——
  查询按钮应作为 toolbar action，避免页面刷新与查询语义混淆（源码 rustdoc 明确要求）。

## 层次感（本轮新增）

层次的表达机制就是 **`crates/ui` 的 `Card`**（`group_box` 底 + border + radius_lg + shadow_md）——
这与框架一致，不是新造轮子。三级明度台阶：
- **chrome**（侧边栏 `--sidebar-bg` / 标签栏 `--tab-bar-bg`）：最沉，是外壳。
- **canvas**（内容区 `--canvas`）：居中，是画布。
- **surface = Card**（摘要/工具栏/表格各是一张 Card，`--surface` + `--shadow-*`）：浮在画布上。

→ 翻译时：内容区(CrudPanel 的 `p_5` 容器)背景走 canvas 级；摘要、工具栏、表格都用 `Card`。
**关键落地缺口**：`Card` 现在取 `theme.tokens.group_box`，而当前 `nexora.json` 的 `group_box`
与 `background` 明度差很小（亮色 `#f7f8fa` vs `#ffffff`），卡片浮不起来。要让层次成立，
需在 nexora.json 里拉开 background(canvas) 与 group_box(surface) 的明度差，并确认 `shadow: true`。
这是配色定稿后同步主题时要一起改的一处。

## 说明

- 全部颜色来自 `theme.*` / `theme.tokens.*`（对应 palettes.css 变量）。
- 状态标签的圆点 + 底色用语义色 `success/warning/muted`，跨配色方案自动适配。
- 除标签栏外**几乎不需要自定义组件**：Shell、Sidebar、Table、Select、Pagination、Tag、Badge
  全是 gpui-component 现成件；账号页脚菜单的交互视觉由 SidebarRegion 显式声明。
