# 登录门禁 · 原型 → gpui 翻译映射

真实实现位置：`crates/ui/src/login_gate.rs`（`LoginGate`，`#[derive(IntoElement)] + RenderOnce`）。
本界面属于框架级可复用组件，翻译时**修改现有 `LoginGate`**，不新建平行实现。

## 区块映射

| 原型区块 | gpui / gpui-component 落地 | 说明 |
| --- | --- | --- |
| 窗口根 900×640 | `div().relative().size_full()` + `bg(theme.background)` | 已有 |
| 顶部透明标题栏 | `gpui_component::TitleBar`（透明、无下边框） | 现成组件，勿用 div 模拟 |
| 左上 Logo + 产品名 | `img(logo)` + `div().text_xl().font_semibold()` | 已有。原型用字母块占位，真实用 `default_application_logo()` |
| 右上「检查更新」 | `Button::new().ghost().small().icon(IconName::CircleCheck)` | 现成 Button，可选渲染 |
| 右上「设置」 | `Button::new().ghost().small().icon(IconName::Settings2)` | 现成 Button |
| 左半装饰区 | `div` + `img(network_image)` 位图（明暗两套素材） | **注意分歧**，见下 |
| 右半标语/大标题/描述 | `div().text_*()` 文本层级 | 纯文本，布局用 v_flex |
| 主登录按钮 | `Button::new().primary().large().loading().disabled()` | 现成 Button，勿自定义 |
| 保持登录复选框 | `gpui_component::checkbox::Checkbox` | 现成组件，勿用 input 模拟 |
| 保护说明 | `Icon::new(IconName::CircleCheck)` + 文本 | 现成 Icon |
| 状态/恢复操作 | `Button`（`retry` + `ghost`） | 现成 Button，受 `can_retry_recovery` 控制 |
| 左下版本 | `div().text_sm().text_color(muted)` | 纯文本 |
| 右下隐私·帮助 | `footer_link()` = `Button::new().small().text()` | 已有薄封装 |

## 待确认的设计分歧（原型 vs 现状）

1. **配色（全局，非本页专属）**：原型已切到「Linear 风蓝紫」提案配色
   `prototype/foundation/tokens-linear.css`，主色 `#5b5bd6`（暗色 `#5b5fd6` + 白字，
   不再反转为浅底深字）。**真源 `crates/theme/themes/nexora.json` 尚未改动。**
   → 采纳后需同步改 nexora.json 的 Light/Dark 两组 `colors`，影响所有界面，需你确认。
2. **左侧装饰**：现状是明暗两张位图 `login-network[-dark].png`（`crates/ui/assets/`）。
   原型改成 **整窗统一背景 + 全幅淡出网格 + 少量节点/连线**，消除了原来 50/50 硬切的割裂感。
   → 翻译时不再用 `img` 位图；网格与节点用 gpui 绘制（`div` 背景渐变 + `quad`/自定义
   Element）。这是一处需要少量自定义绘制的区块，`mapping` 已标注其状态归属为 LoginGate 私有、
   无长期状态。**需你拍板是否走“代码绘制装饰”而非位图。**
3. **底部构建号**：新增 `Build <n>`，对应 updater 的 `current_build_number() -> u64`，
   与 `version` 并列展示（`Console 0.1.0 · Build 142`）。翻译时 `LoginGate` 需新增
   `build_number`/`build_label` 属性，由宿主从 updater 配置注入。
4. **认证块最大宽度**：现状 `440px`，原型收窄到 `340px` 让内容更聚拢。
5. **主按钮高度**：现状 50px，原型 48px；圆角沿用 token `--radius`(6px)。
6. 其余文案、层级、锚点均与现状一致，未改动交互契约。

## 不变的约束

- `LoginGate` 不持有认证状态，只暴露属性与回调 —— 翻译时保持这一职责边界。
- 颜色全部来自 `theme.*`（对应 tokens.css 变量），不引入新色值。
