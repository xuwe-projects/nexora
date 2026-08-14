# CrudPanel 表格展示约束与分页页码设计

## 背景

`CrudPanel` 使用锁定版本的 `gpui-component::table::DataTable`。该组件当前只暴露统一的
`Size`，表头与正文行共同通过 `Size::table_row_height()` 取高，没有独立的正文行高入口。
因此，把头像、姓名、用户名等多个业务值纵向合并进一个正文单元格时，固定行高可能裁剪内容；
若通过提高统一行高规避，又会同时放大表头，效果不符合本项目预期。

本次不扩展正文行高，也不修改上游组件，而是暂时禁止 `CrudTableRow` 合并展示列，并由
`nexora lint` 做强制检查。同时统一状态字段的官方组件选择和语义颜色。

现有 `CrudPanel` 已使用官方 `Pagination`，且没有开启 `compact`。锁定版本的分页算法在
`total_pages <= 1` 时不生成页码项，所以单页只剩“上一页/下一页”；多页时会生成页码和省略号。
本次补齐单页的当前页按钮，并把多页最大可见页码数显式固定为 5，避免默认值变动改变产品行为。

## 目标

- `CrudTableRow` 的每个数据列只展示该列所属字段，不允许头像与姓名、姓名与用户名等合并列。
- 布尔或开关型状态在表格中统一使用基于官方 `Switch` 的 `TableSwitchCell`。
- 其他业务状态统一使用官方 `Tag` 的填充样式，并按状态语义选择颜色。
- `nexora lint` 对上述规则做不可豁免的错误检查。
- `CrudPanel` 默认分页在单页时显示当前页 `1`，多页时显示最多 5 个页码及官方省略号菜单。
- 表头继续使用现有统一尺寸，不新增表头或正文行高配置。

## 非目标

- 不修改 `gpui-component` 的 `DataTable` 或 `Pagination` 公共 API。
- 不新增 `CrudPanel` 正文行高、表头行高或合并列逃生开关。
- 不自动推断所有领域枚举的业务含义；非布尔状态由字段属性显式声明。
- 不用自绘元素重新实现 Switch、Tag 或多页分页。
- 不改变后端分页协议、`CrudListState`、页码起始值或加载请求流程。

## 组件选型记录

按仓库的可见 UI 选型顺序检查后，采用以下方案：

1. 布尔状态复用 `nexora::desktop::TableSwitchCell`。它是当前仓库对
   `gpui_component::switch::Switch` 的表格私有薄封装，已经提供受控值、权限、loading 和
   回调边界。
2. 非布尔状态直接使用 `gpui_component::tag::Tag` 及其语义 `TagVariant`，不增加业务 Tag
   组件。
3. `total_pages > 1` 继续使用官方 `gpui_component::pagination::Pagination`，显式设置
   `visible_pages(5)`。
4. 官方 Pagination 在单页不提供页码项，也没有插入自定义页码的 slot。仅在
   `total_pages == 1` 时，由 `CrudPanel` 私有分页渲染 helper 使用官方 `Button`、官方图标和
   现有尺寸拼成“上一页 / 1 / 下一页”。该 helper 只补官方组件缺失的单页分支，不成为新的
   公共分页控件。

## `CrudTableRow` 字段契约

### 状态声明

为 `#[nexora(column(...))]` 增加无值标记 `status`：

```rust
#[derive(Clone, nexora::CrudTableRow)]
struct UserRow {
    #[nexora(row_id)]
    id: UserId,

    #[nexora(column(title = "姓名", render = Self::render_name, text = Self::name_text))]
    name: String,

    #[nexora(column(
        title = "状态",
        status,
        render = Self::render_status,
        text = Self::status_text
    ))]
    status: UserStatus,

    #[nexora(column(
        title = "启用",
        status,
        render = Self::render_enabled,
        text = Self::enabled_text
    ))]
    enabled: bool,
}
```

- 所有承担业务状态语义的列都必须声明 `status`。
- `status` 只能出现在 `column(...)` 上，不能与 `skip` 或 `row_id` 共用。
- 状态列必须同时声明 `render` 和 `text`：前者保证可见组件受控，后者保证导出、复制或非视觉
  场景仍有稳定文本。
- 派生宏只解析、校验并保留这项元数据，不根据状态自动生成业务渲染器。
- `nexora lint` 对明显的状态字段名（`status`、`state`、`enabled`、`disabled`、`active`、
  `locked` 及其常见后缀）和以 `Status` / `State` 结尾的字段类型检查遗漏的 `status` 标记。
  这项检查报错，提示作者显式声明业务语义，而不是静默猜测颜色。

### 单列单字段

对每个带 `render` 的数据列，lint 解析同一 crate 内可解析的渲染函数，并建立“所属字段”关系：

- 可以读取本列所属字段，进行格式化、枚举匹配或映射为显示文本。
- 可以读取 `#[nexora(row_id)]` 字段，但该值只能进入 `TableSwitchCell::new` 的稳定元素 ID；
  不能作为第二项可见内容。
