# Action 与快捷键

**目录：** [概述](#概述) · [快速开始](#快速开始) · [按键格式](#按键格式) · [Action 命名](#action-命名) · [上下文感知绑定](#上下文感知绑定) · [最佳实践](#最佳实践)

## 概述

Action 为 GPUI 提供声明式的键盘驱动界面交互。

**关键概念：**

- 使用 `actions!` 宏或 `#[derive(Action)]` 定义 Action
- 使用 `cx.bind_keys()` 绑定按键
- 在元素上使用 `.on_action()` 处理 Action
- 通过 `key_context()` 实现上下文感知

## 快速开始

### 简单 Action

```rust
use gpui::actions;

actions!(editor, [MoveUp, MoveDown, Save, Quit]);

const CONTEXT: &str = "Editor";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", MoveUp, Some(CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(CONTEXT)),
        KeyBinding::new("cmd-s", Save, Some(CONTEXT)),
        KeyBinding::new("cmd-q", Quit, Some(CONTEXT)),
    ]);
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::save))
    }
}

impl Editor {
    fn move_up(&mut self, _: &MoveUp, cx: &mut Context<Self>) {
        // 处理向上移动
        cx.notify();
    }

    fn move_down(&mut self, _: &MoveDown, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn save(&mut self, _: &Save, cx: &mut Context<Self>) {
        // 保存逻辑
        cx.notify();
    }
}
```

### 带参数的 Action

```rust
#[derive(Clone, PartialEq, Action, Deserialize)]
#[action(namespace = editor)]
pub struct InsertText {
    pub text: String,
}

#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = editor, no_json)]
pub struct Digit(pub u8);

cx.bind_keys([
    KeyBinding::new("0", Digit(0), Some(CONTEXT)),
    KeyBinding::new("1", Digit(1), Some(CONTEXT)),
    // ...
]);

impl Editor {
    fn on_digit(&mut self, action: &Digit, cx: &mut Context<Self>) {
        self.insert_digit(action.0, cx);
    }
}
```

## 按键格式

```rust
// Modifiers
"cmd-s"         // Command（macOS）/ Ctrl（Windows/Linux）
"ctrl-c"        // Control
"alt-f"         // Alt
"shift-tab"     // Shift
"cmd-ctrl-f"    // 多个修饰键

// Keys
"a-z", "0-9"    // 字母和数字
"f1-f12"        // 功能键
"up", "down", "left", "right"
"enter", "escape", "space", "tab"
"backspace", "delete"
"-", "=", "[", "]" 等     // 特殊字符
```

## Action 命名

优先采用“动词-名词”模式：

```rust
actions!([
    OpenFile,      // ✅ Good
    CloseWindow,   // ✅ Good
    ToggleSidebar, // ✅ Good
    Save,          // ✅ 合适（常见例外）
]);
```

## 上下文感知绑定

```rust
const EDITOR_CONTEXT: &str = "Editor";
const MODAL_CONTEXT: &str = "Modal";

// 同一按键，不同上下文
cx.bind_keys([
    KeyBinding::new("escape", CloseModal, Some(MODAL_CONTEXT)),
    KeyBinding::new("escape", ClearSelection, Some(EDITOR_CONTEXT)),
]);

// 在元素上设置上下文
div()
    .key_context(EDITOR_CONTEXT)
    .child(editor_content)
```

## 最佳实践

### ✅ 使用上下文

```rust
// ✅ 合适：感知上下文
div()
    .key_context("MyComponent")
    .on_action(cx.listener(Self::handle))
```

### ✅ 清晰命名 Action

```rust
// ✅ 合适：意图清晰
actions!([
    SaveDocument,
    CloseTab,
    TogglePreview,
]);
```

### ✅ 使用监听器处理

```rust
// ✅ 合适：处理器命名恰当
impl MyComponent {
    fn on_action_save(&mut self, _: &Save, cx: &mut Context<Self>) {
        // 处理保存
        cx.notify();
    }
}

div().on_action(cx.listener(Self::on_action_save))
```
