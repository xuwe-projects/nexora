# Nexora UI 原型工作区

本目录用于在翻译成真实 gpui 代码**之前**，用网页快速迭代系统的 UI/UX。
网页原型改起来快、看效果直接，定稿后再照着原型翻译成 gpui + gpui-component + 必要的自定义组件。

## 两阶段工作流

1. **阶段一 · 网页原型（本目录）**
   - 在 `screens/<界面名>/` 下用纯 HTML/CSS（必要时少量原生 JS）搭界面。
   - 只引用 `foundation/tokens.css` 里的 CSS 变量表达颜色、圆角、字号，**禁止裸十六进制色值**。
     原型和真实 gpui 主题共用同一套 Nexora 调色板，这样翻译时颜色一一对应。
   - 每个界面配一份 `mapping.md`，逐区块标注对应哪个 gpui-component 组件，
     或说明为什么需要自定义组件（见下方“组件映射规则”）。

2. **阶段二 · 翻译成 gpui**
   - 按 `mapping.md` 逐块落地，遵守 `AGENTS.md` 的 Feature/组件拆分与状态边界规则。
   - 能用 gpui-component 现成组件的地方绝不用 `div/flex` 重造。

## 预览与配色对比

- 打开 `prototype/index.html` 作为索引，进入各界面。
- 每个界面顶部有**切换器**：2 套内置配色（蓝紫 Linear[默认] / 中性无彩）× 明暗 × 平台(macOS/Windows)，
  选择存 localStorage 跨页保持。（slate/teal 已淘汰。）
- 配色定义在 `foundation/palettes.css`：中性骨架（背景/文字/边框/语义色）固定，
  各方案只覆盖强调色系，保证对比时唯一变量是主色。
- **真实主题真源仍是 `crates/theme/themes/nexora.json`，原型阶段不改动**；配色定稿后再同步。

## 铁律：每个窗口都必须有 TitleBar

原型里**每个窗口页面都必须包含 TitleBar**（对应 gpui-component `TitleBar`），且随场景不同而不同：
引入 `foundation/titlebar.css`，在标题栏区放 `.traffic`（左·macOS 交通灯）和 `.winctl`（右·Windows
控制）。macOS 交通灯占窗口左上约 65–80px，贴左上的内容必须让开；Windows 三键在右上、close hover 变红。
翻译时用 gpui-component `TitleBar`，平台差异交给它，**不手绘**。

## 组件映射规则（对齐 AGENTS.md）

翻译前每个 UI 区块按顺序判断，并在 `mapping.md` 记录结论：

1. `nexora::desktop` 或仓库已有业务封装（`crates/ui`：FormDialog、CrudTable、CrudPanel、Card、PanelHeader、Cascader 等）是否已提供？
2. 锁定版本的 `gpui-component` 是否有对应组件？（清单见下）
3. 能否通过 gpui-component 的组合 / builder / 薄包装满足？
4. 以上都不行，才允许纯 gpui 自定义 —— 且必须写清：查过哪些候选、具体缺什么行为、状态归属与作用域。

### gpui-component 现成组件清单（rev 0315556，本地校验）

按钮/输入类：`button`、`input`、`checkbox`、`radio`、`switch`、`select`、`combobox`、
`slider`、`color_picker`、`rating`、`form`
展示/容器类：`card`(仓库封装)、`group_box`、`accordion`、`collapsible`、`tab`、
`description_list`、`badge`、`tag`、`avatar`、`label`、`kbd`、`separator`
数据类：`table`、`list`、`searchable_list`、`tree`、`virtual_list`、`pagination`、`chart`、`plot`
导航/框架类：`sidebar`、`breadcrumb`、`title_bar`、`status_bar`、`dock`、`menu`、`native_menu`、`stepper`
浮层/反馈类：`dialog`、`sheet`、`popover`、`hover_card`、`tooltip`、`notification`、`alert`、
`progress`、`spinner`、`skeleton`
其他：`resizable`、`scroll`、`icon`、`link`、`text`、`setting`、`time`

> 有以上组件时禁止用 `div/h_flex/v_flex` 模拟其 hover / focus / selected / disabled /
> loading / 键盘导航行为。布局用 flex 没问题，模拟语义控件不行。

## 目录结构

```
prototype/
  README.md              本文件
  foundation/
    tokens.css           Nexora 设计 token 的 CSS 变量镜像（真源是 crates/theme/themes/nexora.json）
  screens/
    <界面名>/
      index.html         原型页面
      mapping.md         区块 → gpui-component 组件的翻译映射
```

## 预览方式

直接用浏览器打开 `screens/<界面名>/index.html` 即可；深色模式在 `<html>` 上加
`data-theme="dark"` 切换（或跟随系统）。