- 禁止读取任何其他行字段。
- 禁止把完整的 `self`、完整行值或其引用传入无法继续分析的 helper、闭包或 trait 方法。
- 可以把本列字段传给纯格式化 helper；lint 必须能确认实参不是完整行且没有其他行字段。
- 禁止使用 `v_flex()`、`.flex_col()`、换行/自动折行容器或多个行来源子项构造多行单元格。
- 操作列、选择列等 `CrudTableDelegate` 框架列不属于数据列，不受“所属字段”检查影响。

渲染函数无法解析、存在宏展开后才可见的字段访问，或分析不能证明单列单字段时，lint 采用
保守策略报错。暂不提供 allow 注解；需要复杂展示时拆成独立数据列。

## 状态展示规则

### 布尔与开关型状态

`status` 字段类型为 `bool`，或字段表达明确的 true/false、on/off、enabled/disabled 语义时：

- 渲染器必须使用 `nexora::desktop::TableSwitchCell`。
- 二元领域枚举或远端 on/off 值在进入表格行模型时先适配为 `bool`；表格层不靠枚举名称猜测
  是否属于二元状态。
- 标准 CRUD 表格中不直接使用裸 `Switch`，避免绕过权限、loading 与受控值约束。
- 行 ID 只用于 `TableSwitchCell::new` 的稳定 `ElementId`。
- `allowed`、`loading` 和 `on_change` 仍由最接近业务请求的 Entity 提供；单元格不持有乐观值。
- 只读场景可以省略 `allowed` 或 `on_change`，此时现有包装会显示禁用的官方 Switch。

### 非布尔业务状态

非布尔 `status` 字段必须使用官方 `Tag`，且只能使用默认填充样式。允许的语义映射如下：

| 语义 | `TagVariant` / 构造器 | 示例状态 |
| --- | --- | --- |
| 中性终态或未开始 | `Secondary` / `Tag::secondary()` | 草稿、未开始、已取消、已关闭 |
| 等待外部条件 | `Info` / `Tag::info()` | 等待、排队、待审核 |
| 正在主动执行 | `Primary` / `Tag::primary()` | 处理中、运行中、同步中 |
| 成功或正常可用 | `Success` / `Tag::success()` | 成功、已完成、已启用 |
| 需要注意但未失败 | `Warning` / `Tag::warning()` | 暂停、重试中、部分成功 |
| 失败或异常终态 | `Danger` / `Tag::danger()` | 失败、已拒绝、已过期、异常 |

约束如下：

- 禁止 `.outline()`；状态 Tag 必须保留默认填充变体。
- 禁止 `Tag::custom(...)`、`TagVariant::Custom` 和硬编码 HSLA 颜色。
- 状态列禁止 `Tag::color(...)` / `TagVariant::Color`；`ColorName` 只留给分类标签，不表达状态。
- 渲染器必须在可静态分析的本地 `match` 或等价分支中完成状态到语义变体的映射。
- 一个具有多个业务状态分支的字段不得把所有分支映射成同一变体；至少包含两个与业务语义
  对应的变体。单一状态新类型可以只用一个变体。
- 状态文案由业务本地化层提供；颜色只表达语义，不编码权限或交互能力。

## Lint 设计

新增四个不可豁免的 lint 错误：

| lint 名称 | 触发条件 |
| --- | --- |
| `nexora::crud_table_merged_column` | 列渲染器读取其他展示字段、泄露整行、构造纵向/多行内容，或无法证明单列单字段 |
| `nexora::crud_table_boolean_status_without_switch` | 布尔/开关状态没有使用 `TableSwitchCell`，或直接使用裸 `Switch` |
| `nexora::crud_table_status_without_tag` | 非布尔状态没有使用官方 `Tag`，或明显状态字段缺少 `status` 声明 |
| `nexora::crud_table_invalid_status_tag` | 使用 outline、自定义/分类颜色、全状态统一颜色，或状态映射无法静态分析 |

### 分析流程

1. 扫描派生 `CrudTableRow` 的命名结构体，记录 row ID、列字段、字段类型、`status` 和 `render`
   路径。
2. 解析 `Self::method`、同模块自由函数和可确定的本地 helper；建立函数调用图并设置递归保护。
3. 对表达式做字段来源追踪，区分所属字段、row ID、其他字段与整行值。
4. 对 UI 构造调用做组件追踪，识别 `TableSwitchCell`、`Switch`、`Tag`、Tag 构造器、
   `with_variant`、`outline`、纵向布局与折行调用。
5. 对状态分支收集所有可能的 `TagVariant`，校验允许集合和语义多样性。
6. 在诊断中同时指出列字段、违规访问或组件调用位置，并给出拆列、Switch 或语义 Tag 的
   直接修改建议。

lint 不依赖运行时反射，也不尝试跨依赖 crate 反编译函数体。若公共 helper 需要被 CRUD
渲染器复用，应保持输入为单个字段值，并在调用点完成 Switch/Tag 的可见组件选择。

