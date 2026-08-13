---
name: nexora-ui-prototyping
description: >-
  在 prototype/ 目录用网页快速设计 Nexora 桌面应用的 UI/UX 原型，定稿后再翻译成 gpui +
  gpui-component。当用户要「设计原型 / 做原型 / 重做界面 / 调 UI/UX / 试配色 / 画个页面看看」
  这类桌面界面视觉设计任务时使用。设计原型时同时调用已安装的 design-system skill 作为设计清单。
---

# Nexora UI 原型设计

先在 `prototype/` 用网页把界面设计打磨好看，再照 `mapping.md` 翻译成 gpui。网页迭代快、看效果直接。

## 启动时必做

1. **同时使用 design-system skill**：`affaan-m-everything-claude-code-design-system`（已安装于
   `.claude/skills/`）。把它的六项清单（token / 组件 / Storybook / 无障碍 / 文档 / 团队采用）
   当作设计检查表，逐项对照。本 skill 提供 Nexora 专属的落地方式，两者配合使用。
2. **读现状对齐真源**，别凭空造：
   - 设计 token 真源：`crates/theme/themes/nexora.json`（已镜像到 `prototype/foundation/`）。
   - 组件铁律：`AGENTS.md`「遵守 GPUI 状态与渲染边界」——先查 gpui-component 现成组件再自定义。
   - 相关业务组件：`crates/ui`（CrudPanel/Card/FormDialog/SidebarRegion…）、Shell 能力在
     `crates/nexora/src/application.rs`（`ApplicationTabStyle` 等）。

## 目录与产物

```
prototype/
  index.html               画廊索引
  foundation/
    palettes.css           4 套配色候选（中性骨架固定，各方案只换强调色）
    titlebar.css           共享窗口标题栏部件（交通灯 / Windows 控制）
    switcher.css / .js      切换器：配色 × 明暗 × 平台(macOS/Windows)
    tokens.css             nexora.json 的 token 镜像（真源参照）
  screens/<界面名>/
    index.html             原型页
    mapping.md             区块 → gpui-component 组件的翻译映射 + 设计分歧记录
```

## 铁律

1. **每个窗口页面都必须有 TitleBar**，且随场景不同而不同——引入 `foundation/titlebar.css`，
   在标题栏区放 `.traffic`（左，macOS 交通灯）和 `.winctl`（右，Windows 控制）。
   - macOS：交通灯占窗口左上约 65–80px，任何贴左上的内容（品牌/收起态侧边栏）必须让开这段宽度。
   - Windows/Linux：最小/最大/关闭在右上端，close hover 变红。
   - 对应真实机制见 `crates/…/title_bar.rs`（macOS 系统原生绘制、appears_transparent；
     Windows app 自绘 ControlIcon）。翻译时用 gpui-component `TitleBar`，**不手绘**。
2. **只引用 token 变量，禁止裸十六进制色值**（颜色/圆角/字号来自 palettes.css）。
3. **每个界面配 `mapping.md`**：逐区块标注对应哪个 gpui-component 现成组件；确需自定义的，
   记录查过哪些候选、为什么不满足（对齐 AGENTS.md）。
4. **CrudPanel 等是「能力参照」非「视觉枷锁」**：可以做更现代的布局，但功能必须能映射回真实
   API；对框架的增强点（新槽位等）在 mapping 里标明「需另立规格」。
5. **不改真实主题**：`nexora.json` 在原型阶段只读；配色/层次定稿后才同步，且需用户确认。

## 现代设计倾向（此项目已确认方向）

- 现代克制风：焦点面唯一（主内容用 Card 浮起），其余靠留白+字体+层次，不给每块描边。
- 层次三级台阶：chrome（侧边栏/标题栏）→ canvas（内容画布）→ surface（Card，浮起+阴影）。
- Shell chrome 采 VS Code/Zed 模型：全局工具栏（左收起/中搜索/右全局按钮）独立一行、
  标签页下移、面包屑细带随激活标签变。

## 自查

改完用 Chrome headless 截图核对，注入 `data-palette`/`data-theme`/`data-os` 到 `<html>`
并移除 switcher.js（否则被 localStorage 覆盖）。核对后删除临时文件。

详见 `prototype/README.md`。