## 分页设计

### 展示规则

- `total_pages == 1`：显示禁用的上一页按钮、选中态页码 `1`、禁用的下一页按钮。
- `total_pages > 1`：使用官方 `Pagination` 的普通模式，并显式调用 `visible_pages(5)`；超出
  窗口的页码由官方省略号菜单处理。
- 当前页始终显示选中态；不增加“快速跳页”输入框。
- `loading == true` 时，上一页、下一页、页码和省略号入口全部禁用。
- 页码、上一页和下一页最终都调用现有 `go_to_page`，沿用页码边界校验和加载流程。
- `total_pages` 继续由现有总数与 page size 计算，并维持至少为 1 的不变量。
- 分页尺寸继续跟随 `CrudPanel` 当前 `Size`；本次不改变表格或表头高度。

### 私有渲染边界

在 `crates/ui/src/crud_panel.rs` 内提取私有分页渲染 helper：

- 多页分支返回官方 `Pagination`。
- 单页分支只组合官方 `Button` 与官方图标，并复用官方 Pagination 当前使用的 tooltip、尺寸、
  disabled 和 active 视觉语义。
- helper 不持有页码状态，不创建 Entity，不发请求；所有状态来自 `CrudPanel`，点击只上报页码。
- 不从 `crates/ui` 导出新公共类型，避免形成与上游 Pagination 竞争的第二套 API。

## 兼容性与迁移

- `status` 是新增的可选字段属性；普通列的宏生成代码不变。
- 现有带明显状态语义的 `CrudTableRow` 需要添加 `status`，并按规则迁移其渲染器。
- 现有合并列必须拆成多个字段列；不提供临时 allow 或 feature flag。
- 现有分类 Tag 不标记为 `status`，可以继续使用 `Tag::color(ColorName)`。
- Pagination 公共构造和回调签名不变；调用方不需要迁移。

## 文档与规则同步

实施时同步更新：

- 根 `AGENTS.md` 的 CRUD 表格展示规则。
- `.agents/skills/desktop-ui-component-selection/SKILL.md` 及脚手架模板副本。
- `.agents/skills/gpui-component/SKILL.md` 及脚手架模板副本中与表格、Switch、Tag、Pagination
  有关的选择规则。
- `crates/cli/LINTS.md` 的四条 lint 名称、触发条件、失败与通过示例。
- 中英文桌面组件文档和 unreleased changelog。
- CrudTableRow 派生宏 rustdoc 中的 `status` 属性说明。

## 测试与验证

实现遵循测试先行，先添加失败用例，再完成最小实现。

### 派生宏测试

- `status` 标记可正确解析并与已有列参数组合。
- `status` 与 `skip` / `row_id` 的非法组合编译失败。
- 状态列缺少 `render` 或 `text` 时编译失败。
- 既有不含 `status` 的普通列生成结果保持不变。

### Lint fixture

- 头像与用户名、姓名与用户名的合并渲染失败。
- 只读取所属字段通过；row ID 仅用于 `TableSwitchCell` ID 时通过。
- 把整行传给 helper、使用纵向布局或自动折行失败。
- bool 状态使用普通文本、Tag 或裸 Switch 失败，使用 `TableSwitchCell` 通过。
- 非 bool 状态使用文本、outline Tag、Custom/Color Tag 或统一语义颜色失败。
- 非 bool 状态按至少两个语义变体映射通过；单状态新类型的单一变体通过。
- 分类 Tag 未声明 `status` 且使用 `Tag::color` 通过。
- 明显状态字段遗漏 `status` 失败。

### CrudPanel GPUI 测试

- 总页数为 1 时可查询到上一页、当前页 `1`、下一页，三者顺序正确，当前页为选中态。
- 单页上一页和下一页不可触发加载。
- 总页数大于 1 时出现数字页码；较多页时使用官方省略号入口，最多显示 5 个页码按钮。
- 点击数字页码、上一页和下一页都进入现有 `go_to_page` 路径。
- loading 时所有分页交互项禁用。
- Small、Medium 等现有尺寸下分页仍使用对应官方尺寸。

### 命令验证

至少执行：

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test -p nexora-macros
cargo test -p cli
cargo test -p ui
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo run -p cli -- lint --workspace . --deny-warnings
```

## 验收标准

- 新的 `CrudTableRow` 合并数据列无法通过 `nexora lint`，且诊断能定位违规字段访问。
- 布尔状态统一呈现官方 Switch；其他状态统一呈现填充 Tag，并具有符合状态语义的颜色差异。
- 分类标签仍可使用 `ColorName`，不会被误判为业务状态。
- 默认 `CrudPanel` 在只有一页时显示页码 `1`，多页时显示数字页码和官方省略号行为。
- 表头行高、正文行高、分页数据协议和现有加载生命周期均未改变。
- 宏、lint、UI 测试、严格 Clippy 与仓库 lint 全部通过。
